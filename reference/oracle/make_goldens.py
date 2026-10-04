"""Regenerate every PUBLIC golden from public spectra only (Phase 0b items 3, 6, 7, 14):

  1. end-to-end oracle goldens   reference/oracle/goldens/oracle_public_v1.json  (+ oracle_spectra_public_v1.json)
  2. check-file goldens          plugins/checks/check.*.json                     (+ plugins/checks/golden_spectra_checks_v1.json)
  3. profile goldens             plugins/profiles/profile.*.json

Cases are recipes (reference/oracle/goldens/cases_public_v1.json): a public base spectrum (JAS mmc2, Archaeometry SI,
numbered standard/high-res pairs) or a generator, optionally moved by synth.synthesise to hit a rule branch, plus
ASSERTS: the branch each case exists to test. Regeneration re-synthesises against the CURRENT plug-in files and fails
if any case no longer hits its branch (so a model or parameter change can never silently retire a branch golden).
A golden-only change bumps the file's PATCH version.

    python -m oracle.make_goldens --roots plugins [<staging folder> ...] [--extra-spectra <golden spectra json> ...]
    python -m oracle.make_goldens --roots ... --check        (re-run the stored goldens; no writing)
"""
from __future__ import annotations

import argparse
import base64
import json
import math
import os
import sys
from pathlib import Path

import numpy as np

from . import asd as ASD, checkgold, synth
from ._ref import SR
from .export import flatten
from .pipeline import Oracle
from .split import bump, golden_sha256, public_text_problems

HERE = Path(__file__).resolve().parent
GOLD = HERE / "goldens"
CASES = GOLD / "cases_public_v1.json"
E2E = GOLD / "oracle_public_v1.json"
E2E_SPECTRA = GOLD / "oracle_spectra_public_v1.json"
WL = np.arange(350, 2501).astype(float)
TOL = {"abs": 1e-9, "rel": 1e-9}
SKIP_KEYS = ("dependencies.", "input.file")


def spectra_doc(id_, items, note):
    return {"format": "spyder-bone/golden_spectra", "format_version": 1, "id": id_, "public": True,
            "licence_note": "Public only: JAS mmc2 (Ryder et al. 2026a, J. Archaeol. Sci. 185:106448) and Archaeometry "
                            "SI (Ryder et al. 2026b, doi:10.1111/arcm.70202) rows, synthetic spectra derived from them, "
                            "and unlabelled numbered standard/high-res pairs.",
            "note": note, "spectra": items}


def spec_item(name, x, source, ctx=None):
    x = np.asarray(x, float)
    return {"name": name, "source": source, "scan_context": ctx or {}, "quantity": "reflectance",
            "wl_start_nm": 350.0, "wl_step_nm": 1.0, "sha256": SR.spectrum_sha256(WL, x), "values": x.tolist()}


def load_spectra(paths):
    out = {}
    for p in paths:
        d = json.loads(Path(p).read_text(encoding="utf-8"))
        for s in d.get("spectra", []):
            x = np.asarray(s["values"], float)
            out.setdefault(s["name"], (x, s.get("source", "")))
    return out


def check_asserts(name, flat, asserts):
    bad = []
    for k, want in asserts.items():
        got = flat.get(k)
        if isinstance(want, dict):
            ok = got is not None and all({"ge": got >= v, "gt": got > v, "lt": got < v, "le": got <= v}[op]
                                         for op, v in want.items())
        else:
            ok = got == want
        if not ok:
            bad.append(f"{k}: got {got!r}, want {want!r}")
    if bad:
        raise AssertionError(f"case {name} no longer hits its branch: " + "; ".join(bad))


