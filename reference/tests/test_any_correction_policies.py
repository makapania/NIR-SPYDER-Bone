"""DECISIONS 137 Phase 2 (part 2) on the frozen reference and oracle: the noise policies follow the correction applied,
never a transfer file's SHA or a class name. Per-sign B6b gates, the DECISIONS 107 fallbacks, long-wave and soft-tier
eligibility through a FieldSpec 3-like class file and a lab's own high-res unit file; the uncalibrated note for another
instrument's scan with no correction; heat on the scan as measured when a correction did not fit its visible levels;
per-reader offsets; a correction's own second-opinion gap. The same cases run against the Rust pipeline in
crates/spyder-core/tests/any_correction_policies.rs. Public spectra only.

Independent expectations: the corrections here have no blur, so a statistic term's noise gain through them is the
detector's absorbance gain x its gain as measured; the as-measured gains are g2's research values (planning
g2_gates/g01_gains.py: kernels by finite differences through the oracle kernels), not the engine's own routine. The
cuts (c_s) are read from the shipped check files."""
from __future__ import annotations

import copy
import json
import math
import shutil
import sys
import tempfile
from pathlib import Path

import numpy as np

REF = Path(__file__).resolve().parents[1]
REPO = REF.parent
PLUG = REPO / "plugins"
sys.path.insert(0, str(REF))
import spyder_ref as SR                      # noqa: E402
from oracle import Oracle                    # noqa: E402
from oracle.export import csv_row            # noqa: E402

WL = np.arange(350, 2501).astype(float)
STD, HIRES, FS3 = "asd.labspec4.std", "asd.labspec4.hires", "asd.fieldspec3"
load = lambda name: SR.load_json(PLUG / name)
# g2's research gains as measured (no correction), joins 1000/1800: statistic -> SD per unit N2 of its term
G_NONE = {"plaster_1945": 0.00046797729678216103, "wax_1766": 0.07928105063409477, "ester_2254": 0.07928105063409477,
          "c1_swir2": 0.030949583251220675, "plaster_1446": 0.0007515802359379831, "c1_swir1": 0.04906751646104421,
          "tail_2350": 0.017728969621838987}


def golden(name):
    g = json.loads((PLUG / "golden_spectra_public_v2.json").read_text(encoding="utf-8"))
    return np.asarray(next(s for s in g["spectra"] if s["name"] == name)["values"], float)


def noisy(x, sd, seed, ratio=0.15):
    """x with white noise added in absorbance: SD `sd` above 1800 nm, `ratio` x `sd` below (a dark long-wave detector, as
    on noisy high-res scans); a deterministic LCG, the same in the Rust test."""
    s = seed & 0xFFFFFFFF
    out = np.empty_like(x)
    for i, v in enumerate(x):
        s = (1664525 * s + 1013904223) & 0xFFFFFFFF
        u = s / 4294967296.0 - 0.5
        a = sd if WL[i] > 1800.0 else ratio * sd
        out[i] = v * 10.0 ** (-a * 3.4641016151377544 * u)
    return out


def with_file(doc, name="test.spyder-transfer.json"):
    d = Path(tempfile.mkdtemp())
    (d / name).write_text(json.dumps(doc), encoding="utf-8")
    return Oracle([str(PLUG), str(d)])


def correction(tid, source, ops, serial=None, **extra):
    t = load("hires_to_std_v0_2.spyder-transfer.json")
    t.update(id=tid, version="1.0.0", source_class=source, target_class=STD, operator=ops)
    if serial is not None:
        t["source_serial"] = serial
    t.update(extra)
    return t


def ctx(cls, serial=None):
    return {"instrument_class": cls, "class_source": "user", "splices_nm": [1000.0, 1800.0], "serial": serial}


def cuts():
    a = load("checks/check.acquisition.json")["parameters"]["B6b_signs_gate"]
    lw = load("checks/check.longwave.json")["parameters"]
    soft = load("checks/check.signs.json")["parameters"]["soft_tier"]["eligibility"]
    return a, lw, soft


