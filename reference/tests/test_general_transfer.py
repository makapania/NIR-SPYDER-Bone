"""DECISIONS 137 Phase 2 (part 1) on the frozen reference and oracle: noise and band-readability gains are computed
for ANY linear correction and the scan's own segments; unknown band noise is "too noisy" (fail safe), never
"readable"; a correction the engine cannot propagate noise through says so; a band or model whose kernel or noise
window does not sit in one detector segment is "not available on this instrument". The same cases run against the Rust
pipeline in crates/spyder-core/tests/general_transfer.rs and gains_parity.rs. Run from reference/:
python -m pytest tests. Public data only: the test corrections are the shipped v0.2 file with other operators."""
from __future__ import annotations

import copy
import json
import math
import sys
import tempfile
from pathlib import Path

import numpy as np

REF = Path(__file__).resolve().parents[1]
REPO = REF.parent
PLUG = REPO / "plugins"
sys.path.insert(0, str(REF))
import spyder_ref as SR                      # noqa: E402
from spyder_ref import gains as G            # noqa: E402
from oracle import Oracle, checks            # noqa: E402

WL = np.arange(350, 2501).astype(float)
STD, HIRES = "asd.labspec4.std", "asd.labspec4.hires"
READABLE = ["AM2175", "CH1689", "CH1728", "CH2262", "CH2284", "NH1545c", "NH2044"]
ORACLE = Oracle([str(PLUG)])
load = lambda name: SR.load_json(PLUG / name)


def golden(name):
    g = json.loads((PLUG / "golden_spectra_public_v2.json").read_text(encoding="utf-8"))
    return np.asarray(next(s for s in g["spectra"] if s["name"] == name)["values"], float)


def ctx(cls, joins=(1000.0, 1800.0)):
    return {"instrument_class": cls, "class_source": "user", "splices_nm": list(joins)}


TEST_KEY = "transfer.labspec4.hires_to_std@1.0.0"


def with_correction(ops, noise=None, tid="transfer.test.correction"):
    """The shipped plug-ins plus a user folder holding one test correction: v0.2 with these operators, the shipped id
    at version 1.0.0, so it replaces the bundled v0.3 for high-res scans (within one id the highest version wins; a
    class-wide file with another id would not replace the bundled one). `tid` names the test in the title. The oracle
    does not run transfer goldens."""
    t = load("hires_to_std_v0_2.spyder-transfer.json")
    t["version"], t["operator"], t["title"] = "1.0.0", ops, f"test correction {tid}"
    if noise is not None:
        t["provenance"]["instrument_noise"] = noise
    d = Path(tempfile.mkdtemp())
    (d / "test.spyder-transfer.json").write_text(json.dumps(t), encoding="utf-8")
    return Oracle([str(PLUG), str(d)])