def build_input(o, case, bases, registry):
    """Returns (kind, payload, ctx, measured spectrum or None)."""
    ctx = dict(case.get("context", {"instrument_class": "asd.labspec4.std"}))
    ctx.setdefault("class_source", "user"); ctx.setdefault("splices_nm", [1000.0, 1800.0])
    s = ctx.get("serial")
    if isinstance(s, str) and s.startswith("registry:"):
        want = {"std": "asd.labspec4.std", "hires": "asd.labspec4.hires"}[s.split(":")[1]]
        ctx["serial"] = min(k for k, v in registry.items() if v == want)
    if "generator" in case:
        x = synth.generate(case["generator"])
    else:
        x = bases[case["base"]][0]
        if case.get("recipe"):
            x = synth.synthesise(o, x, case["recipe"])
    if "asd" in case:
        a = case["asd"]
        ref = 20000.0 * np.exp(-0.5 * ((WL - 1300.0) / 700.0) ** 2) + 2000.0
        ref = ref * a.get("ref_scale", 1.0)
        b = ASD.write_as8(
            x, ref, ctx.get("serial") or 0, 46273.5707, 46273.5786, 1788889308,
            joins=tuple(a.get("joins", (1000.0, 1800.0))))
        return "asd", b, ctx, x
    return "spectrum", x, ctx, x


def _gate(status):
    s = str(status or "")
    return s[len("gated: "):] if s.startswith("gated: ") else None


def check_inputs(P, chk, rec, ctx):
    """The inputs a check golden needs, read from the end-to-end record (per-sign gates and the per-check long-wave
    flag since phase 0c)."""
    if chk == "acquisition":
        st = P.stream_transfer(ctx["instrument_class"], ctx.get("serial"))
        return {"transfer_key": "none" if st in (None, "none") else st.sha256}
    if chk in ("evidence_levels", "zooms_patterns"):
        return {"sd_E": rec["band_noise"]["sd_E"]}
    if chk == "signs":
        return {"gates": {n: _gate(rec["signs"][n].get("status")) for n in ("plaster", "wax", "ester", "burnt")}}
    if chk == "c1":
        spec = any(rec["signs"][n].get("fired") for n in P.cp("c1")["runs_only_if_not_fired"])
        return {"gate": _gate(rec["C1"].get("status")), "specific_fired": bool(spec),
                "tail_unreliable": bool(rec["checks"]["longwave"]["tail_unreliable_c1"]),
                "longwave_mode": P.cp("longwave")["when_unreliable"]["c1"]}
    if chk == "b9":
        return {"truncated": bool(rec["checks"]["longwave"]["tail_unreliable"])
                and P.cp("longwave")["when_unreliable"]["b9"] == "truncated"}
    if chk == "longwave":
        return {"instrument_class": ctx["instrument_class"]}
    return {}


def run_case(o, kind, payload, ctx, name):
    if kind == "asd":
        return o.analyse_file(payload, ctx["instrument_class"], ctx.get("class_source", "user"), name=name + ".asd")
    return o.analyse_spectrum(WL, payload, ctx)


def golden_flat(rec):
    return {k: v for k, v in flatten(rec).items() if not k.startswith(SKIP_KEYS)}


def compare(flat_got, flat_want, name):
    res = []
    for k, w in flat_want.items():
        g = flat_got.get(k, "<missing>")
        if isinstance(w, float) and not isinstance(w, bool):
            ok = isinstance(g, (int, float)) and not isinstance(g, bool) and math.isfinite(g) and \
                abs(g - w) <= TOL["abs"] + TOL["rel"] * abs(w)
        else:
            ok = g == w
        res.append((name, k, g, w, ok))
    extra = sorted(set(flat_got) - set(flat_want))
    if extra:
        res.append((name, "unexpected outputs", extra[:5], [], False))
    return res


