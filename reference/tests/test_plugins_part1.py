"""Tests for the Phase 0b part-1 plug-in files and engine additions (consensus kind, shared tables).
Run from reference/:  python -m pytest tests  (or: python tests/test_plugins_part1.py). Public data only."""
from __future__ import annotations
import copy
import json
import sys
from pathlib import Path

import numpy as np

REF = Path(__file__).resolve().parents[1]
PLUG = REF.parent / "plugins"
sys.path.insert(0, str(REF))
import spyder_ref as SR                      # noqa: E402
from spyder_ref import tables as TB          # noqa: E402
from spyder_ref import gains as G            # noqa: E402
from spyder_ref import golden_gen as GG      # noqa: E402
from spyder_ref.validate_plugins import validate_folder, _schema_validator   # noqa: E402

WL = np.arange(350, 2501).astype(float)
load = lambda name: SR.load_json(PLUG / name)
GS = SR.load_golden_spectra(PLUG / "golden_spectra_public_v2.json")
SPEC = {v[2]["name"]: v for v in GS.values()}


def test_folder_validates():
    bad, report = validate_folder(PLUG, verbose=False)
    assert bad == 0, [r for r in report if "FAIL" in r[2] or "REJECT" in r[2] or "MISMATCH" in r[2]]


def test_ids_versions_status():
    want = {"collagen_2045_ryder2026_n140.spyder-model.json": ("collagen.ryder2026.2045", "2.0.0"),
            "collagen_2045_oh_corrected.spyder-model.json": ("collagen.spyder.2045_oh_corrected", "1.0.0"),
            "collagen_1500_oh_corrected.spyder-model.json": ("collagen.spyder.1500_oh_corrected", "1.0.0"),
            "collagen_nh3_oh_corrected.spyder-model.json": ("collagen.spyder.nh3_oh_corrected", "1.0.0"),
            "collagen_consensus3_median.spyder-model.json": ("collagen.spyder.consensus3_median", "1.0.0"),
            "collagen_1500_snv_transfer_free.spyder-model.json": ("collagen.spyder.1500_snv_transfer_free", "1.0.0"),
            "hires_to_std_v0_3.spyder-transfer.json": ("transfer.labspec4.hires_to_std", "0.3.0"),
            "hires_to_std_v0_2.spyder-transfer.json": ("transfer.labspec4.hires_to_std", "0.2.0"),
            "hires_to_std_v0_1.spyder-transfer.json": ("transfer.labspec4.hires_to_std", "0.1.0")}
    for f, (i, v) in want.items():
        d = load(f)
        assert (d["id"], d["version"], d["status"]) == (i, v, "active"), f
        assert tuple(int(x) for x in d["version"].split("."))                  # integer-parsable
    assert load("collagen_nh3_1500_2045_2175.spyder-model.json")["status"] == "withdrawn"   # F05, replaced by F05c (DECISIONS 78)
    for f in ("hires_to_std_v0_3.spyder-transfer.json", "hires_to_std_v0_2.spyder-transfer.json",
              "hires_to_std_v0_1.spyder-transfer.json"):
        assert load(f)["provisional"] is True
    # superseded files stay as history, marked as v0.1 was when v0.2 arrived (title, summary, superseded_by)
    for f, new in (("hires_to_std_v0_1.spyder-transfer.json", "0.2.0"), ("hires_to_std_v0_2.spyder-transfer.json", "0.3.0")):
        d = load(f)
        assert d["title"].endswith(f"; superseded by {new})") and f"superseded by {new}):" in d["provenance"]["summary"]
        assert d["provenance"]["superseded_by"] == f"transfer.labspec4.hires_to_std {new}"
    assert "superseded_by" not in load("hires_to_std_v0_3.spyder-transfer.json")["provenance"]


def test_transfer_v02_constants_and_selection():
    t2, t1 = load("hires_to_std_v0_2.spyder-transfer.json"), load("hires_to_std_v0_1.spyder-transfer.json")
    aff = t2["operator"][1]
    assert aff["gain"] == [0.6343, 0.705, 0.6258] and aff["offset"] == [0.2312, 0.2073, 0.2453] and aff["clip_min"] == 1e-6
    assert t2["operator"][0]["sigma_nm"] == [0.0, 3.3973, 3.3973] and t2["operator"][0]["radius"] == "ceil"
    cons = load("collagen_consensus3_median.spyder-model.json")
    hi = SR.ScanContext((1000.0, 1800.0), "asd.labspec4.hires")
    assert SR.pick_transfer(cons, hi, [t1, t2])["version"] == "0.2.0"            # newest active wins
    assert SR.pick_transfer(cons, SR.ScanContext(), [t1, t2]) == "none"
    assert SR.pick_transfer(load("collagen_1500_snv_transfer_free.spyder-model.json"), hi, [t1, t2]) == "none"


