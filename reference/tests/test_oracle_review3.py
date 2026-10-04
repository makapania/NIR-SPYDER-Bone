"""Regressions for the Codex review of the Phase 3 commit (d39e88f), on the frozen oracle. The same cases run
against the Rust pipeline in crates/spyder-core/tests/codex_phase3.rs. Run from reference/: python -m pytest tests.
Public data only."""
from __future__ import annotations

import json
import shutil
import sys
import tempfile
from pathlib import Path

import numpy as np

REF = Path(__file__).resolve().parents[1]
PLUG = REF.parent / "plugins"
sys.path.insert(0, str(REF))
from oracle import Oracle, checks  # noqa: E402

WL = np.arange(350, 2501).astype(float)
STD = "asd.labspec4.std"
CTX = {"instrument_class": STD, "class_source": "user", "splices_nm": [1000.0, 1800.0]}
ORACLE = Oracle([str(PLUG)])


def golden(name):
    g = json.loads((PLUG / "golden_spectra_public_v2.json").read_text(encoding="utf-8"))
    return np.asarray(next(s for s in g["spectra"] if s["name"] == name)["values"], float)


def test_each_profile_uses_its_own_verdict_model():
    with tempfile.TemporaryDirectory() as td:
        d = Path(td) / "plugins"
        shutil.copytree(PLUG, d)
        f = d / "profiles" / "profile.isotopes.json"
        p = json.loads(f.read_text(encoding="utf-8"))
        p["parameters"]["models"] = [{"key": "ryder2045", "model_id": "collagen.ryder2026.2045",
                                      "role": "verdict_input", "components": [],
                                      "shown_for_classes": [STD, "asd.labspec4.hires"]}]
        f.write_text(json.dumps(p), encoding="utf-8")
        rec = Oracle([str(d)]).analyse_spectrum(WL, golden("mmc2_Reference_Sample_79"), CTX)
    cons3, ryder = rec["models"]["cons3"]["value"], rec["models"]["ryder2045"]["value"]
    assert abs(cons3 - ryder) > 1e-6
    assert rec["radiocarbon"]["sort_value"] == cons3
    assert rec["isotopes"]["sort_value"] == ryder


def test_empty_or_short_spectra_are_unsupported():
    x = golden("mmc2_Reference_Sample_79")
    for w, r in ((np.array([]), np.array([])), (WL, x[:100]), (WL[:100], x[:100]), (WL, np.array([]))):
        rec = ORACLE.analyse_spectrum(w, r, CTX)
        assert rec["radiocarbon"]["verdict"] == "Unsupported"
        assert rec["checks"]["B1"]["reason"] == "unsupported: wavelength grid"


def test_non_finite_heat_reading_is_not_assessed():
    x = golden("mmc2_Reference_Sample_79").copy()
    x[620:640] = np.nan                                   # 970-989 nm, outside B3's range
    rec = ORACLE.analyse_spectrum(WL, x, CTX)
    assert rec["checks"]["B3"]["outcome"] == "ok"
    b = rec["signs"]["burnt"]
    assert b["status"] == "not assessed: non-finite reading" and b["fired"] is None and b["calcined"] is None
    P = ORACLE.P
    s = checks.signs(WL, np.full(2151, np.nan), P.cp("signs"), P.cp("heat"), False)
    for n in ("plaster", "wax", "ester", "burnt"):
        assert s[n]["status"].startswith("not assessed") and s[n]["fired"] is None, n
    E = np.full((1, 2151), np.nan)
    ev = checks.evidence(WL, E, P.cp("evidence_levels"), P.bands(), {})
    assert ev["status"].startswith("not assessed") and ev["level"] is None
    zo = checks.zooms(WL, E, P.cp("zooms_patterns"), P.bands(), {})
    assert zo["status"].startswith("not assessed") and zo["verdict"] == "Can't tell"


def test_clear_noisy_nh2044_is_readable():
    P = ORACLE.P
    E = np.zeros((1, 2151))
    E[0, 2044 - 2 - 350:2044 + 3 - 350] = 3.2               # u = 2
    sd = {b: 0.01 for b in ("CH1728", "CH1689", "CH2262", "AM2175", "CH2284")}
    sd["NH2044"] = 0.528                                  # sd_u = 0.33 > 0.25
    ev = checks.evidence(WL, E, P.cp("evidence_levels"), P.bands(), sd)
    assert ev["bands"]["NH2044"]["state"] == "clear"
    assert ev["level"] == "trace"


def test_rescan_and_rejected_records_have_sort_keys():
    order = ORACLE.P.profile("radiocarbon").doc["parameters"]["sort"]["verdict_order"]
    panel = 1.0 + 0.002 * np.sin(np.arange(2151) * 0.37)
    rec = ORACLE.analyse_spectrum(WL, panel, CTX)
    assert rec["radiocarbon"]["verdict"] == "Rescan"
    assert order[rec["radiocarbon"]["sort_group"]] == "Rescan" and rec["radiocarbon"]["sort_value"] is None
    rec = ORACLE.analyse_spectrum(WL, golden("mmc2_Reference_Sample_79"), dict(CTX, splices_nm=[1000.0, 1830.0]))
    zorder = ORACLE.P.profile("zooms").doc["parameters"]["sort"]["verdict_order"]
    assert rec["zooms"]["verdict"] == "Unsupported" and rec["zooms"]["sort_group"] == len(zorder)
