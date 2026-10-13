"""DECISIONS 103: the foreign-organic sign (C1) also fires on the organic-film contrast (peak 2299 +- 3 nm minus trough
2325 +- 3 nm) at >= 17.8, full variant only. Hand-written expectations (LESSONS 44) on synthetic E arrays; the same cases
run in Rust (crates/spyder-core/tests/c1_film.rs)."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
import pytest

REF = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REF))
from oracle import checks as CK  # noqa: E402

P = json.loads((REF.parent / "plugins" / "checks" / "check.c1.json").read_text(encoding="utf-8"))["parameters"]
WL = 350.0 + np.arange(2151)


def spectrum(peak):
    return np.where((WL >= 2296) & (WL <= 2302), float(peak), 0.0)[None, :]


@pytest.mark.parametrize("peak, want", [(20.0, True), (17.8, True), (17.0, False), (0.0, False)])
def test_film_fires_c1_at_its_cut_and_not_below(peak, want):
    r = CK.c1(WL, spectrum(peak), P, None, False, False, "truncated")
    assert r["status"] == "assessed"
    assert r["film"] == pytest.approx(peak, abs=1e-9)
    assert r["film_fired"] is want and r["fired"] is want
    assert r["excess"] < 8.0


def test_film_does_not_run_in_the_truncated_variant_or_when_gated():
    r = CK.c1(WL, spectrum(50.0), P, None, False, True, "truncated")
    assert r["status"].startswith("assessed: truncated")
    assert r["film"] is None and r["film_fired"] is None and r["fired"] is False
    g = CK.c1(WL, spectrum(50.0), P, "B6b too noisy", False, False, "truncated")
    assert g["film_fired"] is None and g["fired"] is None
