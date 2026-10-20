"""Golden runner for the engine-check parameter files and the analysis profiles (PLAN v3.2 section 4 rules: goldens
run on load and must test something: >= 3 cases, every case a non-empty `expected`, engine-owned tolerance
(<= 1e-9 abs and rel), finite numbers, zero-comparison runs fail; labels compare exactly).

Check-file case:   {"name", "spectrum_sha256" (the standard-resolution-stream spectrum, or the as-measured one for
                   'acquisition' and 'longwave'), "inputs": {...}, "expected": {flat dotted keys: value}}
  inputs: acquisition: {"correction": "none" | a name in the golden block's "corrections" (operators and instrument
          noise, as in the table goldens), optional "splices_nm"} (the per-sign B6b gates follow the noise after it);
          evidence_levels / zooms_patterns: {"sd_E": {band: SD in E units}}
          signs: {"gates": {sign: gate reason or null}, optional "reduced": {sign: fallback variant}};
          c1: {"gate" (reason or null), "specific_fired", "tail_unreliable", "longwave_mode", optional "reduced"};  b9: {"truncated": bool};
          longwave: {"native": bool (the native standard-resolution scan as measured), "correction": as acquisition}
Profile case:      {"name", "inputs": {"m", "components", "evidence_level", "zooms_verdict", "zooms_pattern",
                   "signs_fired": [...], "c1_status", "gated", optional "signs_gated": [...] (per-sign gates),
                   optional "signs_reduced": [...] (signs running a reduced fallback; fired if in signs_fired),
                   "soft_components": [...] (the soft tier fired), optional "zooms_shown_verdict" (the ZooMS verdict
                   after the 1545 nm vote; default zooms_verdict), optional "n_type_lit": [...] (the ZooMS-check bands
                   lit and readable, e.g. AM2175, NH2044, NH1545c; default none), optional "ryder2045" (the
                   published Ryder 2045 reading; default none), optional "models": {key: value} (other profile model
                   readings, e.g. the panel readers ch1728 / nb1545; default none), optional "zooms_check": {"bands": {band: {"lit",
                   "readable"}}, "vote_band": {"id", "lit", "readable"}} (the ZooMS check record as production writes it;
                   overrides n_type_lit)}, "expected": {...}}   (no spectrum)
                   zooms_verdict is the band-pattern verdict (what +D, the lift and the positive-signs line read) for
                   the collagen profiles, and the ZooMS profile's shown verdict when zooms_shown_verdict is absent.

This module is what spyder_ref's validate_file should dispatch to for format 'spyder-bone/engine_check' and
'spyder-bone/analysis_profile' (the hook is left to the spyder_ref owner).
"""
from __future__ import annotations

import math

import numpy as np

from . import checks, kernels as K, verdict as VD
from .export import flatten

MAX_TOL = {"abs": 1e-9, "rel": 1e-9}
MIN_CASES = 3


class GoldenError(ValueError):
    pass


def _cmp(name, key, got, want, tol, res):
    if isinstance(want, bool) or want is None or isinstance(want, str):
        res.append((name, key, got, want, got == want))
        return
    if not isinstance(want, (int, float)) or not math.isfinite(want):
        raise GoldenError(f"case {name}: expected {key} must be a finite number, a label, a bool or null")
    if got is None or isinstance(got, (str, bool)) or not math.isfinite(float(got)):
        res.append((name, key, got, want, False))
        return
    res.append((name, key, got, want, abs(float(got) - want) <= tol["abs"] + tol["rel"] * abs(want)))


def tolerance(doc):
    tol = doc.get("golden", {}).get("tolerance", MAX_TOL)
    for k in ("abs", "rel"):
        v = tol.get(k)
        if not isinstance(v, (int, float)) or isinstance(v, bool) or v < 0 or v > MAX_TOL[k]:
            raise GoldenError(f"golden tolerance {k} must be a number in [0, {MAX_TOL[k]}]")
    return tol


def golden_correction(golden, name):
    """(operators, instrument noise) of a correction a check golden names: 'none' -> (None, None)."""
    if name in (None, "none"):
        return None, None
    c = ((golden or {}).get("corrections") or {}).get(name)
    if c is None:
        raise GoldenError(f"golden correction {name!r} is not in the golden block's corrections")
    return list(c["operator"]), dict(c.get("instrument_noise") or {})