V02 = [{"op": "gaussian_blur", "sigma_nm": [0.0, 3.3973, 3.3973], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"},
       {"op": "absorbance_affine", "gain": [0.6343, 0.705, 0.6258], "offset": [0.2312, 0.2073, 0.2453], "clip_min": 1e-6}]


def test_recorded_format_1_gains_are_reproduced():
    """The format-1 tables recorded each gain per transfer file (6 decimals; a C-H band's once more after its envelope
    factor); the computed gains for the same files give every recorded value through that rounding."""
    rec = json.loads((REPO / "crates/spyder-core/tests/goldens/recorded_gains_v1.json").read_text(encoding="utf-8"))
    ng, bd = load("noise_gains.json"), load("bands.json")
    models = {d["id"]: d for d in (SR.load_json(p) for p in PLUG.glob("*.spyder-model.json"))}
    corr = {k: (G.correction_of(None) if v is None else G.correction_of(load(v["file"])))
            for k, v in rec["corrections"].items()}
    for k, v in rec["corrections"].items():
        if v is not None:
            assert SR.file_sha256(PLUG / v["file"]) == v["sha256"]
    r6 = lambda x: round(x * 1e6) / 1e6
    n, worst = 0, 0.0
    for mid, keyed in rec["models"].items():
        for k, terms in keyed.items():
            got = dict(G.model_terms(ng, models[mid], WL, rec["joins_nm"], *corr[k])[None])
            for t in terms:
                g = got[tuple(t["window_nm"])]
                assert abs(r6(g) - t["gain"]) <= 1e-12, (mid, k, g, t["gain"])
                n += 1; worst = max(worst, abs(g - t["gain"]) / t["gain"])
    for b, keyed in rec["bands"].items():
        f = rec["envelope_factors"].get(b, 1.0)
        assert bd["bands"][b]["readability"].get("envelope_factor", 1.0) == f
        for k, want in keyed.items():
            win, g = G.band_gain(bd, b, WL, rec["joins_nm"], *corr[k])
            assert list(win) == rec["band_noise_windows_nm"][b]
            assert abs(r6(r6(g / f) * f) - want) <= 1e-12, (b, k, g, want)
            n += 1; worst = max(worst, abs(g - want) / want)
    assert n >= 36 and worst < 2.5e-4
    s1 = G.model_terms(ng, models["collagen.spyder.1500_snv_transfer_free"], WL, rec["joins_nm"])[None]
    assert s1[0][1] == rec["nonlinear_models"]["collagen.spyder.1500_snv_transfer_free"]["none"][0]["gain"]


def test_gains_parity_fixture_is_current():
    """The Rust parity fixture (tests/gen/gen_gains_parity.py) still describes this engine: every entry,
    recomputed, agrees to 1e-12 (Rust is held to the fixture at 1e-9)."""
    fx = json.loads((REPO / "crates/spyder-core/tests/goldens/gains_parity_v1.json").read_text(encoding="utf-8"))
    for f, sha in fx["plugin_files"].items():
        assert SR.file_sha256(PLUG / f) == sha, f"{f} changed: rerun gen_gains_parity.py"
    ng, bd = load("noise_gains.json"), load("bands.json")
    lay = {l["name"]: (l["wl_start"] + l["wl_step"] * np.arange(l["n"]), l["joins"]) for l in fx["layouts"]}
    models = {f: load(f) for f in fx["plugin_files"] if f.endswith(".spyder-model.json")}
    for e in fx["entries"]:
        wl, joins = lay[e["layout"]]
        c = fx["corrections"][e["correction"]]
        ops, inn = (None, {}) if c is None else (c["operator"], c.get("instrument_noise", {}))
        try:
            if "statistic" in e:
                want = e["result"]
                got = {"gain": G.stat_term_gain(fx["statistics"][e["statistic"]], wl, joins, ops, inn, 1.25,
                                                tuple(e["window"]), e["statistic"])}
            elif "model" in e:
                got = {("" if k is None else k): [[w[0], w[1], g] for w, g in v]
                       for k, v in G.model_terms(ng, models[e["model"]], wl, joins, ops, inn).items()}
                want = e["terms"]
            else:
                (lo, hi), g = G.band_gain(bd, e["band"], wl, joins, ops, inn)
                got, want = {"window": [lo, hi], "gain": g}, e["gain"]
        except G.GainError as err:
            got = {"error": err.status}
            want = e.get("terms", e.get("gain", e.get("result")))
        assert _same(got, want), e


def _same(got, want, rtol=1e-12):
    """The same structure exactly (keys, lengths, status strings), numbers within rtol."""
    if isinstance(want, dict):
        return isinstance(got, dict) and sorted(got) == sorted(want) and all(_same(got[k], want[k], rtol) for k in want)
    if isinstance(want, (list, tuple)):
        return isinstance(got, (list, tuple)) and len(got) == len(want) and all(
            _same(a, b, rtol) for a, b in zip(got, want))
    if isinstance(want, bool) or not isinstance(want, (int, float)):
        return got == want
    return isinstance(got, (int, float)) and not isinstance(got, bool) and abs(got - want) <= rtol * abs(want)


def test_parity_comparison_is_strict():
    """Astra's part-1 review: a 100x gain in a later component, or a different status text, must not pass."""
    want = {"": [[1500.0, 1600.0, 0.01]], "b": [[2000.0, 2100.0, 0.02]]}
    assert _same(want, json.loads(json.dumps(want)))
    assert not _same({"": [[1500.0, 1600.0, 0.01]], "b": [[2000.0, 2100.0, 2.0]]}, want)
    assert not _same({"": [[1500.0, 1600.0, 0.01]]}, want)
    assert not _same({"error": "not assessed: a"}, {"error": "not assessed: b"})
    assert _same({"error": "not assessed: a"}, {"error": "not assessed: a"})


def test_a_correction_without_noise_propagation_makes_band_noise_unknown():
    ops = V02 + [{"op": "savgol", "window": 5, "polyorder": 2, "deriv": 0, "delta": 1.0, "mode": "interp"}]
    o = with_correction(ops, tid="transfer.test.smooth")
    why = "not assessed: no noise propagation through the correction's savgol step"
    for name in ("pair22_hires", "pair10_hires", "synthetic_hireslike_RS79"):
        rec = o.analyse_spectrum(WL, golden(name), ctx(HIRES))
        assert rec["stream"]["transfer"] == TEST_KEY
        assert rec["band_noise"]["noise_unknown"] == {b: why for b in READABLE}
        assert all(rec["band_noise"]["sd_E"][b] is None for b in READABLE)
        note = [n for n in rec["instrument"]["notes"] if n["key"] == "band_noise_unknown"]
        assert note and note[0]["bands"] == READABLE
        ev = rec["evidence"]
        for b, d in ev["bands"].items():
            assert d["sd_u"] is None
            assert (d["state"] == "can't tell") == (d["u"] < 1.5), (name, b, d)
        assert ev["level"] != "none"
        for k in ("cons3", "ryder2045"):
            assert rec["models"][k]["B6"] == {"status": why, "outcome": None}
            assert rec["models"][k]["implied_sd"] is None
        assert rec["models"]["s1_r2"]["B6"]["status"] == "assessed"


def test_a_block_snv_correction_is_non_linear():
    o = with_correction([V02[0], {"op": "snv", "ddof": 0, "blocks_nm": [[1100.0, 1700.0]]}], tid="transfer.test.snv")
    rec = o.analyse_spectrum(WL, golden("pair22_hires"), ctx(HIRES))
    why = "not assessed: non-linear correction"
    for k in ("cons3", "ryder2045"):
        assert rec["models"][k]["B6"]["status"] == why
    assert rec["band_noise"]["noise_unknown"] == {b: why for b in READABLE}


def test_flat_bands_with_unknown_noise_never_read_as_flat():
    P = ORACLE.P
    doc = load("checks/check.evidence_levels.json")
    case = next(c for c in doc["golden"]["cases"] if c["name"] == "rc_flat_none_low_m")
    gs = json.loads((PLUG / "checks/golden_spectra_checks_v1.json").read_text(encoding="utf-8"))
    x = np.asarray(next(s for s in gs["spectra"] if s["sha256"] == case["spectrum_sha256"])["values"], float)
    from oracle import kernels as K
    evp, zp = P.cp("evidence_levels"), P.cp("zooms_patterns")
    E = K.derivative(x, evp["kernel"])
    bands = P.bands()
    known = case["inputs"]["sd_E"]
    assert checks.evidence(WL, E, evp, bands, known)["level"] == "none"
    assert checks.zooms(WL, E, zp, bands, known)["verdict"] == "Unlikely"
    for sd in ({}, {b: float("nan") for b in known}):
        ev = checks.evidence(WL, E, evp, bands, sd)
        assert ev["level"] == "can't tell" and ev["S"] is None
        assert all(d["state"] == "can't tell" for d in ev["bands"].values())
        zo = checks.zooms(WL, E, zp, bands, sd)
        assert zo["verdict"] == "Can't tell" and not any(d["readable"] for d in zo["bands"].values())


def test_a_band_across_a_join_is_not_available_on_this_instrument():
    x = golden("mmc2_Reference_Sample_79")
    rec = ORACLE.analyse_spectrum(WL, x, ctx(STD, (1000.0, 1740.0)))
    unk = rec["band_noise"]["noise_unknown"]
    assert list(unk) == ["CH1728"]
    assert unk["CH1728"].startswith("not assessed: not available on this instrument (band CH1728: 1711-1741 nm crosses "
                                    "the detector join at 1740 nm")
    assert all(rec["band_noise"]["sd_E"][b] is not None for b in READABLE if b != "CH1728")
    rec = ORACLE.analyse_spectrum(WL, x, ctx(STD))
    assert "noise_unknown" not in rec["band_noise"] and rec["instrument"]["notes"] == []


def test_panel_readers_report_an_implied_sd_without_b6():
    for name, cls in (("pair22_std", STD), ("pair22_hires", HIRES)):
        rec = ORACLE.analyse_spectrum(WL, golden(name), ctx(cls))
        for k in ("lowrange", "ch1728", "nb1545"):
            sd = rec["models"][k]["implied_sd"]
            assert sd is not None and sd > 0 and math.isfinite(sd)
            assert rec["models"][k]["B6"] == {"status": "not assessed: B6 not applied to this reader", "outcome": None}
        for k in ("cons3", "ryder2045"):
            assert rec["models"][k]["B6"]["status"] == "assessed"


def test_a_lab_built_linear_correction_gets_computed_gains():
    ops = [{"op": "gaussian_blur", "sigma_nm": [0.0, 2.6, 3.9], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"},
           {"op": "absorbance_affine", "gain": [0.65, 0.74, 0.58], "offset": [0.2, 0.2, 0.25], "clip_min": 1e-6}]
    noise = {"correlation_nm": 1.4, "ch_envelope_factors": {"CH1728": 2.0}}
    x = golden("pair29_hires")
    rec = with_correction(ops, noise, "transfer.test.lab").analyse_spectrum(WL, x, ctx(HIRES))
    plain = with_correction(ops, None, "transfer.test.lab").analyse_spectrum(WL, x, ctx(HIRES))
    assert rec["stream"]["transfer"] == TEST_KEY
    assert "noise_unknown" not in rec["band_noise"]
    for k in ("cons3", "ryder2045"):
        assert rec["models"][k]["B6"]["status"] == "assessed"
    # the expectation comes from the independent research path (golden_gen: scipy SG coefficients, an explicit segment
    # blur matrix, the variance from the explicit noise covariance, a line-by-line N2), not the engine's own routine
    from spyder_ref import golden_gen as GG
    joins = (1000.0, 1800.0)
    want = GG.research_model_sd(load("noise_gains.json"), load("collagen_2045_ryder2026_n140.spyder-model.json"), WL,
                                x, joins, ops, noise)
    sd = rec["models"]["ryder2045"]["implied_sd"]
    assert abs(sd - want) <= 1e-9 * want, (sd, want)
    for b in ("CH1689", "CH1728", "NH2044"):
        want_b = GG.research_band_sd(load("bands.json"), b, WL, x, joins, ops, noise)
        assert abs(rec["band_noise"]["sd_E"][b] - want_b) <= 1e-9 * want_b, (b, rec["band_noise"]["sd_E"][b], want_b)
    r1689 = rec["band_noise"]["sd_E"]["CH1689"] / plain["band_noise"]["sd_E"]["CH1689"]
    r1728 = rec["band_noise"]["sd_E"]["CH1728"] / plain["band_noise"]["sd_E"]["CH1728"]
    assert abs(r1689 - 1) > 1e-3 and abs(r1728 / r1689 - 2.0 / 1.72) < 1e-9


def test_instrument_noise_is_validated():
    t = load("hires_to_std_v0_2.spyder-transfer.json")
    for bad in ({"correlation_nm": -1}, {"ch_envelope_factors": {"CH1728": 0}}, "x"):
        u = copy.deepcopy(t); u["provenance"]["instrument_noise"] = bad
        try:
            SR.check_transfer(u)
        except SR.SpyderError:
            continue
        raise AssertionError(f"accepted {bad}")


def test_a_noise_window_the_grid_only_partly_covers_is_not_available():
    """Astra's part-1 review: on a 1598-1750 nm grid the C-H 1689 kernel fits but its 1500-1600 nm noise window has one
    centre (zero MAD). The whole window, with the neighbours its second difference reads, must be on the grid; else the
    band is "not available on this instrument" and its SD is +infinity (fail safe), never (0.0, assessed)."""
    from spyder_ref import tables as TB
    bd = load("bands.json")
    wl = np.arange(1598, 1751, dtype=float)
    rng = np.random.default_rng(3)
    x = 0.3 * 10 ** -(1e-3 * rng.standard_normal(len(wl)))
    n2c = checks.N2Cache(wl, x)
    sd, why = TB.readability_sd(bd, "CH1689", wl, [], None, {}, n2c)
    assert sd == float("inf")
    assert why == ("not assessed: not available on this instrument (band CH1689: the N2 window 1500-1600 nm is not "
                   "covered by the scan)")
    assert not G.window_covered(wl, 1500, 1600) and G.window_segment(wl, [(0, len(wl))], 1500, 1600) is None
    # exact edges: a centre at the first grid point has no left neighbour; the last centre needs the point at hi
    assert G.window_covered(np.arange(1499, 1601.0), 1500, 1600)
    assert not G.window_covered(np.arange(1500, 1601.0), 1500, 1600)
    assert not G.window_covered(np.arange(1499, 1600.0), 1500, 1600)


def test_a_zero_gain_correction_is_refused_in_both_engines_alike():
    """Astra's part-1 review: an absorbance_affine gain of 0 erases the spectrum (Python raised IndexError where Rust
    skipped the term). Both refuse the file at load; the oracle never selects it."""
    t = load("hires_to_std_v0_2.spyder-transfer.json")
    for g in (0.0, [0.6343, 0.0, 0.6258]):
        u = copy.deepcopy(t)
        u["operator"][1]["gain"] = g
        try:
            SR.check_transfer(u)
        except SR.SpyderError as e:
            assert "gain must be a finite non-zero number" in str(e)
        else:
            raise AssertionError(f"accepted gain {g}")
    zero = [V02[0], dict(V02[1], gain=0.0, offset=0.2)]
    o = with_correction(zero, tid="transfer.test.zero")
    rec = o.analyse_spectrum(WL, golden("pair22_hires"), ctx(HIRES))
    assert rec["stream"]["transfer"] == "transfer.labspec4.hires_to_std@0.3.0"
    # the gain engine itself: a part erased by the correction contributes nothing (as Rust), never an IndexError
    bd = load("bands.json")
    try:
        G.band_gain(bd, "CH1689", WL, (1000.0, 1800.0), zero, {})
    except G.GainError as e:
        assert e.status == "not assessed: not available on this instrument (band CH1689: no weight on the scan)"
    else:
        raise AssertionError("a zero-gain band has a gain")


def test_statistic_gains_match_the_g2_research_anchors():
    """The checks' linear statistics (DECISIONS 137 g2) get their noise gains from the same propagation as the bands
    and models. Independent anchors: g2's research gains (planning g2_gates/g01_gains.py: each statistic's kernel taken
    by finite differences through the oracle's own kernels, the blur applied without segment clipping), for no
    correction and v0.2 at joins 1000/1800. Agreement to 1e-8 (wax 1766 on v0.2 to 1e-6: its blur reaches the 1800 nm
    join, which the engine clips and g2 did not)."""
    fx = json.loads((REPO / "crates/spyder-core/tests/goldens/gains_parity_v1.json").read_text(encoding="utf-8"))
    st = fx["statistics"]
    g01 = {  # statistic, window: (no correction, v0.2)
        ("plaster_1446", (1500, 1600)): (0.0007515802359379831, 0.000289861231067345),
        ("plaster_1945", (2000, 2100)): (0.00046797729678216103, 0.0001885599468186423),
        ("wax_1766", (1500, 1600)): (0.07928105063409477, 0.035158499927923234),
        ("ester_2254", (2220, 2245)): (0.07928105063409477, 0.03120877908495654),
        ("c1", (1500, 1600)): (0.028447283793473527, 0.015567071533499156),
        ("c1", (2000, 2100)): (0.030949583251220675, 0.014537290635137418),
        ("c1_swir1", (1500, 1600)): (0.04906751646104421, 0.02626565881541621),
        ("tail_2350", (2300, 2400)): (0.017728969621838987, 0.00832745248834729),
    }
    v02 = load("hires_to_std_v0_2.spyder-transfer.json")["operator"]
    for (name, win), (g_none, g_v02) in g01.items():
        a = G.stat_term_gain(st[name], WL, (1000.0, 1800.0), None, {}, 1.25, win, name)
        b = G.stat_term_gain(st[name], WL, (1000.0, 1800.0), v02, {}, 1.25, win, name)
        assert abs(a / g_none - 1) < 1e-8, (name, a, g_none)
        assert abs(b / g_v02 - 1) < (1e-6 if name == "wax_1766" else 1e-8), (name, b, g_v02)
    # a statistic with no weight in the window's detector segment, or a window off the grid: not available
    try:
        G.stat_term_gain(st["plaster_1945"], WL, (1000.0, 1800.0), None, {}, 1.25, (1500, 1600), "plaster_1945")
    except G.GainError as e:
        assert e.status.endswith("no weight in the detector segment of the N2 window 1500-1600 nm)")
    else:
        raise AssertionError("a SWIR2 statistic has a SWIR1 term")
    short = np.arange(900, 1701, dtype=float)
    try:
        G.stat_term_gain(st["c1"], short, [], None, {}, 1.25, (2000, 2100), "c1")
    except G.GainError as e:
        assert "not available on this instrument" in e.status and "2000-2100 nm is not covered" in e.status
    else:
        raise AssertionError("a window off the grid has a gain")


def _folder_with(doc):
    d = Path(tempfile.mkdtemp())
    (d / "test.spyder-transfer.json").write_text(json.dumps(doc), encoding="utf-8")
    return Oracle([str(PLUG), str(d)])


def test_versions_are_compared_within_an_id_only():
    """A user class-wide correction under another id, even at 9.0.0, does not replace the bundled v0.3."""
    t = load("hires_to_std_v0_2.spyder-transfer.json")
    t["id"], t["version"] = "transfer.lab.other", "9.0.0"
    rec = _folder_with(t).analyse_spectrum(WL, golden("pair22_hires"), ctx(HIRES))
    assert rec["stream"]["transfer"] == "transfer.labspec4.hires_to_std@0.3.0"


def test_a_standard_unit_file_applies_to_its_own_serial():
    """std -> std with source_serial (as text; the scan's serial is a number): applies to that unit only."""
    t = load("hires_to_std_v0_2.spyder-transfer.json")
    t.update(id="transfer.lab.std_unit", version="1.0.0", source_class=STD, target_class=STD, source_serial="424242",
             operator=[{"op": "absorbance_affine", "gain": [1.0, 1.05, 0.95], "offset": 0.0, "clip_min": 1e-6}])
    o = _folder_with(t)
    x = golden("pair22_std")
    rec = o.analyse_spectrum(WL, x, dict(ctx(STD), serial=424242))
    plain = o.analyse_spectrum(WL, x, dict(ctx(STD), serial=1))
    assert rec["stream"]["transfer"] == "transfer.lab.std_unit@1.0.0"
    assert plain["stream"]["transfer"] == "none"
    assert rec["checks"]["B6b"] == plain["checks"]["B6b"]
    u = copy.deepcopy(t); u.pop("source_serial")
    try:
        SR.check_transfer(u)
    except SR.SpyderError as e:
        assert "must differ" in str(e)
    else:
        raise AssertionError("a class-to-itself file without a serial was accepted")


def test_private_names_list_is_found_from_a_git_worktree():
    """make_goldens' public-text name check reads the private names list from the private planning folder; a git
    worktree has none of its own, so the check follows the worktree's .git file to the main checkout's (REPORT_phase4
    'Open': the check was skipped in the worktree). Temporary folders only; no names."""
    from oracle import split
    t = Path(tempfile.mkdtemp())
    main, wt = t / "main", t / "main" / ".claude" / "worktrees" / "wt"
    gd = main / ".git" / "worktrees" / "wt"
    gd.mkdir(parents=True)
    (gd / "commondir").write_text("../..\n", encoding="utf-8")
    wt.mkdir(parents=True)
    want = str(main.resolve().joinpath(*split.NAMES_REL))
    for gitdir in (str(gd), "../../../.git/worktrees/wt"):                 # absolute, and relative to the worktree
        (wt / ".git").write_text(f"gitdir: {gitdir}\n", encoding="utf-8")
        c = split._names_candidates(wt, {"SPYDER_PRIVATE_NAMES": "x.txt"})
        assert c[0] == "x.txt" and c[1] == str(wt.joinpath(*split.NAMES_REL)) and c[2] == want, c
    assert split._names_candidates(main, {}) == ["", str(main.joinpath(*split.NAMES_REL))]   # a main checkout
