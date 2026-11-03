"""Download public-domain books and build the prose corpus.

Outputs (in `--out`, default `engine/data/corpora/prose/`):
  sentences.txt.gz  one clean sentence per line (printable ASCII, 30-220 chars)
  wordfreq.tsv      `word<TAB>count`, lower-case words, most frequent first

Source: the Project Gutenberg selection mirrored by NLTK (public-domain
texts; the Gutenberg licence header was already removed by NLTK). Only prose
books are used: no verse, plays or scripture.

The package ships a pre-built copy (`engine/data/bundled/`) so Qwertea works
offline; a fetched corpus takes precedence (`qwertea-engine fetch-corpus`).
"""

from __future__ import annotations

import argparse
import collections
import gzip
import io
import re
import sys
import urllib.request
import zipfile
from pathlib import Path

URL = "https://raw.githubusercontent.com/nltk/nltk_data/gh-pages/packages/corpora/gutenberg.zip"
BOOKS = [
    "austen-emma.txt",
    "austen-persuasion.txt",
    "austen-sense.txt",
    "bryant-stories.txt",
    "burgess-busterbrown.txt",
    "carroll-alice.txt",
    "chesterton-ball.txt",
    "chesterton-brown.txt",
    "chesterton-thursday.txt",
    "edgeworth-parents.txt",
    "melville-moby_dick.txt",
]

PRINTABLE = set(chr(c) for c in range(32, 127))
REPLACE = {
    "‘": "'",
    "’": "'",
    "“": '"',
    "”": '"',
    "—": "-",
    "–": "-",
    "…": "...",
    "_": "",
    "æ": "ae",
}
SPLIT = re.compile(r"(?<=[.!?])[\"']?\s+(?=[\"']?[A-Z])")
WORD = re.compile(r"[a-z]+(?:'[a-z]+)?")


def clean_paragraph(par: str) -> str:
    for a, b in REPLACE.items():
        par = par.replace(a, b)
    par = par.replace("--", " - ")
    return re.sub(r"\s+", " ", par).strip()


def good_sentence(s: str) -> bool:
    if not 30 <= len(s) <= 220:
        return False
    if not set(s) <= PRINTABLE:
        return False
    if not (s[0].isupper() or s[0] in "\"'"):
        return False
    if s[-1] not in ".!?\"'":
        return False
    if re.search(r"\b[A-Z]{3,}\b", s):  # headings, CHAPTER, roman numerals
        return False
    if re.search(r"[\[\]{}<>|\\#*@]|\d{4,}", s):
        return False
    return s.count("(") == s.count(")")


def normalise_quotes(s: str) -> str:
    if s.count('"') % 2:
        s = s.replace('"', "").strip()
    return s


def sentences_from(text: str) -> list[str]:
    out: list[str] = []
    for par in re.split(r"\n\s*\n", text):
        par = clean_paragraph(par)
        if not par:
            continue
        for s in SPLIT.split(par):
            s = normalise_quotes(s.strip())
            if good_sentence(s):
                out.append(s)
    return out


def build(texts: dict[str, str], out: Path) -> tuple[int, int]:
    out.mkdir(parents=True, exist_ok=True)
    seen: set[str] = set()
    sentences: list[str] = []
    counts: collections.Counter[str] = collections.Counter()
    for name in sorted(texts):
        text = texts[name]
        counts.update(WORD.findall(clean_paragraph(text).lower()))
        for s in sentences_from(text):
            key = s.lower()
            if key not in seen:
                seen.add(key)
                sentences.append(s)
    with gzip.open(out / "sentences.txt.gz", "wt", encoding="ascii", compresslevel=9) as f:
        for s in sentences:
            f.write(s + "\n")
    words = [(w, c) for w, c in counts.most_common() if c >= 3 and len(w) <= 14]
    with (out / "wordfreq.tsv").open("w", encoding="ascii") as f:
        for w, c in words:
            f.write(f"{w}\t{c}\n")
    return len(sentences), len(words)


def download(url: str = URL) -> dict[str, str]:
    with urllib.request.urlopen(url, timeout=60) as r:
        data = r.read()
    texts: dict[str, str] = {}
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        for name in BOOKS:
            raw = z.read(f"gutenberg/{name}")
            texts[name] = raw.decode("utf-8", errors="replace").replace("\r\n", "\n")
    return texts


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    from .paths import CORPORA_DIR

    ap.add_argument("--out", type=Path, default=CORPORA_DIR / "prose")
    ap.add_argument("--from-dir", type=Path, help="use already-downloaded .txt files instead")
    args = ap.parse_args(argv)
    if args.from_dir:
        texts = {
            n: (args.from_dir / n).read_text(encoding="utf-8", errors="replace") for n in BOOKS
        }
    else:
        print(f"downloading {URL} ...", file=sys.stderr)
        texts = download()
    n_s, n_w = build(texts, args.out)
    print(f"wrote {n_s} sentences and {n_w} words to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