def expected(gains, n2):
    """The gate outcomes the SD rule implies for a blur-free correction with per-detector absorbance gains (VNIR,
    SWIR1, SWIR2), from N2 as measured: the research gains as measured x the detector's gain."""
    a, lw, soft = cuts()
    g1, g2 = abs(gains[1]), abs(gains[2])
    sd = {"plaster": g2 * G_NONE["plaster_1945"] * n2["2000-2100"], "wax": g1 * G_NONE["wax_1766"] * n2["1500-1600"],
          "ester": g2 * G_NONE["ester_2254"] * n2["2220-2245"], "C1": g2 * G_NONE["c1_swir2"] * n2["2000-2100"]}
    gated = {s: sd[s] > a["corrected"][s]["sd_above"] for s in sd}
    red = {"plaster": gated["plaster"] and g1 * G_NONE["plaster_1446"] * n2["1500-1600"] <= a["fallbacks"]["plaster"]["sd_at_most"],
           "C1": gated["C1"] and g1 * G_NONE["c1_swir1"] * n2["1500-1600"] <= a["fallbacks"]["C1"]["sd_at_most"]}
    tail = g2 * G_NONE["tail_2350"] * n2["2300-2400"]
    return {"gated": gated, "reduced": red, "tail_b9": tail > lw["b9_sd_above"], "tail_c1": tail > lw["c1_sd_above"],
            "soft": g2 * G_NONE["c1_swir2"] * n2["2000-2100"] <= soft["otherwise_sd_at_most"], "sd": sd}


def check_record(rec, gains, tag):
    n2 = rec["n2"]
    e = expected(gains, n2)
    b = rec["checks"]["B6b"]
    for s, g in e["gated"].items():
        st = (rec["C1"] if s == "C1" else rec["signs"][s])["status"]
        if g and e["reduced"].get(s):
            assert s in b["signs_reduced"] and st.startswith(("assessed: reduced", "skipped")), (tag, s, st)
        elif g:
            assert s in b["signs_gated"] and st.startswith("gated: noise in"), (tag, s, st)
        else:
            assert s not in b["signs_gated"] and s not in b["signs_reduced"], (tag, s, b)
    assert not rec["signs"]["burnt"]["status"].startswith("gated"), tag          # burnt never gated
    lw = rec["checks"]["longwave"]
    assert lw["status"] == "assessed"
    assert lw["tail_unreliable"] == e["tail_b9"] and lw["tail_unreliable_c1"] == e["tail_c1"], (tag, lw, e)
    assert rec["signs"]["soft"]["eligible"] == e["soft"], (tag, rec["signs"]["soft"], e)
    return e


NOISE = [0.0, 1e-4, 2e-4, 3e-4, 4.5e-4, 6e-4, 1e-3, 1.6e-3, 2.5e-3, 4e-3]


def test_a_fieldspec3_like_class_file_gets_the_sd_rule_gates():
    gains = [1.0, 1.25, 1.8]
    t = correction("transfer.test.fs3like", FS3, [{"op": "absorbance_affine", "gain": gains, "offset": 0.0,
                                                   "clip_min": 1e-6}])
    o = with_file(t)
    seen = {"gated": 0, "reduced": 0, "tail": 0, "soft_off": 0, "clean": 0}
    x0 = golden("mmc2_Reference_Sample_79")
    for k, sd in enumerate(NOISE):
        rec = o.analyse_spectrum(WL, noisy(x0, sd, 7 + k), ctx(FS3))
        assert rec["stream"]["transfer"] == "transfer.test.fs3like@1.0.0"
        assert not any(n["key"] == "instrument_uncalibrated" for n in rec["instrument"]["notes"])
        e = check_record(rec, gains, f"fs3like sd {sd}")
        seen["gated"] += sum(e["gated"].values())
        seen["reduced"] += sum(e["reduced"].values())
        seen["tail"] += e["tail_b9"]
        seen["soft_off"] += not e["soft"]
        seen["clean"] += not any(e["gated"].values())
        # S1_R2 reads the scan as measured for any class, never through the FieldSpec 3 file
        assert rec["models"]["s1_r2"]["transfer"] == "none"
    assert all(v > 0 for v in seen.values()), seen