def test_transfer_v03_constants_and_selection():
    """The rebuilt LabSpec high-res file (DECISIONS 137, general method, datasheet blur; REPORT_b1_builder section 9) is
    the newest active version of the id, so it reads every high-res scan."""
    t3 = load("hires_to_std_v0_3.spyder-transfer.json")
    t2, t1 = load("hires_to_std_v0_2.spyder-transfer.json"), load("hires_to_std_v0_1.spyder-transfer.json")
    assert SR.file_sha256(PLUG / "hires_to_std_v0_3.spyder-transfer.json").startswith("594ee6a6")
    blur, aff = t3["operator"]
    assert blur["op"] == "gaussian_blur" and blur["sigma_nm"] == [0.0, 3.3973, 3.3973] and blur["radius"] == "ceil"
    assert aff["gain"] == [0.5777, 0.6983, 0.5881] and aff["offset"] == [0.2742, 0.2005, 0.3552]
    assert t3["provenance"]["visible_levels"] == "fitted"
    assert t3["provenance"]["supersedes"].startswith("transfer.labspec4.hires_to_std 0.2.0")
    cons = load("collagen_consensus3_median.spyder-model.json")
    hi = SR.ScanContext((1000.0, 1800.0), "asd.labspec4.hires")
    assert SR.pick_transfer(cons, hi, [t1, t2, t3])["version"] == "0.3.0"        # newest active wins
    assert SR.pick_transfer(cons, hi, [t3, t2, t1])["version"] == "0.3.0"


def test_consensus_equals_components():
    cons = load("collagen_consensus3_median.spyder-model.json")
    comps = {k["name"]: load(k["file"]) for k in cons["consensus"]["components"]}
    for wl, x, sp in GS.values():
        out = SR.predict(cons, wl, x[None], SR.ScanContext())
        vals = []
        for name, d in comps.items():
            v = SR.predict(d, wl, x[None], SR.ScanContext())["value"][0]
            assert abs(v - out[f"component:{name}"][0]) <= 1e-9 * max(1, abs(v)), (sp["name"], name)
            vals.append(v)
        assert abs(sorted(vals)[1] - out["value"][0]) <= 1e-9 * max(1, abs(out["value"][0]))


def test_consensus_negative():
    cons = load("collagen_consensus3_median.spyder-model.json")
    bad = copy.deepcopy(cons); bad["consensus"]["components"] = bad["consensus"]["components"][:2]
    _raises(lambda: SR.check_model(bad), "odd number")
    bad = copy.deepcopy(cons); bad["engine_min"] = "1.0"
    _raises(lambda: SR.check_model(bad), "engine_min 1.1")
    bad = copy.deepcopy(cons); bad["consensus"]["combine"] = "mean"
    _raises(lambda: SR.check_model(bad), "median")
    bad = copy.deepcopy(cons); bad["consensus"]["components"][0]["feature"]["weights"][10] *= 1.001
    res = SR.run_goldens(bad, GS)
    assert any(not r[-1] for r in res)                                          # a perturbed weight fails its goldens
    bad = copy.deepcopy(cons); del bad["golden"]["cases"][0]["expected"]["component:F05"]
    _raises(lambda: SR.run_goldens(bad, GS), "missing")


def test_schema_negative():
    sv = _schema_validator()
    if sv is None:
        return
    d = json.loads((PLUG / "collagen_consensus3_median.spyder-model.json").read_text(encoding="utf-8")); d.pop("consensus")
    assert sv(d)
    t = json.loads((PLUG / "hires_to_std_v0_2.spyder-transfer.json").read_text(encoding="utf-8")); t.pop("provisional")
    assert sv(t)
    b = json.loads((PLUG / "bands.json").read_text(encoding="utf-8")); b["bands"]["CH2262"]["readability"]["gain_E_per_N2"] = {"none": 1.0}
    assert sv(b)                                                                  # format 2: no per-transfer gains
    n = json.loads((PLUG / "noise_gains.json").read_text(encoding="utf-8")); n["format_version"] = 1
    assert sv(n)


