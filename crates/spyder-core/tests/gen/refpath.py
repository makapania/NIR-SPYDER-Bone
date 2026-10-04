"""Locate the frozen Python reference engine `spyder_ref` (numpy only) for the generators in this folder.

Search order: $SPYDER_REF_DIR (a folder holding the `spyder_ref` package or a single-file spyder_ref.py), then the
repository package <repo>/reference/spyder_ref. Returns (module, the file whose SHA-256 identifies the engine).
"""
import importlib
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]


def import_spyder_ref():
    cands = []
    if os.environ.get("SPYDER_REF_DIR"):
        cands.append(Path(os.environ["SPYDER_REF_DIR"]))
    cands.append(REPO / "reference")
    for c in cands:
        if (c / "spyder_ref" / "__init__.py").is_file():
            sys.path.insert(0, str(c))
            sr = importlib.import_module("spyder_ref")
            return sr, c / "spyder_ref" / "core.py"
        if (c / "spyder_ref.py").is_file():
            sys.path.insert(0, str(c))
            sr = importlib.import_module("spyder_ref")
            return sr, c / "spyder_ref.py"
    raise SystemExit("spyder_ref not found; set SPYDER_REF_DIR")
