"""Golden generator: expected values for plug-in goldens from research paths INDEPENDENT of spyder_ref.core.

Developer tool (needs scipy); never imported by the engine. Golden values must come from code that does not share
the engine's implementation, so a bug in the engine cannot write its own expected values:

  models     scipy.signal.savgol_filter (mode 'interp') + numpy absorbance / crop / block SNV -> centred linear form.
             Every feature (and every SNV block) must lie at least window//2 channels inside the grid, where scipy's
             'interp' edge and the engine's 'zero_edges' agree (asserted).
  consensus  the median of the COMPONENT model files' research predictions (not the folded features in the file).
  transfers  an explicit per-channel loop: Gaussian taps to radius ceil(truncate*sigma) (or round), renormalised inside
             the detector segment, never crossing a join; then the per-segment absorbance affine in numpy.
  N2         a line-by-line loop over the second differences (checked against spyder_ref.n2).
"""
from __future__ import annotations
import math
import numpy as np
from scipy.signal import savgol_filter

WL = np.arange(350, 2501).astype(float)


def _wl(spec):
    x = np.asarray(spec["values"], float)
    return spec["wl_start_nm"] + spec["wl_step_nm"] * np.arange(len(x)), x


# ------------------------------------------------------------------ models
def research_features(doc, wl, R):
    X = np.atleast_2d(np.asarray(R, float)); wl = np.asarray(wl, float)
    n0 = len(wl)
    sg_h = None
    for op in doc["preprocessing"]:
        o = op["op"]
        if o == "absorbance":
            X = np.log10(1.0 / np.maximum(X, op["clip_min"]))
        elif o == "savgol":
            if len(wl) != n0:
                raise ValueError("research path runs SG on the full grid only")
            X = savgol_filter(X, op["window"], op["polyorder"], deriv=op.get("deriv", 0), delta=op.get("delta", 1.0), axis=1)
            sg_h = op["window"] // 2
        elif o == "crop":
            m = (wl >= op["range_nm"][0] - 1e-9) & (wl <= op["range_nm"][1] + 1e-9); X, wl = X[:, m], wl[m]
        elif o == "snv":
            blocks = op["blocks_nm"]
            parts, wls = [], []
            for lo, hi in blocks:
                m = (wl >= lo - 1e-9) & (wl <= hi + 1e-9)
                assert sg_h is not None and lo - sg_h > wl[0] + sg_h and hi + sg_h < wl[-1] - sg_h
                B = X[:, m]
                parts.append((B - B.mean(1, keepdims=True)) / B.std(1, ddof=op.get("ddof", 0), keepdims=True)); wls.append(wl[m])
            X, wl = np.hstack(parts), np.concatenate(wls)
        else:
            raise ValueError(f"research path has no op {o}")
    fw = np.asarray(doc["features"]["wavelengths_nm"], float)
    idx = [int(np.flatnonzero(np.abs(wl - f) < 1e-9)[0]) for f in fw]
    if sg_h is not None and not any(op["op"] == "snv" for op in doc["preprocessing"]):
        assert fw.min() - sg_h > 350 + sg_h and fw.max() + sg_h < 2500 - sg_h, "feature inside the SG edge zone"
    return X[:, idx]


def research_predict(doc, wl, R):
    """(value, features) of a regression model file on one spectrum."""
    F = research_features(doc, wl, R); L = doc["regression"]
    c = np.asarray(L.get("x_center", np.zeros(F.shape[1])), float)
    return float(((F - c) @ np.asarray(L["coefficients"], float) + L["offset"])[0]), F[0]