def test_a_lab_unit_file_gets_the_sd_rule_gates_for_its_serial_only():
    gains = [0.7, 0.6, 0.5]
    t = correction("transfer.lab.unit42", HIRES, [{"op": "absorbance_affine", "gain": gains, "offset": 0.0,
                                                   "clip_min": 1e-6}], serial="4242")
    o = with_file(t)
    x0 = golden("mmc2_Reference_Sample_79")
    seen = {"gated": 0, "reduced": 0, "clean": 0}
    for k, sd in enumerate(NOISE):
        x = noisy(x0, sd, 11 + k, 0.08)
        rec = o.analyse_spectrum(WL, x, ctx(HIRES, 4242))
        assert rec["stream"]["transfer"] == "transfer.lab.unit42@1.0.0"
        e = check_record(rec, gains, f"unit42 sd {sd}")
        seen["gated"] += sum(e["gated"].values())
        seen["reduced"] += sum(e["reduced"].values())
        seen["clean"] += not any(e["gated"].values())
        other = o.analyse_spectrum(WL, x, ctx(HIRES, 4243))
        assert other["stream"]["transfer"] == "transfer.labspec4.hires_to_std@0.3.0"
    assert all(v > 0 for v in seen.values()), seen


def _v02_only():
    """The shipped plug-ins without v0.3: v0.2 (superseded, kept as history) is the high-res correction (the oracle has
    no pins; the Rust test pins 0.2.0)."""
    d = Path(tempfile.mkdtemp()) / "plugins"
    shutil.copytree(PLUG, d, ignore=shutil.ignore_patterns("hires_to_std_v0_3.spyder-transfer.json"))
    o = Oracle([str(d)])
    assert o.P.stream_transfer(HIRES, None).ref == "transfer.labspec4.hires_to_std@0.2.0"
    return o


def test_v02_reproduces_its_shipped_cuts():
    """Through v0.2 (pinned) the SD rule is its shipped N2 table exactly: plaster 150 / wax 60 / ester 250 / C1 125,
    fallbacks at N2(1500-1600) <= 30, long-wave 150 / 250, soft tier N2(2000-2100) <= 60."""
    o = _v02_only()
    x0 = golden("pair22_hires")
    for k, sd in enumerate(NOISE):
        rec = o.analyse_spectrum(WL, noisy(x0, sd, 3 + k), ctx(HIRES))
        n2, b = rec["n2"], rec["checks"]["B6b"]
        g = lambda s: s in b["signs_gated"] or s in b["signs_reduced"]
        assert g("plaster") == (n2["2000-2100"] > 150) and g("C1") == (n2["2000-2100"] > 125)
        assert g("wax") == (n2["1500-1600"] > 60) and g("ester") == (n2["2220-2245"] > 250)
        for s in b["signs_reduced"]:
            assert n2["1500-1600"] <= 30
        lw = rec["checks"]["longwave"]
        assert lw["tail_unreliable"] == (n2["2300-2400"] > 150) and lw["tail_unreliable_c1"] == (n2["2300-2400"] > 250)
        assert rec["signs"]["soft"]["eligible"] == (n2["2000-2100"] <= 60)


NEAR = "not assessed: near-uncorrected correction (read as a native standard-resolution scan)"


def _second_opinion_fired(rec):
    return any(n.get("key") == "second_opinion_differs" for a in ("radiocarbon", "isotopes", "zooms")
               for n in rec[a]["notes_all"])