def compute_check(name, P, wl, x, inputs, golden=None):
    """The check's outputs for one spectrum, as a flat dict (what the goldens compare)."""
    p = P.cp(name)
    x = np.asarray(x, float)
    joins = tuple(inputs.get("splices_nm", (1000.0, 1800.0)))
    if name == "acquisition":
        n2c = checks.N2Cache(wl, x)
        r = checks.acquisition(wl, x, p, None, joins)
        ops, inn = golden_correction(golden, inputs.get("correction", "none"))
        # a near-uncorrected correction reads the uncorrected table (the test reads check.longwave and check.signs too)
        near = ops is not None and checks.standard_like(p, P.cp("longwave"), P.cp("signs"), wl, joins, ops, inn)
        r["B6b"] = checks.b6b(p, n2c, wl, joins, ops, inn, near=near)[0]
        return flatten(r)
    if name == "longwave":
        ops, inn = golden_correction(golden, inputs.get("correction", "none"))
        return flatten(checks.longwave(p, bool(inputs["native"]), checks.N2Cache(wl, x), wl, joins, ops, inn))
    if name == "b9":
        tr = bool(inputs.get("truncated", False))
        sc = (p["truncated"] if tr else p)["prototypes"]
        prot = np.load(P.sidecar(sc["sidecar"], sc["sha256"]), allow_pickle=False)
        return flatten(checks.b9(wl, x, p, prot, truncated=tr))
    if name in ("evidence_levels", "zooms_patterns", "c1"):
        E = K.derivative(x, p["kernel"])
        if name == "c1":
            return flatten(checks.c1(wl, E, p, inputs.get("gate"), inputs["specific_fired"], inputs["tail_unreliable"],
                                     inputs.get("longwave_mode", "not_assessed"), inputs.get("reduced")))
        fn = checks.evidence if name == "evidence_levels" else checks.zooms
        return flatten(fn(wl, E, p, P.bands(), inputs["sd_E"]))
    if name == "signs":
        return flatten(checks.signs(wl, x, p, P.cp("heat"), inputs.get("gates", {}), inputs.get("reduced", {})))
    if name == "heat":
        N = K.derivative(x, P.cp("signs")["kernel"])
        return flatten(checks.heat(wl, x, N, p))
    raise GoldenError(f"unknown check {name}")


def compute_profile(doc, inputs):
    pp = doc["parameters"]
    sg = {s: {"fired": s in inputs["signs_fired"], "status": "assessed"} for s in ("plaster", "wax", "ester", "burnt")}
    if inputs.get("gated"):
        sg = {s: {"fired": None, "status": "gated"} for s in sg}
    for s in inputs.get("signs_gated", []):
        if s in sg:
            sg[s] = {"fired": None, "status": "gated"}
    for s in inputs.get("signs_reduced", []):
        if s in sg:
            sg[s] = {"fired": s in inputs["signs_fired"], "status": "assessed: reduced"}
    if inputs.get("soft_components"):
        sg["soft"] = {"fired": True, "components": list(inputs["soft_components"])}
    c1 = {"status": inputs["c1_status"], "fired": "C1" in inputs["signs_fired"]}
    ev = {"level": inputs["evidence_level"]}
    zo = {"verdict": inputs.get("zooms_shown_verdict", inputs["zooms_verdict"]), "pattern": inputs["zooms_pattern"],
          "pattern_verdict": inputs["zooms_verdict"],
          "bands": {b: {"lit": True, "readable": True} for b in inputs.get("n_type_lit", [])}}
    if "zooms_check" in inputs:
        zo["bands"] = dict(inputs["zooms_check"].get("bands", {}))
        if "vote_band" in inputs["zooms_check"]:
            zo["vote_band"] = dict(inputs["zooms_check"]["vote_band"])
    if pp["kind"] == "collagen_rule_L2":
        models = {"ryder2045": {"value": inputs["ryder2045"]}} if "ryder2045" in inputs else {}
        models.update({k: {"value": v} for k, v in inputs.get("models", {}).items()})   # panel readers (notes.panel)
        r = VD.collagen_verdict(pp, inputs["m"], inputs.get("components", {}), ev, zo, sg, c1, inputs.get("gated", False),
                                models)
    else:
        r = VD.zooms_verdict(pp, zo, sg, c1, inputs.get("m"))
    return {"verdict": r["verdict"], "rule_step": r["rule_step"], "model_verdict": r.get("model_verdict"),
            "notes_shown": ",".join(r["notes_shown"]), "notes_all": ",".join(n["key"] for n in r["notes_all"]),
            "flags": ",".join(f["key"] for f in r["flags"])}


def run(doc, P, spectra):
    """Run every golden case of a check or profile file. spectra: {sha256: (wl, x)}. Returns [(case, key, got,
    want, ok)]; raises GoldenError when the goldens cannot test anything (the file is then rejected)."""
    tol = tolerance(doc)
    cases = doc.get("golden", {}).get("cases")
    if not isinstance(cases, list) or len(cases) < MIN_CASES:
        raise GoldenError(f"at least {MIN_CASES} golden cases required")
    res = []
    for c in cases:
        exp = c.get("expected")
        if not isinstance(exp, dict) or not exp:
            raise GoldenError(f"case {c.get('name')}: empty expected block")
        if doc["format"] == "spyder-bone/analysis_profile":
            got = compute_profile(doc, c["inputs"])
        else:
            if c.get("spectrum_sha256") not in spectra:
                raise GoldenError(f"case {c.get('name')}: spectrum not in the golden spectra file")
            wl, x = spectra[c["spectrum_sha256"]]
            got = compute_check(doc["check"], P, wl, x, c.get("inputs", {}), doc.get("golden"))
        n0 = len(res)
        for k, w in exp.items():
            if k not in got:
                res.append((c["name"], k + " (not produced)", None, w, False))
            else:
                _cmp(c["name"], k, got[k], w, tol, res)
        if len(res) == n0:
            raise GoldenError(f"case {c.get('name')}: zero comparisons")
    return res
