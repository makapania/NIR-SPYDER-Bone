"""The end-to-end oracle: PLAN v3.2 Steps 1-10 in the Step 3 order of operations.

  1. read the file; raw acquisition statistics (N2 per window, R range, splice steps) on the AS-MEASURED scan;
  2. resolve the instrument class (an INPUT: the user's Standard / High-res switch for LabSpec files; a file another
     instrument wrote, named by its format in the instrument registry, is scored as that instrument's class; a known
     serial may preset the switch and a mismatch adds a gentle note), then the models, the correction and the profiles;
     the noise policies (per-sign gates, long-wave, soft tier, second opinion, S1_R2 shown) key on the correction applied
     and on whether the stream is the native standard-resolution scan (as measured, or through a near-uncorrected
     correction such as a standard-resolution unit's own file), never on class names (DECISIONS 137);
  3. noise propagation computed for each model and band from the correction actually applied and the scan's own
     detector segments (spyder_ref.tables / gains; no lookup by transfer file);
  4. B6, long-wave eligibility, band readability (an unknown band noise is +infinity: unreadable unless strong, with a
     "band noise unknown" note);
  5. on the standard-resolution-equivalent stream: B9, models, CONS3, evidence, ZooMS pattern, signs, C1;
  6. verdict per analysis type; CSV row + analysis manifest (export.py).
"""
from __future__ import annotations

import math

import numpy as np

from . import asd, checks, verdict as VD
from ._ref import SR
from .params import PluginSet, load_npy, transfer_key
from spyder_ref import gains as GN, tables as TB

ORACLE_VERSION = "1.0.0"
ANALYSES = ("radiocarbon", "isotopes", "zooms")
STD, HIRES = "asd.labspec4.std", "asd.labspec4.hires"


def _f(x):
    if x is None:
        return None
    x = float(x)
    return x if math.isfinite(x) else None


NO_GAIN = "not assessed: no noise gain for this model and transfer"


def model_shown(spec, native, cls):
    """A profile model is computed and shown: on every scan, or only when the stream is not the native
    standard-resolution scan ('non_native': a correction applied, unless near-uncorrected, or another instrument's scan
    as measured); a legacy class list (shown_for_classes) names classes."""
    if spec.get("role") == "verdict_input":
        return True
    w = spec.get("shown_for")
    if w is not None:
        return w == "every_scan" or (w == "non_native" and not native)
    return cls in spec.get("shown_for_classes", [])


def second_opinion_applies(so, native, cls):
    w = so.get("streams")
    if w is not None:
        return w == "every_scan" or (w == "non_native" and not native)
    return cls in so.get("classes", [])


def _transferred(tdoc, wl, R, sctx):
    """The transferred stream on the scan's grid. A transfer that fails, or whose output is not on the scan's grid (a
    block-SNV correction keeps only its block's points), gives a NaN stream: its consumers are not assessed."""
    try:
        w2, X = SR.apply_transfer(tdoc, wl, np.asarray(R, float)[None, :], sctx)
    except SR.SpyderError:
        return np.full(len(wl), np.nan)
    if len(w2) != len(wl) or np.any(np.asarray(w2) != np.asarray(wl)):
        return np.full(len(wl), np.nan)
    return X[0]


def b6_noise(value, sd, p, status="assessed", applies=True):
    """B6 (DECISIONS 109): Check when a verdict cut lies within check_if_cut_within_sd x the implied SD of the reading,
    i.e. when noise alone could carry the reading across a cut. Not applied to an exempt reader; no SD: not assessed,
    with the noise status (a non-linear correction, not available on this instrument, ...); no finite reading: no
    outcome."""
    if not applies:
        return {"status": TB.B6_NOT_APPLIED, "outcome": None}
    if sd is None or not math.isfinite(sd):
        return {"status": NO_GAIN if status in (None, "assessed") else status, "outcome": None}
    v = _f(value)
    if v is None:
        return {"status": "assessed", "outcome": None}
    near = any(abs(v - c) <= p["check_if_cut_within_sd"] * sd for c in p["cuts"])
    return {"status": "assessed", "outcome": "Check" if near else "ok"}


