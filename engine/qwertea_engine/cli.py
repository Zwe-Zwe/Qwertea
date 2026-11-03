"""`qwertea-engine` command-line interface (section 8.6)."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Annotated

import typer

from . import db
from .layouts import Layout
from .paths import ENGINE_DIR, Config, load_config
from .pipeline import Engine, Settings

app = typer.Typer(no_args_is_help=True, add_completion=False, help="Qwertea analysis engine.")

ConfigOpt = Annotated[
    Path | None, typer.Option("--config", help="Path to config.toml (default: repo root).")
]


def _engine(config: Path | None, seed: int | None = None) -> tuple[Config, Engine]:
    cfg = load_config(config)
    conn = db.connect(cfg.db_path)
    return cfg, Engine(conn, Settings.from_config(cfg), seed=seed)


def _reports_dir(cfg: Config) -> Path:
    return cfg.db_path.parent / "reports"


@app.callback()
def main() -> None:
    """Qwertea analysis engine."""


@app.command()
def init(config: ConfigOpt = None) -> None:
    """Create the database from the schema and load the layout."""
    cfg = load_config(config)
    layout = Layout.load(cfg.layout)
    conn = db.connect(cfg.db_path)
    typer.echo(f"database: {cfg.db_path} (schema version {db.schema_version(conn)})")
    typer.echo(f"layout: {layout.name} ({len(layout.chars())} characters)")
    typer.echo(f"pending lessons: {db.count_pending(conn)}")


@app.command("fetch-corpus")
def fetch_corpus() -> None:
    """Download public-domain books and rebuild the prose corpus (data/corpora/prose)."""
    script = ENGINE_DIR / "scripts" / "fetch_corpus.py"
    raise typer.Exit(subprocess.call([sys.executable, str(script)]))


@app.command("plan-calibration")
def plan_calibration(
    config: ConfigOpt = None,
    force: Annotated[bool, typer.Option(help="Queue a new plan even if one exists.")] = False,
) -> None:
    """Create the calibration lessons (pending) and print the coverage report."""
    _, eng = _engine(config)
    n, report = eng.plan_calibration(force=force)
    typer.echo(
        f"queued {n} calibration lessons" if n else "calibration already planned (use --force)"
    )
    typer.echo(json.dumps(report, indent=2))


@app.command("run-once")
def run_once(config: ConfigOpt = None) -> None:
    """Process new finished lessons and refill the lesson queue."""
    _, eng = _engine(config)
    typer.echo(json.dumps(eng.run_once().__dict__))


def _write_report(cfg: Config, eng: Engine) -> Path:
    from .report import build

    return build(eng.conn, _reports_dir(cfg), eng.layout, eng.s.mode)


@app.command()
def serve(
    config: ConfigOpt = None,
    interval: Annotated[float, typer.Option(help="Polling interval in seconds.")] = 2.0,
    once_report: Annotated[
        bool, typer.Option("--report/--no-report", help="Refresh the HTML report after refits.")
    ] = True,
) -> None:
    """Poll the database and run `run-once` whenever there is work (Ctrl+C to stop)."""
    cfg, eng = _engine(config)
    lock = cfg.db_path.parent / "engine.pid"
    lock.write_text(str(os.getpid()))
    typer.echo(f"qwertea-engine serving {cfg.db_path} (pid {os.getpid()})", err=True)
    last_report = 0.0
    try:
        while True:
            try:
                res = eng.run_once()
                if res.derived or res.queued or res.calibration_planned:
                    typer.echo(json.dumps(res.__dict__), err=True)
                if (
                    once_report
                    and (res.refit or not (_reports_dir(cfg) / "index.html").exists())
                    and time.monotonic() - last_report > 20
                ):
                    _write_report(cfg, eng)
                    last_report = time.monotonic()
            except Exception as e:  # keep serving; the next poll retries
                typer.echo(f"engine error: {e!r}", err=True)
            time.sleep(interval)
    except KeyboardInterrupt:
        pass
    finally:
        lock.unlink(missing_ok=True)


@app.command()
def rebuild(config: ConfigOpt = None) -> None:
    """Drop and re-derive keystrokes and item_stats from raw events."""
    _, eng = _engine(config)
    typer.echo(json.dumps(eng.rebuild().__dict__))


@app.command()
def report(config: ConfigOpt = None) -> None:
    """Write the HTML report to data/reports/index.html."""
    cfg, eng = _engine(config)
    eng.run_once()
    typer.echo(str(_write_report(cfg, eng)))


@app.command()
def recalibrate(config: ConfigOpt = None) -> None:
    """Queue a mini-recalibration block (fixed probe text) at the front of the queue."""
    _, eng = _engine(config)
    eng.recalibrate()
    typer.echo("queued 1 checkpoint lesson")


if __name__ == "__main__":
    app()