def test_a_near_uncorrected_correction_reads_as_a_native_standard_scan():
    """Standard-res never changes (Matt, 2026-10-19): a correction whose every noise-policy gain (the B6b gate and
    fallback statistics, the long-wave tail reading, the soft-tier statistic) is within check.acquisition's
    std_like_within (15%) of uncorrected, here a standard-resolution unit's own file, is read exactly like a native
    standard-resolution scan by every noise policy: the uncorrected B6b table (no fallbacks), long-wave not assessed, the
    soft tier always eligible, no S1_R2 and no second opinion. The noise levels reach the SD rules' cuts."""
    gains = [1.0, 1.05, 0.95]
    t = correction("transfer.lab.std_unit", STD, [{"op": "absorbance_affine", "gain": gains, "offset": 0.0,
                                                   "clip_min": 1e-6}], serial="424242")
    o = with_file(t)
    a, lw, soft = cuts()
    x0 = golden("pair22_std")
    sd_rule = [0, 0, 0]
    for k, sd in enumerate(NOISE):
        x = noisy(x0, sd, 21 + k)
        rec = o.analyse_spectrum(WL, x, ctx(STD, 424242))
        plain = o.analyse_spectrum(WL, x, ctx(STD, 1))
        assert rec["stream"]["transfer"] == "transfer.lab.std_unit@1.0.0" and plain["stream"]["transfer"] == "none"
        assert rec["checks"]["B6b"] == plain["checks"]["B6b"], sd
        for s in ("plaster", "wax", "ester", "burnt"):
            assert rec["signs"][s]["gated"] == plain["signs"][s]["gated"], (sd, s)
        assert rec["C1"]["gated"] == plain["C1"]["gated"]
        assert rec["checks"]["longwave"]["status"] == NEAR
        for r in (rec, plain):
            assert r["checks"]["longwave"]["tail_unreliable"] is False
            assert r["checks"]["longwave"]["tail_unreliable_c1"] is False
        assert rec["signs"]["soft"]["eligible"] is True and rec["signs"]["soft"]["status"] == "assessed"
        assert "s1_r2" not in rec["models"] and not _second_opinion_fired(rec)
        n2 = rec["n2"]
        tail = gains[2] * G_NONE["tail_2350"] * n2["2300-2400"]
        sd_rule[0] += tail > lw["b9_sd_above"]
        sd_rule[1] += tail > lw["c1_sd_above"]
        sd_rule[2] += gains[2] * G_NONE["c1_swir2"] * n2["2000-2100"] > soft["otherwise_sd_at_most"]
    assert all(v > 0 for v in sd_rule), sd_rule


def test_an_identity_unit_file_gives_the_native_record():
    """An identity correction (gain 1, no blur) gives the native record itself: only the stream, the transfer the
    models name and the long-wave status's reason differ (numbers to 1e-9: the identity runs through absorbance)."""
    from oracle.export import flatten
    t = correction("transfer.lab.std_unit", STD, [{"op": "absorbance_affine", "gain": 1.0, "offset": 0.0,
                                                   "clip_min": 1e-6}], serial="424242")
    o = with_file(t)
    x0 = golden("pair22_std")
    skip = lambda k: (k.startswith(("stream.", "dependencies.", "instrument.serial")) or k == "band_noise.transfer_key"
                      or k == "checks.longwave.status" or (k.startswith("models.") and k.endswith(".transfer")))
    for k, sd in enumerate(NOISE):
        x = noisy(x0, sd, 41 + k)
        rec = o.analyse_spectrum(WL, x, ctx(STD, 424242))
        plain = o.analyse_spectrum(WL, x, ctx(STD, 424243))
        a = {k: v for k, v in flatten(rec).items() if not skip(k)}
        b = {k: v for k, v in flatten(plain).items() if not skip(k)}
        assert list(a) == list(b), sd
        for key, v in b.items():
            w = a[key]
            if isinstance(v, float) and not isinstance(v, bool):
                assert isinstance(w, float) and abs(w - v) <= 1e-9 * (1 + abs(v)), (sd, key, w, v)
            else:
                assert w == v, (sd, key, w, v)
        assert rec["checks"]["longwave"]["status"] == NEAR


def test_a_unit_file_beyond_the_near_band_gets_the_sd_rules():
    """A unit file beyond 15% on any noise-policy statistic is a correction like any other: the SD rules."""
    gains = [1.0, 1.25, 1.25]
    t = correction("transfer.lab.std_unit", STD, [{"op": "absorbance_affine", "gain": gains, "offset": 0.0,
                                                   "clip_min": 1e-6}], serial="424242")
    o = with_file(t)
    x0 = golden("pair22_std")
    seen = [0, 0]
    for k, sd in enumerate(NOISE):
        rec = o.analyse_spectrum(WL, noisy(x0, sd, 61 + k), ctx(STD, 424242))
        assert rec["stream"]["transfer"] == "transfer.lab.std_unit@1.0.0"
        e = check_record(rec, gains, f"std unit far sd {sd}")
        seen[0] += sum(e["gated"].values())
        seen[1] += e["tail_b9"]
        assert "s1_r2" in rec["models"]
    assert all(v > 0 for v in seen), seen