def _write_if_changed(path, doc, cases_key="golden"):
    """Replace doc's golden block; PATCH bump when the goldens changed. Returns a status text."""
    old = json.loads(path.read_text(encoding="utf-8"))
    if golden_sha256(old) == golden_sha256(doc):
        return "goldens unchanged"
    if old.get("golden", {}).get("cases") and os.environ.get("SPYDER_PRERELEASE") != "1":
        doc["version"] = bump(old["version"], "patch")      # SPYDER_PRERELEASE=1: no bumps before the first release
    bad = public_text_problems(doc)
    if bad:
        raise ValueError(f"{path.name}: private names in public text: {bad}")
    path.write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return f"goldens written, version {doc['version']}"


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--roots", nargs="+", required=True)
    ap.add_argument("--extra-spectra", nargs="*", default=[])
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args(argv)
    o = Oracle(a.roots)
    P = o.P
    plugroot = Path(a.roots[0])
    if a.check:
        return run_stored(o, plugroot)
    C = json.loads(CASES.read_text(encoding="utf-8"))
    paths = list(P.spectra_files.values()) + [Path(p) for p in a.extra_spectra]
    if E2E_SPECTRA.exists():
        paths = [E2E_SPECTRA] + paths
    bases = load_spectra(paths)
    registry = P.registry()
    e2e_cases, e2e_spectra, chk_spectra = [], {}, {}
    chk_cases = {k: [] for k in C["check_goldens"]}
    stream_cache = {}
    for case in C["cases"]:
        name = case["name"]
        kind, payload, ctx, x = build_input(o, case, bases, registry)
        rec = run_case(o, kind, payload, ctx, name)
        fl = golden_flat(rec)
        check_asserts(name, fl, case["asserts"])
        item = {"name": name, "purpose": case["purpose"], "context": ctx, "expected": fl}
        if kind == "asd":
            item["asd_b64"] = base64.b64encode(payload).decode("ascii")
        else:
            si = spec_item(name, x, case.get("base", "generator") + (" + synthetic recipe" if case.get("recipe") else ""), ctx)
            item["spectrum_sha256"] = si["sha256"]
            e2e_spectra[si["sha256"]] = si
        if case.get("base") and case["base"] in bases:
            b = bases[case["base"]]
            si = spec_item(case["base"], b[0], b[1])
            e2e_spectra[si["sha256"]] = si
        e2e_cases.append(item)
        # check-file goldens fed by this case
        for chk, names in C["check_goldens"].items():
            if name not in names:
                continue
            if chk in ("acquisition", "longwave"):
                xs, tag = x, "measured"
            else:
                key = (name, "stream")
                if key not in stream_cache:
                    st = P.stream_transfer(ctx["instrument_class"], ctx.get("serial"))
                    if st in (None, "none"):
                        stream_cache[key] = np.asarray(x, float)
                    else:
                        sctx = SR.ScanContext(tuple(ctx["splices_nm"]), ctx["instrument_class"], ctx.get("serial"))
                        stream_cache[key] = SR.apply_transfer(st.doc, WL, np.asarray(x, float)[None, :], sctx)[1][0]
                xs, tag = stream_cache[key], "standard-resolution stream"
            si = spec_item(f"{name} ({tag})", xs, f"oracle golden case {name}, {tag}", ctx)
            chk_spectra[si["sha256"]] = si
            inputs = check_inputs(P, chk, rec, ctx)
            variants = [(name, inputs)]
            if chk == "c1" and inputs["tail_unreliable"]:          # both long-wave modes are engine paths
                other = "not_assessed" if inputs["longwave_mode"] == "truncated" else "truncated"
                variants.append((f"{name} [longwave mode {other}]", dict(inputs, longwave_mode=other)))
            if chk == "b9" and inputs["truncated"]:
                variants.append((f"{name} [full windows]", dict(inputs, truncated=False)))
            for vn, vin in variants:
                got = checkgold.compute_check(chk, P, WL, xs, vin)
                chk_cases[chk].append({"name": vn, "spectrum_sha256": si["sha256"], "inputs": vin, "expected": got})
        print(f"  {name:44s} {rec.get('radiocarbon', {}).get('verdict')!s:24s} {rec.get('radiocarbon', {}).get('rule_step')}")
    # ---- 1. end-to-end goldens
    GOLD.mkdir(exist_ok=True)
    E2E_SPECTRA.write_text(json.dumps(spectra_doc("spyder.oracle.spectra.public.v1", list(e2e_spectra.values()),
                                                  "Spectra of the end-to-end oracle goldens (bases + synthetic)."),
                                      ensure_ascii=False) + "\n", encoding="utf-8")
    e2e = {"format": "spyder-bone/oracle_goldens", "format_version": 1, "id": "spyder.oracle.goldens.public",
           "oracle_version": rec.get("oracle_version"), "spectra_file": E2E_SPECTRA.name, "tolerance": TOL,
           "plugin_dependencies": o.P.used(),
           "compare": "every flattened output; numbers |got - want| <= abs + rel |want|; labels, booleans, null exactly; "
                      "keys starting with " + ", ".join(SKIP_KEYS) + " are not compared",
           "cases": e2e_cases}
    for d in (e2e, json.loads(E2E_SPECTRA.read_text(encoding="utf-8"))):
        bad = public_text_problems(d)
        if bad:
            raise ValueError(f"private names in the public end-to-end goldens: {bad}")
    E2E.write_text(json.dumps(e2e, indent=1, ensure_ascii=False, allow_nan=False) + "\n", encoding="utf-8")
    print("end-to-end goldens:", len(e2e_cases), "cases ->", E2E)
    # ---- 2. check-file goldens
    cdir = P.check("acquisition").path.parent
    sp = cdir / "golden_spectra_checks_v1.json"
    sp.write_text(json.dumps(spectra_doc("spyder.checks.spectra.public.v1", list(chk_spectra.values()),
                                         "Spectra of the engine-check goldens: as measured (acquisition, longwave) or "
                                         "the standard-resolution stream (all others)."), ensure_ascii=False) + "\n",
                  encoding="utf-8")
    for chk, cases in chk_cases.items():
        d = P.check(chk)
        doc = json.loads(d.path.read_text(encoding="utf-8"))
        doc["golden"] = {"spectra_file": sp.name, "tolerance": TOL, "cases": cases}
        print(f"  check.{chk}: {len(cases)} cases;", _write_if_changed(d.path, doc))
    # ---- 3. profile goldens
    for analysis, cases in C["profile_cases"].items():
        if cases == "SEE_COLLAGEN":
            cases = C["collagen_profile_cases"]
        d = P.profile(analysis)
        doc = json.loads(d.path.read_text(encoding="utf-8"))
        out = []
        for c in cases:
            got = checkgold.compute_profile(doc, c["inputs"])
            check_asserts(c["name"], got, c["asserts"])
            out.append({"name": c["name"], "inputs": c["inputs"], "expected": got})
        doc["golden"] = {"tolerance": TOL, "cases": out}
        print(f"  profile.{analysis}: {len(out)} cases;", _write_if_changed(d.path, doc))
    return run_stored(Oracle(a.roots), plugroot)