def research_consensus(doc, components, wl, R):
    """components: {name: component model doc}. Returns (value, {component:<name>: value})."""
    vals = {}
    for k in doc["consensus"]["components"]:
        vals[f"component:{k['name']}"] = research_predict(components[k["name"]], wl, R)[0]
    v = sorted(vals.values())
    return v[len(v) // 2], vals


# ------------------------------------------------------------------ transfers
def _seg_bounds(wl, joins):
    cuts = [0]
    for j in sorted(joins):
        if wl[0] <= j < wl[-1]:
            cuts.append(int(np.flatnonzero(wl <= j + 1e-6)[-1]) + 1)
    cuts.append(len(wl))
    return [(a, b) for a, b in zip(cuts[:-1], cuts[1:]) if b > a]


def research_transfer(doc, wl, R, joins):
    R = np.asarray(R, float).copy(); wl = np.asarray(wl, float)
    segs = _seg_bounds(wl, joins)
    for op in doc["operator"]:
        if op["op"] == "gaussian_blur":
            sv = op["sigma_nm"]; sv = [sv] * len(segs) if isinstance(sv, (int, float)) else sv
            out = R.copy()
            for (a, b), s in zip(segs, sv):
                if s <= 0:
                    continue
                r = math.ceil(op.get("truncate", 4.0) * s) if op.get("radius", "round") == "ceil" \
                    else math.floor(op.get("truncate", 4.0) * s + 0.5)
                for i in range(a, b):
                    num = den = 0.0
                    for j in range(max(a, i - r), min(b, i + r + 1)):
                        w = math.exp(-0.5 * ((wl[j] - wl[i]) / s) ** 2); num += w * R[j]; den += w
                    out[i] = num / den
            R = out
        elif op["op"] == "absorbance_affine":
            g, o = op["gain"], op.get("offset", 0.0)
            g = [g] * len(segs) if isinstance(g, (int, float)) else g
            o = [o] * len(segs) if isinstance(o, (int, float)) else o
            out = R.copy()
            for (a, b), gv, ov in zip(segs, g, o):
                A = np.log10(1.0 / np.maximum(R[a:b], op["clip_min"]))
                out[a:b] = 10.0 ** (-(gv * A + ov))
            R = out
        else:
            raise ValueError(f"research transfer has no op {op['op']}")
    return R


# ------------------------------------------------------------------ N2
def n2_loop(R, lo, hi, wl=WL):
    A = np.log10(1.0 / np.maximum(np.asarray(R, float), 1e-4))
    v = np.array([A[i + 1] - 2.0 * A[i] + A[i - 1] for i in range(1, len(A) - 1) if lo <= wl[i] < hi])
    return 1.4826 * float(np.median(np.abs(v - np.median(v)))) * 1e5


# ------------------------------------------------------------------ case builders
def model_cases(doc, spectra, feature_case=None, note="research: scipy savgol_filter + numpy (spyder_ref.golden_gen)"):
    """One golden case per spectrum (dicts in golden-spectra format). feature_case: name of the spectrum that also
    carries the feature vector (default: the first)."""
    out = []
    for i, sp in enumerate(spectra):
        wl, x = _wl(sp)
        v, F = research_predict(doc, wl, x)
        c = {"name": sp["name"], "spectrum_sha256": sp["sha256"], "scan_context": sp.get("scan_context", {}),
             "expected": {"value": v, "linear": v}, "generated_by": note}
        if (feature_case is None and i == 0) or sp["name"] == feature_case:
            c["features"] = [float(t) for t in F]
        out.append(c)
    return out


def consensus_cases(doc, components, spectra, note="research: median of the component files' scipy-path predictions "
                                                      "(spyder_ref.golden_gen)"):
    out = []
    for sp in spectra:
        wl, x = _wl(sp)
        v, comp = research_consensus(doc, components, wl, x)
        out.append({"name": sp["name"], "spectrum_sha256": sp["sha256"], "scan_context": sp.get("scan_context", {}),
                    "expected": dict(value=v, **comp), "generated_by": note})
    return out


def transfer_cases(doc, spectra, note="research: explicit segment-blur loop + numpy per-segment absorbance affine "
                                      "(spyder_ref.golden_gen)"):
    out = []
    for sp in spectra:
        wl, x = _wl(sp)
        joins = sp.get("scan_context", {}).get("splices_nm", [1000.0, 1800.0])
        Rt = research_transfer(doc, wl, x, joins)
        out.append({"name": sp["name"], "spectrum_sha256": sp["sha256"], "scan_context": sp.get("scan_context", {}),
                    "expected": {"transferred_at": {str(int(round(l))): float(v) for l, v in zip(wl, Rt)}},
                    "generated_by": note})
    return out
