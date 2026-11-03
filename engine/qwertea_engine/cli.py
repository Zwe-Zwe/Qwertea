"""`qwertea-engine` command-line interface.

Implemented so far (M1): `init`. Further commands arrive with their milestones.
"""

from __future__ import annotations

from pathlib import Path
from typing import Annotated

import typer

from . import db
from .layouts import Layout
from .paths import load_config

app = typer.Typer(no_args_is_help=True, add_completion=False, help="Qwertea analysis engine.")

ConfigOpt = Annotated[
    Path | None, typer.Option("--config", help="Path to config.toml (default: repo root).")
]


@app.callback()
def main() -> None:
    """Qwertea analysis engine."""


@app.command()
def init(config: ConfigOpt = None) -> None:
    """Create the database from the schema and load the layout."""
    cfg = load_config(config)
    layout = Layout.load(cfg.layout)
    conn = db.connect(cfg.db_path)
    version = db.schema_version(conn)
    typer.echo(f"database: {cfg.db_path} (schema version {version})")
    typer.echo(f"layout: {layout.name} ({len(layout.chars())} characters)")
    typer.echo(f"pending lessons: {db.count_pending(conn)}")
    conn.close()


if __name__ == "__main__":
    app()
