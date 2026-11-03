"""Paths and `config.toml` loading, for both a source checkout and an installed package.

Data shipped with the engine (schema, layouts, bundled corpus, default config)
lives inside the package when installed from a wheel, and at its usual place
in the repository during development.

Config resolution order (the client uses the same order):
  1. `$QWERTEA_CONFIG`;
  2. a source checkout: `<repo>/config.toml` next to `<repo>/schema/`;
  3. the user config `$XDG_CONFIG_HOME/qwertea/config.toml` (`~/.config/...`),
     created on first use with the database in `$XDG_DATA_HOME/qwertea/`.
"""

from __future__ import annotations

import os
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

PACKAGE_DIR = Path(__file__).resolve().parent
ENGINE_DIR = PACKAGE_DIR.parent
REPO_ROOT = ENGINE_DIR.parent
APP = "qwertea"


def _first_dir(*candidates: Path) -> Path:
    return next((c for c in candidates if c.is_dir()), candidates[-1])


# Packaged location first (wheel install), then the repository layout.
SCHEMA_DIR = _first_dir(PACKAGE_DIR / "schema", REPO_ROOT / "schema")
LAYOUTS_DIR = _first_dir(PACKAGE_DIR / "data" / "layouts", ENGINE_DIR / "data" / "layouts")
BUNDLED_DIR = _first_dir(PACKAGE_DIR / "data" / "bundled", ENGINE_DIR / "data" / "bundled")
_PACKAGED_CONFIG = PACKAGE_DIR / "default_config.toml"
_REPO_CONFIG = REPO_ROOT / "config.toml"


def is_source_checkout() -> bool:
    return _REPO_CONFIG.is_file() and (REPO_ROOT / "schema").is_dir()


def _home() -> Path:
    return Path(os.environ.get("HOME") or os.path.expanduser("~"))


def user_config_dir() -> Path:
    if sys.platform == "win32":
        return Path(os.environ.get("APPDATA", _home())) / APP
    if sys.platform == "darwin":
        return _home() / "Library" / "Application Support" / APP
    return Path(os.environ.get("XDG_CONFIG_HOME") or _home() / ".config") / APP


def user_data_dir() -> Path:
    if sys.platform in ("win32", "darwin"):
        return user_config_dir()
    return Path(os.environ.get("XDG_DATA_HOME") or _home() / ".local" / "share") / APP


CORPORA_DIR = (
    ENGINE_DIR / "data" / "corpora" if is_source_checkout() else user_data_dir() / "corpora"
)


def default_config_text() -> str:
    src = _PACKAGED_CONFIG if _PACKAGED_CONFIG.is_file() else _REPO_CONFIG
    return src.read_text(encoding="utf-8") if src.is_file() else ""


def user_config_text(template: str, data_dir: Path) -> str:
    """The default config, adapted for an installed copy: absolute database path
    and the installed `qwertea-engine` command."""
    db = (data_dir / "qwertea.db").as_posix()
    text = re.sub(r"(?m)^db_path\s*=.*$", f'db_path = "{db}"', template)
    return re.sub(r"(?m)^command\s*=.*$", 'command = ["qwertea-engine", "serve"]', text)


def ensure_user_config() -> Path:
    path = user_config_dir() / "config.toml"
    if not path.is_file():
        path.parent.mkdir(parents=True, exist_ok=True)
        user_data_dir().mkdir(parents=True, exist_ok=True)
        path.write_text(user_config_text(default_config_text(), user_data_dir()), encoding="utf-8")
    return path


@dataclass(frozen=True)
class Config:
    """Parsed `config.toml`. `root` is the directory holding the file."""

    root: Path
    raw: dict[str, Any] = field(default_factory=dict)
    path: Path | None = None

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
        p = Path(os.path.expanduser(str(self.get("general", "db_path", "data/qwertea.db"))))
        return p if p.is_absolute() else self.root / p


def config_path() -> Path:
    env = os.environ.get("QWERTEA_CONFIG")
    if env:
        return Path(env)
    if is_source_checkout():
        return _REPO_CONFIG
    return ensure_user_config()


def load_config(path: Path | None = None) -> Config:
    """Load the given path, or resolve it as described in the module docstring."""
    path = path or config_path()
    if not path.is_file():
        return Config(root=path.parent, path=path)
    with path.open("rb") as f:
        return Config(root=path.resolve().parent, raw=tomllib.load(f), path=path)


def corpus_dir(mode: str = "prose") -> Path:
    """Fetched corpus (`<corpora>/<mode>/`) if present, else the bundled copy."""
    fetched = CORPORA_DIR / mode
    if (fetched / "sentences.txt.gz").is_file() and (fetched / "wordfreq.tsv").is_file():
        return fetched
    return BUNDLED_DIR