def test_another_instruments_scan_with_no_correction_is_uncalibrated():
    o = Oracle([str(PLUG)])
    x = golden("mmc2_Reference_Sample_79")
    rec = o.analyse_spectrum(WL, x, ctx(FS3))
    std = o.analyse_spectrum(WL, x, ctx(STD))
    assert {"key": "instrument_uncalibrated", "class": FS3} in rec["instrument"]["notes"]
    assert rec["stream"]["transfer"] == "none"
    assert rec["checks"]["B6b"] == std["checks"]["B6b"]                 # uncorrected: today's standard-res table
    assert rec["checks"]["longwave"]["status"] == "assessed"            # not the native standard-resolution scan
    assert std["checks"]["longwave"]["status"] == "not assessed: native standard-resolution scan"
    assert "s1_r2" in rec["models"] and "s1_r2" not in std["models"]
    assert rec["models"]["cons3"]["value"] == std["models"]["cons3"]["value"]


def test_heat_reads_the_scan_as_measured_when_visible_levels_are_not_fitted():
    shipped = load("hires_to_std_v0_3.spyder-transfer.json")      # the bundled correction
    t = copy.deepcopy(shipped); t["version"] = "1.0.0"; t["provenance"]["visible_levels"] = "not_fitted"
    x = golden("pair22_hires")
    rec = with_file(t).analyse_spectrum(WL, x, ctx(HIRES))
    fitted = Oracle([str(PLUG)]).analyse_spectrum(WL, x, ctx(HIRES))
    assert rec["stream"]["transfer"] == "transfer.labspec4.hires_to_std@1.0.0"
    assert {"key": "visible_levels_not_fitted"} in rec["instrument"]["notes"]
    from oracle import checks, kernels as K
    P = Oracle([str(PLUG)]).P
    keys = ("edge50_nm", "R_vis", "OH1433", "OH979", "charred", "calcined")
    heat = lambda r: {k: r["signs"]["burnt"][k] for k in keys}
    h = checks.heat(WL, x, K.derivative(x, P.cp("signs")["kernel"]), P.cp("heat"))
    assert heat(rec) == {k: h[k] for k in keys}                       # the heat check on the scan as measured
    assert heat(fitted) != heat(rec)
    assert not any(n["key"] == "visible_levels_not_fitted" for n in fitted["instrument"]["notes"])
    # only "fitted" or "not_fitted" (absent = fitted); null is refused too, with the Rust engine's message, so the two
    # engines never read heat on different streams (Astra's whole-branch review: null used to mean not fitted here and
    # fitted in Rust); a refused file is never selected
    for v in ("partly", None, 1):
        bad = copy.deepcopy(t); bad["provenance"]["visible_levels"] = v
        try:
            SR.check_transfer(bad)
        except SR.SpyderError as e:
            assert str(e) == 'provenance.visible_levels must be "fitted" or "not_fitted"', (v, str(e))
        else:
            raise AssertionError(f"accepted visible_levels {v!r}")
        r2 = with_file(bad).analyse_spectrum(WL, x, ctx(HIRES))
        assert r2["stream"]["transfer"] == "transfer.labspec4.hires_to_std@0.3.0", v
        assert r2["signs"]["burnt"] == fitted["signs"]["burnt"], v
    absent = copy.deepcopy(t); del absent["provenance"]["visible_levels"]
    assert SR.transfer_settings(SR.check_transfer(absent))[0] is True


def test_heat_outside_its_range_is_not_available():
    from oracle import checks
    P = Oracle([str(PLUG)]).P
    wl = np.arange(900, 2501).astype(float)
    x = golden("mmc2_Reference_Sample_79")[550:]
    from oracle import kernels as K
    h = checks.heat(wl, x, K.derivative(x, P.cp("signs")["kernel"]), P.cp("heat"))
    assert h["status"] == "not assessed: not available on this instrument (the heat sign reads 450-1446 nm)"
    assert h["fired"] is None


