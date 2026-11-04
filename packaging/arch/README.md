# Arch Linux packages

| Package | Builds | Use it for |
|---|---|---|
| `qwertea` (`PKGBUILD`) | the tagged release (`v1.0.0`) | normal installs |
| `qwertea-git` (`qwertea-git/PKGBUILD`) | the latest `main` | trying unreleased changes |

## Install from this repository

```bash
cd packaging/arch            # or packaging/arch/qwertea-git
makepkg -si
```

`makepkg` builds the app (Rust, with kernel-timestamp support) and the analysis engine (Python wheel), then installs:

| Path | What |
|---|---|
| `/usr/bin/qwertea` | the app |
| `/usr/bin/qwertea-engine` | the engine; the app starts it for you |
| `/usr/lib/python3.*/site-packages/qwertea_engine/` | engine code, schema, keyboard layout and bundled corpus |
| `/usr/share/applications/qwertea.desktop` | launcher entry |
| `/usr/share/icons/hicolor/scalable/apps/qwertea.svg` | icon |

Your settings are created on first run in `~/.config/qwertea/config.toml` and your data lives in `~/.local/share/qwertea/`. Uninstalling the package leaves both alone.

The explainable insight model in the report needs `python-lightgbm` from the AUR:

```bash
yay -S python-lightgbm      # optional
```

## Troubleshooting

**`rust-lld: error: undefined symbol: sqlite3_…` during `build()`**
Fixed in `qwertea 1.0.1-2`. makepkg's link-time-optimisation flags make GCC
emit LTO bitcode for the bundled SQLite, which Rust's linker cannot read, so
the PKGBUILDs set `options=('!lto')`. If you hit it, update the PKGBUILD and
rebuild from a clean source tree: `makepkg -Csi`.

## Publishing to the AUR (maintainer)

The AUR needs an account with your SSH key added under *My Account*.

```bash
git clone ssh://aur@aur.archlinux.org/qwertea.git aur-qwertea
cp packaging/arch/PKGBUILD aur-qwertea/
cd aur-qwertea
updpkgsums                  # fills in sha256sums (pacman-contrib)
makepkg --printsrcinfo > .SRCINFO
makepkg -si                 # test it
namcap PKGBUILD *.pkg.tar.zst
git add PKGBUILD .SRCINFO && git commit -m "qwertea 1.0.0" && git push
```

Do the same with `qwertea-git/PKGBUILD` in a separate `qwertea-git` AUR repository.

## Releasing a new version

1. Bump `version` in `client/Cargo.toml` and `engine/pyproject.toml`, and `__version__` in `engine/qwertea_engine/__init__.py`. Then run `cargo update -p qwertea-client` in `client/` and `uv lock` in `engine/`.
2. Add a section to `CHANGELOG.md`.
3. Set `pkgver` (and `pkgrel=1`) in `PKGBUILD`, then commit and tag: `git tag -a vX.Y.Z -m "Qwertea X.Y.Z"` and `git push origin main vX.Y.Z`.
4. Run `updpkgsums` and `makepkg --printsrcinfo > .SRCINFO`, commit, and push to the AUR.
