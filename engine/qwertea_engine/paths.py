"""Repository paths and `config.toml` loading."""

from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

PACKAGE_DIR = Path(__file__).resolve().parent
ENGINE_DIR = PACKAGE_DIR.parent
REPO_ROOT = ENGINE_DIR.parent
SCHEMA_DIR = REPO_ROOT / "schema"
LAYOUTS_DIR = ENGINE_DIR / "data" / "layouts"


@dataclass(frozen=True)
class Config:
    """Parsed `config.toml`. `root` is the directory holding the file."""

    root: Path
    raw: dict[str, Any] = field(default_factory=dict)

    def get(self, section: str, key: str, default: Any = None) -> Any:
        return self.raw.get(section, {}).get(key, default)

    @property
    def mode(self) -> str:
        return str(self.get("general", "mode", "prose"))

    @property
    def layout(self) -> str:
        return str(self.get("general", "layout", "qwerty-us"))

    @property
    def db_path(self) -> Path:
        p = Path(self.get("general", "db_path", "data/qwertea.db"))
        return p if p.is_absolute() else self.root / p


def load_config(path: Path | None = None) -> Config:
    """Load `$QWERTEA_CONFIG`, the given path, or `<repo>/config.toml`."""
    if path is None:
        env = os.environ.get("QWERTEA_CONFIG")
        path = Path(env) if env else REPO_ROOT / "config.toml"
    if not path.is_file():
        return Config(root=path.parent)
    with path.open("rb") as f:
        return Config(root=path.resolve().parent, raw=tomllib.load(f))


BUNDLED_DIR = ENGINE_DIR / "data" / "bundled"
CORPORA_DIR = ENGINE_DIR / "data" / "corpora"


def corpus_dir(mode: str = "prose") -> Path:
    """Fetched corpus (`data/corpora/<mode>/`) if present, else the bundled copy."""
    fetched = CORPORA_DIR / mode
    if (fetched / "sentences.txt.gz").is_file() and (fetched / "wordfreq.tsv").is_file():
        return fetched
    return BUNDLED_DIR