def test_reader_offsets_are_added_to_their_readers_and_recorded():
    shipped = load("hires_to_std_v0_3.spyder-transfer.json")      # the bundled correction
    t = copy.deepcopy(shipped); t["version"] = "1.0.0"
    t["reader_offsets"] = {"collagen.ryder2026.2045": 0.5, "collagen.spyder.1500_oh_corrected": -0.3}
    x = golden("pair22_hires")
    rec = with_file(t).analyse_spectrum(WL, x, ctx(HIRES))
    plain = Oracle([str(PLUG)]).analyse_spectrum(WL, x, ctx(HIRES))
    r, p = rec["models"], plain["models"]
    assert abs(r["ryder2045"]["value"] - (p["ryder2045"]["value"] + 0.5)) < 1e-12
    assert r["ryder2045"]["reader_offset"] == 0.5 and "reader_offset" not in p["ryder2045"]
    c = dict(p["cons3"]["components"]); c["wc1500"] -= 0.3
    assert abs(r["cons3"]["components"]["wc1500"] - c["wc1500"]) < 1e-12
    assert abs(r["cons3"]["value"] - float(np.median(list(c.values())))) < 1e-12
    assert r["cons3"]["component_offsets"] == {"wc1500": -0.3}
    row = csv_row(rec, "radiocarbon")
    assert row["ryder2045.reader_offset"] == 0.5 and row["cons3.wc1500.reader_offset"] == -0.3
    assert "ryder2045.reader_offset" not in csv_row(plain, "radiocarbon")
    for bad in ({"collagen.ryder2026.2045": 6.0}, {"collagen.ryder2026.2045": float("nan")}):
        u = copy.deepcopy(t); u["reader_offsets"] = bad
        try:
            SR.check_transfer(u)
        except SR.SpyderError:
            continue
        raise AssertionError(f"accepted {bad}")
    # a consensus (or an unknown model) cannot carry an offset: the file is never selected
    for bad in ({"collagen.spyder.consensus3_median": 0.2}, {"collagen.unknown": 0.2}):
        u = copy.deepcopy(t); u["reader_offsets"] = bad
        assert with_file(u).analyse_spectrum(WL, x, ctx(HIRES))["stream"]["transfer"] == \
            "transfer.labspec4.hires_to_std@0.3.0"


def test_a_corrections_own_second_opinion_gap():
    shipped = load("hires_to_std_v0_3.spyder-transfer.json")      # the bundled correction
    x = golden("pair22_hires")
    plain = Oracle([str(PLUG)]).analyse_spectrum(WL, x, ctx(HIRES))
    gap = abs(plain["models"]["s1_r2"]["value"] - plain["models"]["cons3"]["value"])
    assert gap < 4.1 and not any(n["key"] == "second_opinion_differs" for n in plain["radiocarbon"]["notes_all"])
    t = copy.deepcopy(shipped); t["version"] = "1.0.0"
    t["provenance"]["second_opinion_gap"] = {"points": gap / 2, "n_scans": 25}
    rec = with_file(t).analyse_spectrum(WL, x, ctx(HIRES))
    assert any(n["key"] == "second_opinion_differs" for n in rec["radiocarbon"]["notes_all"])
    t["provenance"]["second_opinion_gap"]["n_scans"] = 12
    try:
        SR.check_transfer(t)
    except SR.SpyderError as e:
        assert "n_scans" in str(e)
    else:
        raise AssertionError("accepted a gap from 12 scans")


BAND_NOTES = {"flat_bands_lead", "plus_d", "lift_good", "lift_borderline", "lift_blocked_contaminant",
              "lift_blocked_noisy", "zooms_better_good", "zooms_better_protein", "zooms_better_1545",
              "zooms_better_1545_flat", "zooms_faint_protein", "positive_signs_all_six", "positive_signs_clear",
              "bands_too_noisy"}
NOT_BUILT_IN = "not assessed: band evidence needs this instrument built in"
# the public cases whose soft-tier sign is just over its line (Astra's whole-branch review reproduced the unqualified
# soft note on them as FieldSpec 3)
SOFT_CASES = ("soft_c1_just_over", "soft_ester_just_over", "soft_plaster_just_over")


