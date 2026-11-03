//! Manual winit + egui-winit + egui-wgpu plumbing (no `eframe`).
//!
//! The application owns the winit event loop and stamps key events before
//! anything here sees them; this module only turns egui output into pixels.

use crate::config::PresentModePref;
use egui_wgpu::wgpu;
use std::sync::Arc;
use std::time::Duration;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

pub struct Gfx {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: egui_wgpu::Renderer,
    pub egui_state: egui_winit::State,
    pub ctx: egui::Context,
    pub present_mode: wgpu::PresentMode,
    pub adapter_info: String,
}

/// Map the preference to the best mode the surface supports:
/// preferred one first, then mailbox, immediate, fifo.
pub fn choose_present_mode(
    pref: PresentModePref,
    supported: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    use wgpu::PresentMode as P;
    let first = match pref {
        PresentModePref::Mailbox => P::Mailbox,
        PresentModePref::Immediate => P::Immediate,
        PresentModePref::Fifo => P::Fifo,
    };
    [first, P::Mailbox, P::Immediate, P::Fifo]
        .into_iter()
        .find(|m| supported.contains(m))
        .unwrap_or(P::Fifo)
}

impl Gfx {
    pub fn new(
        event_loop: &ActiveEventLoop,
        attrs: WindowAttributes,
        present: PresentModePref,
    ) -> Result<Self, String> {
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| e.to_string())?);
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("create_surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("request_adapter: {e}"))?;
        let info = adapter.get_info();
        let adapter_info = format!("{} ({:?})", info.name, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("qwertea"),
            required_limits:
                wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface is not supported by the adapter")?;
        let caps = surface.get_capabilities(&adapter);
        config.present_mode = choose_present_mode(present, &caps.present_modes);
        config.format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(config.format);
        config.desired_maximum_frame_latency = 1;
        surface.configure(&device, &config);

        let renderer = egui_wgpu::Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );
        let ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        Ok(Self {
            present_mode: config.present_mode,
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            egui_state,
            ctx,
            adapter_info,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Forward a window event to egui. Call this only *after* stamping.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> egui_winit::EventResponse {
        if let WindowEvent::Resized(s) = event {
            self.resize(s.width, s.height);
        }
        self.egui_state.on_window_event(&self.window, event)
    }

    /// Run one egui pass and present it. Returns the delay egui asks for
    /// before the next repaint (`Duration::MAX` = only on input).
    pub fn frame(&mut self, mut run_ui: impl FnMut(&mut egui::Ui)) -> Duration {
        let raw = self.egui_state.take_egui_input(&self.window);
        let out = self.ctx.run_ui(raw, |ui| run_ui(ui));
        self.egui_state
            .handle_platform_output(&self.window, out.platform_output);
        let repaint = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .unwrap_or(Duration::MAX);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return repaint;
            }
            _ => return repaint,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let ppp = out.pixels_per_point;
        let prims = self.ctx.tessellate(out.shapes, ppp);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ppp,
        };
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui"),
            });
        let extra =
            self.renderer
                .update_buffers(&self.device, &self.queue, &mut enc, &prims, &screen);
        {
            let bg = self.ctx.global_style().visuals.panel_fill;
            let [r, g, b, _] = bg.to_normalized_gamma_f32();
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: r as f64,
                            g: g as f64,
                            b: b as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &prims, &screen);
        }
        self.queue
            .submit(extra.into_iter().chain(std::iter::once(enc.finish())));
        self.window.pre_present_notify();
        self.queue.present(frame);
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
        repaint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::PresentMode as P;

    #[test]
    fn present_mode_fallback_order() {
        assert_eq!(
            choose_present_mode(PresentModePref::Mailbox, &[P::Fifo, P::Immediate]),
            P::Immediate
        );
        assert_eq!(
            choose_present_mode(PresentModePref::Mailbox, &[P::Fifo]),
            P::Fifo
        );
        assert_eq!(
            choose_present_mode(PresentModePref::Fifo, &[P::Mailbox, P::Fifo]),
            P::Fifo
        );
        assert_eq!(
            choose_present_mode(PresentModePref::Immediate, &[P::Mailbox, P::Immediate]),
            P::Immediate
        );
    }
}