def run_stored(o, plugroot):
    """Re-run every stored public golden (end-to-end, check files, profiles). Returns the number of failures."""
    P = o.P
    fails = 0
    E = json.loads(E2E.read_text(encoding="utf-8"))
    sp = {s["sha256"]: np.asarray(s["values"], float)
          for s in json.loads((GOLD / E["spectra_file"]).read_text(encoding="utf-8"))["spectra"]}
    n = 0
    for c in E["cases"]:
        if "asd_b64" in c:
            rec = o.analyse_file(base64.b64decode(c["asd_b64"]), c["context"]["instrument_class"],
                                 c["context"].get("class_source", "user"), name=c["name"] + ".asd")
        else:
            rec = o.analyse_spectrum(WL, sp[c["spectrum_sha256"]], c["context"])
        r = compare(golden_flat(rec), c["expected"], c["name"])
        n += len(r)
        bad = [x for x in r if not x[-1]]
        fails += len(bad)
        for b in bad[:5]:
            print("   FAIL", *b)
    print(f"end-to-end: {len(E['cases'])} cases, {n} comparisons, {fails} failed")
    for chk in ("acquisition", "longwave", "b9", "evidence_levels", "zooms_patterns", "signs", "heat", "c1"):
        d = P.check(chk)
        g = d.doc.get("golden", {})
        sfile = d.path.parent / g.get("spectra_file", "")
        spx = {s["sha256"]: (WL, np.asarray(s["values"], float))
               for s in json.loads(sfile.read_text(encoding="utf-8"))["spectra"]} if sfile.is_file() else {}
        try:
            r = checkgold.run(d.doc, P, spx)
            bad = [x for x in r if not x[-1]]
            print(f"check.{chk}@{d.version}: {len(r)} comparisons, {len(bad)} failed")
        except checkgold.GoldenError as e:
            bad = [e]
            print(f"check.{chk}: REJECTED ({e})")
        fails += len(bad)
    for analysis in ("radiocarbon", "isotopes", "zooms"):
        d = P.profile(analysis)
        try:
            r = checkgold.run(d.doc, P, {})
            bad = [x for x in r if not x[-1]]
            print(f"profile.{analysis}@{d.version}: {len(r)} comparisons, {len(bad)} failed")
        except checkgold.GoldenError as e:
            bad = [e]
            print(f"profile.{analysis}: REJECTED ({e})")
        fails += len(bad)
    return fails


if __name__ == "__main__":
    sys.exit(1 if main() else 0)