def test_an_instrument_not_built_in_reads_its_readings_only():
    """DECISIONS 137 (amended 2026-10-19): a class with no built-in correction (FieldSpec 3 here) has the band calls
    off: the model verdict stands (no flat bands lead, +D, lift or blocked lift), no band ZooMS line, positive-signs line
    or bands-too-noisy note; evidence and the ZooMS band pattern say 'not assessed: band evidence needs this instrument
    built in' while the band readings stay (the close-ups); the ZooMS profile says Can't tell; flags still show, marked
    provisional, with the same sort penalty, and so is the soft contaminant note (possible_thin_coating); S1_R2 is shown
    but its second-opinion note is not added. B9 is the one deliberate exception (an input screen, the D2 profile's
    shape against bone prototypes, not band evidence; DECISIONS 118, 137): when it passes the readings set the verdict,
    when it fails its own 'Doesn't look like bone' stands (public not_bone_sine). Every public end-to-end golden
    spectrum, against the same spectrum on the standard-resolution class (unchanged)."""
    o = Oracle([str(PLUG)])
    sp = json.loads((REF / "oracle" / "goldens" / "oracle_spectra_public_v1.json").read_text(encoding="utf-8"))
    seen = {"std_band_calls": 0, "flags": 0, "would_fire": 0, "soft": 0}
    soft_cases = set()
    b9 = {"pass_or_not_assessed": 0, "fail": 0}
    b9_failed = set()
    gap = load("profiles/profile.radiocarbon.json")["parameters"]["second_opinion"]["gap_points"]
    for s in sp["spectra"]:
        x = np.asarray(s["values"], float)
        u, std = o.analyse_spectrum(WL, x, ctx(FS3)), o.analyse_spectrum(WL, x, ctx(STD))
        if "evidence" not in u:                                       # a Rescan
            continue
        assert {"key": "instrument_uncalibrated", "class": FS3} in u["instrument"]["notes"]
        assert u["evidence"]["status"] == NOT_BUILT_IN and u["evidence"]["level"] is None
        assert u["evidence"]["bands"] == std["evidence"]["bands"]     # the readings stay for the close-ups
        zo = u["zooms_pattern"]
        assert zo["status"] == NOT_BUILT_IN and all(zo[k] is None for k in ("pattern", "verdict", "pattern_verdict",
                                                                              "vote_1545"))
        assert zo["bands"] == std["zooms_pattern"]["bands"]
        assert std["evidence"]["status"] == "assessed" and std["zooms_pattern"]["status"] == "assessed"
        notfit = u["checks"]["B9"].get("outcome") == "fail"
        for a in ("radiocarbon", "isotopes", "zooms"):
            r, q = u[a], std[a]
            assert all(f.get("provisional") is True for f in r["flags"]), (s["name"], a)
            assert [(f["key"], f["signs"]) for f in r["flags"]] == [(f["key"], f["signs"]) for f in q["flags"]]
            assert not any("provisional" in f for f in q["flags"])
            assert r.get("sort_penalty") == q.get("sort_penalty")      # the flags keep their effect on sorting
            # the soft contaminant note is provisional too, as the flags are (never on the standard class)
            soft = [n for n in r["notes_all"] if n["key"] == "possible_thin_coating"]
            assert all(n.get("provisional") is True for n in soft), (s["name"], a)
            assert not any("provisional" in n for n in q["notes_all"] if n["key"] == "possible_thin_coating")
            if a == "radiocarbon" and soft:
                seen["soft"] += 1
                soft_cases.add(s["name"])
            keys = {n["key"] for n in r["notes_all"]}
            assert not keys & BAND_NOTES and "second_opinion_differs" not in keys, (s["name"], a, keys)
            if notfit:
                # B9 is a deliberate exception: its own verdict replaces the readings' (flags still provisional)
                assert (r["verdict"], r["rule_step"]) == ("Doesn't look like bone", "B9"), (s["name"], a)
                continue
            # B9 passed (or was not assessed): the readings set the verdict
            if a == "zooms":
                assert (r["verdict"], r["rule_step"], r["pattern"]) == ("Can't tell", "band_evidence_not_built_in", None)
            elif r["model_verdict"] is not None:
                assert (r["verdict"], r["rule_step"]) == (r["model_verdict"], "model_verdict"), (s["name"], a)
        b9["fail" if notfit else "pass_or_not_assessed"] += 1
        if notfit:
            b9_failed.add(s["name"])
        seen["std_band_calls"] += std["radiocarbon"]["rule_step"] in ("flat_bands_lead", "plus_d", "lift_good",
                                                                       "lift_borderline", "lift_blocked")
        seen["flags"] += bool(u["radiocarbon"]["flags"])
        s1, m = (u["models"].get("s1_r2") or {}).get("value"), u["models"]["cons3"]["value"]
        assert "s1_r2" in u["models"]
        seen["would_fire"] += s1 is not None and m is not None and abs(s1 - m) > gap
    assert all(v > 0 for v in seen.values()), seen
    assert set(SOFT_CASES) <= soft_cases, soft_cases
    # both B9 branches ran; not_bone_sine is caught on the instrument not built in
    assert all(v > 0 for v in b9.values()), b9
    assert "not_bone_sine" in b9_failed, b9_failed


