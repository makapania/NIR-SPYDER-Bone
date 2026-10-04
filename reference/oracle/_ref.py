"""Locate the frozen reference engine `spyder_ref` (model files, transfer files, golden runner).

Search order:
  1. the directory named by the environment variable SPYDER_REF_PATH (a folder holding spyder_ref.py, or a folder
     holding the spyder_ref package);
  2. <repo>/reference/spyder_ref/ (a folder holding spyder_ref.py), then <repo>/reference/ (the spyder_ref package).
The oracle never re-implements model or transfer chains: it calls spyder_ref.predict / apply_transfer.
"""
from __future__ import annotations

import importlib
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REFERENCE = HERE.parent


def _load():
    cands = []
    env = os.environ.get("SPYDER_REF_PATH")
    if env:
        cands.append(Path(env))
    cands += [REFERENCE / "spyder_ref", REFERENCE]
    for c in cands:
        if (c / "spyder_ref.py").is_file() or (c / "spyder_ref" / "__init__.py").is_file():
            if str(c) not in sys.path:
                sys.path.insert(0, str(c))
            mod = importlib.import_module("spyder_ref")
            if hasattr(mod, "predict"):
                return mod
            for sub in ("spyder_ref", "core", "engine"):        # a package that keeps the engine in a submodule
                try:
                    m2 = importlib.import_module(f"spyder_ref.{sub}")
                    if hasattr(m2, "predict"):
                        return m2
                except ImportError:
                    pass
    raise ImportError("spyder_ref not found: set SPYDER_REF_PATH or install reference/spyder_ref")


SR = _load()
