"""The end-to-end oracle: PLAN v3.2 Steps 1-10 in the Step 3 order of operations.

  1. read the file; raw acquisition statistics (N2 per window, R range, splice steps) on the AS-MEASURED scan;
  2. resolve the instrument class (an INPUT: the user's Standard / High-res switch; a known serial may preset it and a
     mismatch adds a gentle note), then the models, the transfer and the analysis profiles;
  3. select the noise parameters that match each model and the transfer actually applied (keyed by transfer hash);
  4. B6, long-wave eligibility, band readability;
  5. on the standard-resolution-equivalent stream: B9, models, CONS3, evidence, ZooMS pattern, signs, C1;
  6. verdict per analysis type; CSV row + analysis manifest (export.py).
"""
from __future__ import annotations

import math

import numpy as np

from . import asd, checks, verdict as VD
from ._ref import SR
from .params import PluginSet, load_npy, transfer_key

ORACLE_VERSION = "1.0.0"
ANALYSES = ("radiocarbon", "isotopes", "zooms")
STD, HIRES = "asd.labspec4.std", "asd.labspec4.hires"


def _f(x):
    if x is None:
        return None
    x = float(x)
    return x if math.isfinite(x) else None


def _rss(terms, n2c):
    s = 0.0
    for t in terms:
        s += (t["gain"] * n2c(*t["window_nm"])) ** 2
    return math.sqrt(s)