S1_R2 = "collagen.spyder.1500_snv_transfer_free"


def test_predictions_read_s1_r2_as_measured_like_the_pipeline():
    """The predictions share the pipeline's per-reader stream policy (Astra's whole-branch review): S1_R2 reads the scan
    as measured even when a unit file for the scan's serial, or another instrument's correction, matches; every other
    reader goes through that file. Public high_res_23 through a unit file holding the shipped v0.3 operators: S1_R2
    reads 10.97832074 as measured (10.51328874 had it gone through the file). The same case runs on the Rust engine
    (crates/spyder-core/tests/any_correction_policies.rs)."""
    sys.path.insert(0, str(REPO / "crates" / "spyder-core" / "tests" / "gen"))
    from parity_common import Engine
    from oracle import asd as ASD
    h, wl, dn, ref = ASD.read(str(REPO / "testdata" / "scans" / "high_res" / "high_res_23.asd"))
    R = dn / ref
    joins, serial = h["splices_nm"], h["serial"]
    ops = load("hires_to_std_v0_3.spyder-transfer.json")["operator"]
    for tag, cls, t in (("unit", HIRES, correction("transfer.lab.unit_hr23", HIRES, ops, serial=str(serial))),
                        ("fs3", FS3, correction("transfer.test.fs3", FS3, ops))):
        d = Path(tempfile.mkdtemp()) / "plugins"
        shutil.copytree(PLUG, d)
        (d / "test.spyder-transfer.json").write_text(json.dumps(t), encoding="utf-8")
        eng = Engine(d)
        got = eng.predict_all(wl, R, joins, cls, str(serial))
        sctx = SR.ScanContext(tuple(joins), cls, str(serial))
        md = next(m for m in eng.models if m["id"] == S1_R2)
        raw = float(SR.predict(md, wl, R[None, :], sctx)["value"][0])
        assert got[S1_R2]["transfer"] is None and got[S1_R2]["value"] == raw, (tag, got[S1_R2])
        tsha = next(x["_sha256"] for x in eng.transfers if x["id"] == t["id"])
        assert all(v["transfer"] == tsha for k, v in got.items() if k != S1_R2), tag
        tw, tx = SR.apply_transfer(next(x for x in eng.transfers if x["id"] == t["id"]), wl, R[None, :], sctx)
        corrected = float(SR.predict(md, tw, tx, sctx)["value"][0])
        assert abs(raw - corrected) > 0.1, (tag, raw, corrected)
        if tag == "unit":
            assert abs(raw - 10.97832074) < 1e-8 and abs(corrected - 10.51328874) < 1e-8, (raw, corrected)
        # the pipeline record agrees
        rec = Oracle([str(d)]).analyse_spectrum(wl, R, {"instrument_class": cls, "class_source": "user",
                                                        "splices_nm": joins, "serial": serial})
        assert rec["stream"]["transfer"] == f"{t['id']}@1.0.0"
        assert rec["models"]["s1_r2"]["transfer"] == "none" and rec["models"]["s1_r2"]["value"] == raw, tag
        assert rec["models"]["cons3"]["value"] == got["collagen.spyder.consensus3_median"]["value"], tag