class Oracle:
    def __init__(self, roots):
        self.P = PluginSet(roots)
        self._b9 = None

    # ------------------------------------------------------------------ entry points
    def analyse_file(self, path, instrument_class, class_source="user", name=None):
        """path: an ASD file path (as8, as7 or legacy v1) or its bytes (then give `name`)."""
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
        # other instruments (DECISIONS 87): recorded only when they differ from the LabSpec as8 case, so as8 records
        # are unchanged
        if h["format"] != "as8":
            meta["format"] = h["format"]
        if h["joins_source"] != "header":
            meta["joins_source"] = h["joins_source"]
        if reason:
            return self._rejected(reason, meta)
        R = asd.reflectance_of(h, dn, ref)
        # a file another instrument wrote (named in the instrument registry by its format and detector joins) is scored
        # as that instrument's class: the Standard / High-res switch chooses between the LabSpec classes only
        # (DECISIONS 137). An as7 file with the LabSpec joins 1000/1800 is a LabSpec 4 file, like as8.
        fc = P.class_of_file(h["format"], h["splices_nm"], h["joins_source"])
        if fc is not None and fc != instrument_class:
            instrument_class, class_source = fc, "file_format"
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
        okjoin = asd.joins_accepted(joins, acqp["input_matrix"])
        if not (okgrid and okjoin):
            return self._rejected("unsupported: " + ("wavelength grid" if not okgrid else f"detector joins {list(joins)}"),
                                  meta)
        # ---- Step 2: raw acquisition statistics, as measured
        n2c = checks.N2Cache(wl, R_meas)
        acq = {"B1": {"status": "assessed", "outcome": "ok"}}
        no_dn = ("not assessed: the file stores reflectance only" if meta.get("format") == "legacy"
                 else "not assessed: no raw DN (spectrum input)")
        acq.update(checks.acquisition(wl, R_meas, acqp, dn, joins, no_dn))
        # the stream's correction is resolved (picked) first: the per-sign B6b gates follow the noise after it
        # (DECISIONS 137); it is applied after the Rescan check below. native: the standard-resolution scan as measured,
        # or through a near-uncorrected correction (every noise-policy gain within check.acquisition's std_like_within
        # of uncorrected, e.g. a standard-resolution unit's own file): every noise policy then treats the stream
        # exactly like a native standard-resolution scan (Matt, 2026-10-19: LabSpec 4 standard never changes)
        st = P.stream_transfer(cls, serial)
        sops, sinn = GN.correction_of(st.doc if st not in (None, "none") else None)
        near = checks.standard_like(acqp, P.cp("longwave"), P.cp("signs"), wl, joins, sops, sinn)
        native = (checks.NEAR_UNCORRECTED if near else
                  checks.NATIVE_SCAN if (cls == STD and st == "none") else None)
        acq["B6b"], gates, reduced = checks.b6b(acqp, n2c, wl, joins, sops, sinn, near=near)
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
            # no correction for this instrument is installed: every result is uncalibrated for it
            rec["instrument"]["notes"].append({"key": "instrument_uncalibrated", "class": cls})
            stream_t = None
        elif st == "none":
            R_std, skey, stream_t = R_meas, "none", None
        else:
            R_std = _transferred(st.doc, wl, R_meas, sctx)
            skey, stream_t = transfer_key(st), st
            P._cache[("used_transfer", st.id)] = st
        rec["stream"] = {"transfer": stream_t.ref if stream_t else "none",
                         "transfer_sha256": stream_t.sha256 if stream_t else None,
                         "provisional": bool(stream_t and stream_t.doc.get("provisional", True))}
        # ---- Step 4: long-wave eligibility, band readability (gains computed for the stream's correction), [B6 with
        # the models below]
        lw = checks.longwave(P.cp("longwave"), native, n2c, wl, joins, sops, sinn)
        rec["checks"]["longwave"] = lw
        bands = P.bands(); bdoc = P.bands_doc()
        sd_E, unknown = {}, {}
        for b in sorted(bands):
            sd, why = TB.readability_sd(bdoc, b, wl, joins, sops, sinn, n2c)
            if sd is None:
                continue
            sd_E[b] = sd
            if why != "assessed":
                unknown[b] = why
        rec["band_noise"] = {"transfer_key": skey, "sd_E": {b: _f(v) for b, v in sorted(sd_E.items())}}
        if unknown:
            rec["band_noise"]["noise_unknown"] = unknown
            rec["instrument"]["notes"].append({"key": "band_noise_unknown", "bands": sorted(unknown)})
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
        rec["models"], per_profile, noise = self._models([profs[a].doc["parameters"] for a in ANALYSES], wl, R_meas,
                                                         sctx, n2c, cls, native)
        b6p = acqp["B6_noise"]
        for k, mr in rec["models"].items():
            why, applies = noise.get(k, (NO_GAIN, True))
            mr["B6"] = b6_noise(mr.get("value"), mr.get("implied_sd"), b6p, why, applies)
        # evidence and ZooMS
        ev = checks.evidence(wl, E, evp, bands, sd_E)
        zo = checks.zooms(wl, E, P.cp("zooms_patterns"), bands, sd_E)
        # no correction for this instrument (uncalibrated): the band calls are off (DECISIONS 137, amended 2026-10-19)
        uncal = st is None
        if uncal:
            checks.band_evidence_off(ev, zo)
        rec["evidence"] = ev; rec["zooms_pattern"] = zo
        # signs and C1
        # heat reads the corrected stream when the correction's visible levels were fitted (or none applies), else the
        # scan as measured, with a note (DECISIONS 137: the levels are not display-only)
        fitted, so_gap, _ = SR.transfer_settings(stream_t.doc) if stream_t else (True, None, {})
        if not fitted:
            rec["instrument"]["notes"].append({"key": "visible_levels_not_fitted"})
        sg = checks.signs(wl, R_std, P.cp("signs"), P.cp("heat"), gates, reduced,
                          heat_stream=None if fitted else R_meas)
        spec = any(sg[n].get("fired") for n in P.cp("c1")["runs_only_if_not_fired"])
        c1r = checks.c1(wl, E, P.cp("c1"), gates.get("C1"), spec, bool(lw["tail_unreliable_c1"]),
                        lwp["when_unreliable"]["c1"], reduced.get("C1"))
        sg["soft"] = checks.soft_tier(P.cp("signs"), P.cp("c1"), native, n2c, sg, c1r, wl, joins, sops, sinn)
        rec["signs"] = sg; rec["C1"] = c1r
        # ---- Step 9: verdicts
        notfit = b9r.get("outcome") == "fail"
        # B9 ("Doesn't look like bone") replaces the verdict on every instrument, also one not built in, where the
        # readings otherwise set it (DECISIONS 137): a deliberate exception. B9 is an input screen, not band evidence:
        # the highest Pearson correlation of the scan's D2 profile with bone prototypes, a shape test that a uniform
        # band-depth (calibration) change does not move; it is about rock and other targets, not bone quality
        # (DECISIONS 118). On the 61 real FieldSpec 3 / Trek scans it passed on all.
        for ai, a in enumerate(ANALYSES):
            pp = profs[a].doc["parameters"]
            m, comps, s1 = per_profile[ai]
            if notfit:
                rec[a] = {"verdict": "Doesn't look like bone", "rule_step": "B9", "notes_all": [], "notes_shown": [],
                          "flags": VD.flags_for(pp, sg, c1r, provisional=uncal)}
            elif pp["kind"] == "collagen_rule_L2":
                rec[a] = VD.collagen_verdict(pp, m, comps, ev, zo, sg, c1r, gated, rec.get("models"), uncalibrated=uncal)
            else:
                rec[a] = VD.zooms_verdict(pp, zo, sg, c1r, m, uncalibrated=uncal)
            so = pp.get("second_opinion")
            # S1_R2 stays shown on an uncalibrated instrument, but its second-opinion note is not added there: the
            # instrument note already says the readings are uncalibrated (DECISIONS 137, amended 2026-10-19)
            if (not notfit and not uncal and so and s1 is not None and m is not None
                    and second_opinion_applies(so, native, cls)):
                # the correction's own gap when it carries one (estimated from >= 20 scans), else the profile's
                if abs(s1 - m) > (so_gap if so_gap is not None else so["gap_points"]):
                    rec[a]["notes_all"].append({"key": "second_opinion_differs", "s1": s1, "m": m})
            self._sort(rec[a], a, m, ev.get("S"))
        self._finish(rec, n2c)
        return rec

    # ------------------------------------------------------------------ helpers
    def _sort(self, out, analysis, m, S):
        """'most promising first' key of a verdict block (Codex Phase 3 review 5: every verdict has one)."""
        pp = self.P.profile(analysis).doc["parameters"]
        g, val = VD.sort_key(pp, out["verdict"], m, S)
        out["sort_group"], out["sort_value"] = g, val
        pen = VD.sort_penalty(pp, out)
        if pen is not None:
            out["sort_penalty"] = pen

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

    def _models(self, pps, wl, R_meas, sctx, n2c, cls, native=False):
        """pps: the profiles' parameter blocks. Returns (models record, [(m, comps, s1) per profile], {model key: (noise
        status, whether B6 applies)}). A model whose spec says reads 'as_measured' (S1_R2) runs on the scan as measured
        for any class; a correction's reader_offsets are added to the readings it makes (a consensus: to its components
        before the median)."""
        P = self.P; G = P.gains()
        out, noise = {}, {}
        tcache = {}
        specs = []                                    # union of the profiles' model specs, first profile first
        for pp in pps:
            for spec in pp["models"]:
                if spec["key"] not in [s["key"] for s in specs]:
                    specs.append(spec)
        shown = {s["key"]: any(model_shown(x, native, cls) for pp in pps for x in pp["models"] if x["key"] == s["key"])
                 for s in specs}
        dn = pps[0].get("domain_note_ratio_above")
        for spec in specs:
            md = P.model(spec["model_id"])
            if not shown[spec["key"]]:
                continue
            # the per-reader stream policy (spyder_ref.pick_reader_transfer; the predictions share it)
            pick = "none" if spec.get("reads") == "as_measured" else P.transfer_doc_for(md.doc, sctx)
            if pick is None:
                X, tdoc, tref = R_meas, None, "none (no transfer available)"
            elif pick == "none":
                X, tdoc, tref = R_meas, None, "none"
            else:
                if pick.id not in tcache:
                    tcache[pick.id] = _transferred(pick.doc, wl, R_meas, sctx)
                X, tdoc, tref = tcache[pick.id], pick.doc, pick.ref
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
            offs = SR.transfer_settings(tdoc)[2] if tdoc is not None else {}
            if offs and "value" in pr:
                if md.doc.get("kind") == "consensus":
                    co = {c["name"]: offs[c["id"]] for c in md.doc["consensus"]["components"] if c["id"] in offs}
                    if co:
                        vals = [float(pr[f"component:{c['name']}"][0]) + co.get(c["name"], 0.0)
                                for c in md.doc["consensus"]["components"]]
                        for c in md.doc["consensus"]["components"]:
                            if c["name"] in co:
                                feats[c["name"]] = _f(float(pr[f"component:{c['name']}"][0]) + co[c["name"]])
                        r["value"] = _f(float(np.median(vals)))
                        r["component_offsets"] = co
                elif md.id in offs:
                    r["value"] = _f(float(pr["value"][0]) + offs[md.id])
                    r["reader_offset"] = offs[md.id]
            ops, inn = GN.correction_of(tdoc)
            isd = TB.implied_sd(G.doc, md.doc, wl, sctx.splices_nm, ops, inn, n2c)
            if spec["role"] == "verdict_input":
                r["components"] = {c: feats.get(c) for c in spec["components"]}
                if isd["component_sd"] is not None:
                    r["component_sd"] = dict(isd["component_sd"])
            r["implied_sd"] = isd["sd"]
            noise[spec["key"]] = (isd["status"], isd["b6"])
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
        return out, per, noise