def test_tables_compute_gains_and_say_why_not():
    """Format 2 (DECISIONS 137): the gains are computed for the correction applied; a consensus SD is the factor x the
    median of its components' SDs; an output the engine cannot compute says why (never a silent default)."""
    ng = load("noise_gains.json"); bj = load("bands.json")
    t02 = load("hires_to_std_v0_2.spyder-transfer.json")
    ops, inn = G.correction_of(t02)
    wl, x, _ = SPEC["pair10_hires"]
    n2_of = lambda lo, hi: SR.n2(x, lo, hi)
    cons = load("collagen_consensus3_median.spyder-model.json")
    r = TB.implied_sd(ng, cons, wl, (1000.0, 1800.0), ops, inn, n2_of)
    comp = list(r["component_sd"].values())
    assert r["status"] == "assessed" and abs(r["sd"] - ng["consensus"][cons["id"]]["factor"] * sorted(comp)[1]) < 1e-12
    snv = ops + [{"op": "snv", "ddof": 0, "blocks_nm": [[1100.0, 1700.0]]}]
    ry = load("collagen_2045_ryder2026_n140.spyder-model.json")
    r = TB.implied_sd(ng, ry, wl, (1000.0, 1800.0), snv, inn, n2_of)
    assert r["sd"] is None and r["status"] == "not assessed: non-linear correction"
    sd, st = TB.readability_sd(bj, "CH2262", wl, (1000.0, 1800.0), snv, inn, n2_of)
    assert sd == float("inf") and st == "not assessed: non-linear correction"     # unknown noise is too noisy
    assert bj["bands"]["CH2262"]["readability"]["noise_window_nm"] == [2220, 2245]
    bad = copy.deepcopy(ng); bad["consensus"][cons["id"]]["factor"] *= 1.01
    assert any(not r[-1] for r in TB.run_table_goldens(bad, GS, PLUG))


def test_n2_matches_independent_loop():
    for name in ("synthetic_noise_N2_above60", "synthetic_noise_N2_below60", "pair22_hires"):
        wl, x, _ = SPEC[name]
        for lo, hi in ((2000, 2100), (1500, 1600), (2220, 2245), (2300, 2400)):
            assert abs(SR.n2(x, lo, hi) - GG.n2_loop(x, lo, hi)) < 1e-9
    assert SR.n2(SPEC["synthetic_noise_N2_above60"][1]) > 60 > SR.n2(SPEC["synthetic_noise_N2_below60"][1])


def test_b9_sidecars():
    P = np.load(PLUG / "checks" / "b9_prototypes_v1.npy", allow_pickle=False)
    Pt = np.load(PLUG / "checks" / "b9_prototypes_trunc_v1.npy", allow_pickle=False)
    assert SR.file_sha256(PLUG / "checks" / "b9_prototypes_v1.npy").startswith("89a0a43e")
    assert P.shape == (5, 762) and Pt.shape == (5, 662) and P.dtype == Pt.dtype == np.dtype("<f8")
    m = np.zeros(2151, bool); m[(WL >= 1350) & (WL <= 1560)] = True; m[(WL >= 1850) & (WL <= 2400)] = True
    t = np.zeros(2151, bool); t[(WL >= 1350) & (WL <= 1560)] = True; t[(WL >= 1850) & (WL <= 2300)] = True
    q = P[:, t[m]]; q = (q - q.mean(1, keepdims=True)) / q.std(1, keepdims=True)
    assert np.allclose(q, Pt, atol=1e-12)


def test_export_roundtrip(tmp_path=None):
    import shutil, tempfile
    d = Path(tempfile.mkdtemp()) if tmp_path is None else Path(tmp_path)
    shutil.copy(PLUG / "golden_spectra_public_v2.json", d / "golden_spectra_public_v2.json")
    ry = load("collagen_2045_ryder2026_n140.spyder-model.json")
    from spyder_ref.export import export_regression
    n = export_regression(d / "toy.spyder-model.json", id="test.toy.ryder_copy", version="1.0.0", title="toy",
                          chain=ry["preprocessing"], wavelengths_nm=ry["features"]["wavelengths_nm"],
                          coefficients=ry["regression"]["coefficients"], x_center=ry["regression"]["x_center"],
                          offset=ry["regression"]["offset"], golden_spectra_file=d / "golden_spectra_public_v2.json",
                          golden_names=["mmc2_Reference_Sample_1", "mmc2_Reference_Sample_79", "pair10_std"])
    assert n >= 6


def _raises(fn, text):
    try:
        fn()
    except SR.SpyderError as e:
        assert text in str(e), str(e)
        return
    raise AssertionError(f"expected SpyderError containing {text!r}")


if __name__ == "__main__":
    fails = 0
    for k, f in sorted(globals().items()):
        if k.startswith("test_") and callable(f):
            try:
                f(); print("PASS", k)
            except Exception as e:              # noqa: BLE001
                fails += 1; print("FAIL", k, type(e).__name__, e)
    sys.exit(1 if fails else 0)
