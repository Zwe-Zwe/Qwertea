"""Rebuild the prose corpus (see `qwertea_engine.corpus_build`).

uv run python scripts/fetch_corpus.py [--out DIR] [--from-dir DIR]
"""

from qwertea_engine.corpus_build import main

if __name__ == "__main__":
    raise SystemExit(main())