class Oracle:
    def __init__(self, roots):
        self.P = PluginSet(roots)
        self._b9 = None

    # ------------------------------------------------------------------ entry points
    def analyse_file(self, path, instrument_class, class_source="user", name=None):
        """path: an .asd file path or its bytes (then give `name`)."""
        P = self.P
        fname = name if name is not None else (str(path) if not isinstance(path, (bytes, bytearray)) else "<bytes>")
        acqp = P.cp("acquisition")
        try:
            h, wl, dn, ref = asd.read(path)
        except asd.AsdError as e:
            return self._rejected(str(e), {"file": fname})
        reason = asd.acceptance(h, wl, dn, ref, acqp["input_matrix"])
        meta = {"file": fname, "input_sha256": h["bytes_sha256"], "input_kind": "asd",
                "serial": h["serial"], "time_ole": h.get("ole_spec_time"),
                "time_local": asd.ole_to_iso(h.get("ole_spec_time")), "utc_offset_min": asd.utc_offset_minutes(h),
                "integration_time_ms": h["integration_time_ms"], "averages": h["sample_count"],
                "splices_nm": h["splices_nm"]}
        if reason:
            return self._rejected(reason, meta)
        R = asd.reflectance(dn, ref)
        ctx = {"instrument_class": instrument_class, "class_source": class_source, "serial": h["serial"],
               "splices_nm": h["splices_nm"]}
        return self.analyse_spectrum(wl, R, ctx, dn=dn, meta=meta)

    def _rejected(self, reason, meta):
        r = {"oracle_version": ORACLE_VERSION, "input": meta,
             "checks": {"B1": {"status": "assessed", "outcome": "Unusable", "reason": reason}}}
        for a in ANALYSES:
            r[a] = {"verdict": "Rescan" if not reason.startswith("unsupported") else "Unsupported",
                    "rule_step": "B1", "notes_all": [], "notes_shown": [], "flags": []}
            self._sort(r[a], a, None, None)
        r["dependencies"] = self.P.used()
        return r

    # ------------------------------------------------------------------ main
    def analyse_spectrum(self, wl, R_meas, ctx, dn=None, meta=None):
        P = self.P
        wl = np.asarray(wl, float); R_meas = np.asarray(R_meas, float)
        meta = dict(meta or {})
        if "input_sha256" not in meta:
            meta["input_sha256"] = SR.spectrum_sha256(wl, R_meas)
            meta["input_kind"] = "spectrum"
        cls = ctx["instrument_class"]
        serial = ctx.get("serial")
        joins = tuple(ctx.get("splices_nm", (1000.0, 1800.0)))
        rec = {"oracle_version": ORACLE_VERSION, "input": meta,
               "instrument": {"class": cls, "class_source": ctx.get("class_source", "user"), "serial": serial}}
        acqp = P.cp("acquisition")
        # ---- Step 1 grid + joins (spectrum input: the matrix parts that apply)
        g = acqp["input_matrix"]["grid"]
        # Codex Phase 3 review 2: an empty or short spectrum is unsupported input, never an exception
        okgrid = (len(wl) == g["n"] and R_meas.ndim == 1 and len(R_meas) == len(wl) and len(wl) > 0
                  and abs(wl[0] - g["start_nm"]) < 1e-6 and np.allclose(np.diff(wl), g["step_nm"]))
        okjoin = all(abs(a - b) < 1e-3 for a, b in zip(sorted(joins), acqp["input_matrix"]["accepted_joins_nm"]))
        if not (okgrid and okjoin):
            return self._rejected("unsupported: " + ("wavelength grid" if not okgrid else f"detector joins {list(joins)}"),
                                  meta)
        # ---- Step 2: raw acquisition statistics, as measured
        n2c = checks.N2Cache(wl, R_meas)
        acq = {"B1": {"status": "assessed", "outcome": "ok"}}
        acq.update(checks.acquisition(wl, R_meas, acqp, dn))
        # the stream's transfer is resolved (picked) first: the per-sign B6b gates are keyed by it (phase 0c); it is
        # applied after the Rescan check below
        st = P.stream_transfer(cls, serial)
        skey = transfer_key(st) if st not in (None, "none") else "none"
        acq["B6b"], gates = checks.b6b(acqp, n2c, skey)
        gated = acq["B6b"]["gated"]
        rec["checks"] = acq
        bad = checks.unusable(acq)
        # ---- Step 3: class, serial preset/mismatch, models, transfer, profiles
        reg = P.registry()
        notes_inst = []
        if serial is not None and int(serial) in reg and reg[int(serial)] != cls:
            notes_inst.append({"key": "serial_class_mismatch", "serial_class": reg[int(serial)]})
        rec["instrument"]["serial_known_class"] = reg.get(int(serial)) if serial is not None else None
        rec["instrument"]["notes"] = notes_inst
        profs = {a: P.profile(a) for a in ANALYSES}
        rec["profiles"] = {a: profs[a].ref for a in ANALYSES}
        if bad:
            for a in ANALYSES:
                rec[a] = {"verdict": "Rescan", "rule_step": "+".join(bad), "notes_all": [], "notes_shown": [], "flags": []}
                self._sort(rec[a], a, None, None)
            self._finish(rec, n2c)
            return rec
        sctx = SR.ScanContext(joins, cls, serial)
        if st is None:
            R_std, skey = R_meas, "none"
            rec["instrument"]["notes"].append({"key": "no_transfer_available"})
            stream_t = None
        elif st == "none":
            R_std, skey, stream_t = R_meas, "none", None
        else:
            R_std = SR.apply_transfer(st.doc, wl, R_meas[None, :], sctx)[1][0]
            skey, stream_t = transfer_key(st), st
            P._cache[("used_transfer", st.id)] = st
        rec["stream"] = {"transfer": stream_t.ref if stream_t else "none",
                         "transfer_sha256": stream_t.sha256 if stream_t else None,
                         "provisional": bool(stream_t and stream_t.doc.get("provisional", True))}
        # ---- Step 4: long-wave eligibility, band readability (keyed gains), [B6 with the models below]
        lw = checks.longwave(P.cp("longwave"), cls, n2c)
        rec["checks"]["longwave"] = lw
        bands = P.bands(); G = P.gains()
        sd_E = {}
        for b, bd in bands.items():
            gb = P.band_gain(b, skey)
            if gb is not None and bd.get("noise_window_nm"):
                sd_E[b] = gb * n2c(*bd["noise_window_nm"])
        rec["band_noise"] = {"transfer_key": skey, "sd_E": {b: _f(v) for b, v in sorted(sd_E.items())}}
        # ---- Step 5: spectral consumers on the standard-resolution-equivalent stream
        evp = P.cp("evidence_levels")
        from . import kernels as K
        E = K.derivative(R_std, evp["kernel"])
        # B9 (long-wave policy: truncated variant or not assessed when the tail is unreliable)
        b9p = P.cp("b9"); lwp = P.cp("longwave")
        mode_b9 = lwp["when_unreliable"]["b9"]
        if lw["tail_unreliable"] and mode_b9 != "truncated":
            b9r = {"status": "not assessed: long-wave region too noisy", "score": None, "outcome": None}
        else:
            tr = bool(lw["tail_unreliable"])
            b9r = checks.b9(wl, R_std, b9p, self._prototypes(b9p, tr), truncated=tr)
        rec["checks"]["B9"] = b9r
        # models
        # Codex Phase 3 review 1: every profile reads its OWN model list (verdict input, second opinion); the
        # record's models block is the union over the profiles, keyed by the profiles' model keys
        rec["models"], per_profile = self._models([profs[a].doc["parameters"] for a in ANALYSES], wl, R_meas, sctx,
                                                  n2c, cls)
        b6p = acqp["B6_noise"]
        for k, mr in rec["models"].items():
            sd = mr.get("implied_sd")
            mr["B6"] = {"status": "assessed" if sd is not None else "not assessed: no noise gain for this model and transfer",
                        "outcome": None if sd is None else ("Check" if sd > b6p["check_if_implied_sd_above"] else "ok")}
        # evidence and ZooMS
        ev = checks.evidence(wl, E, evp, bands, sd_E)
        zo = checks.zooms(wl, E, P.cp("zooms_patterns"), bands, sd_E)
        rec["evidence"] = ev; rec["zooms_pattern"] = zo
        # signs and C1
        sg = checks.signs(wl, R_std, P.cp("signs"), P.cp("heat"), gates)
        spec = any(sg[n].get("fired") for n in P.cp("c1")["runs_only_if_not_fired"])
        c1r = checks.c1(wl, E, P.cp("c1"), gates.get("C1"), spec, bool(lw["tail_unreliable_c1"]),
                        lwp["when_unreliable"]["c1"])
        sg["soft"] = checks.soft_tier(P.cp("signs"), P.cp("c1"), cls, n2c, sg, c1r)
        rec["signs"] = sg; rec["C1"] = c1r
        # ---- Step 9: verdicts
        notfit = b9r.get("outcome") == "fail"
        for ai, a in enumerate(ANALYSES):
            pp = profs[a].doc["parameters"]
            m, comps, s1 = per_profile[ai]
            if notfit:
                rec[a] = {"verdict": "Doesn't look like bone", "rule_step": "B9", "notes_all": [], "notes_shown": [],
                          "flags": VD.flags_for(pp, sg, c1r)}
            elif pp["kind"] == "collagen_rule_L2":
                rec[a] = VD.collagen_verdict(pp, m, comps, ev, zo, sg, c1r, gated, rec.get("models"))
            else:
                rec[a] = VD.zooms_verdict(pp, zo, sg, c1r, m)
            if not notfit and pp.get("second_opinion") and s1 is not None and m is not None and cls in pp["second_opinion"]["classes"]:
                if abs(s1 - m) > pp["second_opinion"]["gap_points"]:
                    rec[a]["notes_all"].append({"key": "second_opinion_differs", "s1": s1, "m": m})
            self._sort(rec[a], a, m, ev.get("S"))
        self._finish(rec, n2c)
        return rec

    # ------------------------------------------------------------------ helpers
    def _sort(self, out, analysis, m, S):
        """'most promising first' key of a verdict block (Codex Phase 3 review 5: every verdict has one)."""
        g, val = VD.sort_key(self.P.profile(analysis).doc["parameters"], out["verdict"], m, S)
        out["sort_group"], out["sort_value"] = g, val

    def _finish(self, rec, n2c):
        rec["n2"] = n2c.table()
        rec["dependencies"] = self.P.used()

    def _prototypes(self, b9p, truncated=False):
        key = "trunc" if truncated else "full"
        if self._b9 is None:
            self._b9 = {}
        if key not in self._b9:
            sc = (b9p["truncated"] if truncated else b9p)["prototypes"]
            arr = load_npy(self.P.sidecar(sc["sidecar"], sc["sha256"]))
            if list(arr.shape) != list(sc["shape"]):
                raise ValueError("B9 prototypes: wrong shape")
            self._b9[key] = arr
        return self._b9[key]

    def _models(self, pps, wl, R_meas, sctx, n2c, cls):
        """pps: the profiles' parameter blocks. Returns (models record, [(m, comps, s1) per profile])."""
        P = self.P; G = P.gains()
        out = {}
        tcache = {}
        specs = []                                    # union of the profiles' model specs, first profile first
        for pp in pps:
            for spec in pp["models"]:
                if spec["key"] not in [s["key"] for s in specs]:
                    specs.append(spec)
        shown = {s["key"]: any(cls in x["shown_for_classes"] or x["role"] == "verdict_input"
                               for pp in pps for x in pp["models"] if x["key"] == s["key"]) for s in specs}
        dn = pps[0].get("domain_note_ratio_above")
        for spec in specs:
            md = P.model(spec["model_id"])
            if not shown[spec["key"]]:
                continue
            pick = P.transfer_doc_for(md.doc, sctx)
            if pick is None:
                X, tkey, tref = R_meas, "none", "none (no transfer available)"
            elif pick == "none":
                X, tkey, tref = R_meas, "none", "none"
            else:
                if pick.id not in tcache:
                    tcache[pick.id] = SR.apply_transfer(pick.doc, wl, R_meas[None, :], sctx)[1][0]
                X, tkey, tref = tcache[pick.id], transfer_key(pick), pick.ref
                P._cache[("used_transfer", pick.id)] = pick
            try:
                pr = SR.predict(md.doc, wl, X[None, :], sctx)
                status = "assessed"
            except Exception as e:                            # spyder_ref raises SpyderError for an ineligible scan
                pr, status = {}, f"not assessed: {e}"
            r = {"id": md.ref, "sha256": md.sha256, "role": spec["role"], "transfer": tref, "status": status,
                 "value": _f(pr["value"][0]) if "value" in pr else None,
                 "domain_ratio": _f(pr["domain_ratio"][0]) if "domain_ratio" in pr else None}
            feats = {k.split(":", 1)[1]: _f(v[0]) for k, v in pr.items() if k.startswith(("feature:", "component:"))}
            if spec["role"] == "verdict_input":
                r["components"] = {c: feats.get(c) for c in spec["components"]}
                cs = G.consensus(md.id)
                if cs:
                    sds = []
                    for c, mk in cs["components"].items():
                        t = G.model_terms(mk, tkey)
                        sds.append(None if t is None else _rss(t, n2c))
                    r["component_sd"] = dict(zip(cs["components"], sds))
                    r["implied_sd"] = None if None in sds else cs["factor"] * float(np.median(sds))
            else:
                t = G.model_terms(md.id, tkey)
                r["implied_sd"] = None if t is None else _rss(t, n2c)
            r["domain_note"] = bool(dn is not None and r["domain_ratio"] is not None and r["domain_ratio"] > dn)
            out[spec["key"]] = r
        per = []
        for pp in pps:
            m = comps = s1 = None
            for x in pp["models"]:
                r = out.get(x["key"])
                if r is None:
                    continue
                if x["role"] == "verdict_input":
                    m, comps = r["value"], r.get("components", {})
                elif x["role"] == "second_opinion":
                    s1 = r["value"]
            per.append((m, comps or {}, s1))
        return out, per
