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
  gains      (format-2 noise and band tables) the phase 0b p04 method: kernels folded with scipy's SG coefficients, the
             correction's adjoint as an explicit segment blur matrix (transposed) x the segment's absorbance gain, the
             variance from the kernel as a second difference against the differenced noise Gaussian's autocorrelation.
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


# ------------------------------------------------------------------ noise and readability gains (format 2 tables)
# Research path for the computed gains (spyder_ref.gains): the p04 method of phase 0b. The output's absorbance kernel
# is folded with scipy's Savitzky-Golay coefficients; the correction's adjoint is an explicit segment blur MATRIX
# (transposed) times the segment's absorbance gain; the variance comes from the kernel written as a second difference
# (two cumulative sums) against the autocorrelation of the differenced noise Gaussian (taps to +-ceil(8 sigma)).
def _research_sg(window, polyorder, deriv, delta):
    from scipy.signal import savgol_coeffs
    return savgol_coeffs(int(window), int(polyorder), deriv=int(deriv), delta=float(delta), use="dot")


def research_band_kernel(bands_doc, band, wl=WL):
    """Plain band kernel (no projection): mean over centre +- half_width of scale x SG(absorbance)."""
    b = bands_doc["bands"][band]; k = bands_doc["kernels"][b["kernel"]]; sg = k["savgol"]
    c = _research_sg(sg["window"], sg["polyorder"], sg.get("deriv", 0), sg.get("delta", 1.0))
    h = int(sg["window"]) // 2
    w = np.zeros(len(wl))
    pts = [i for i in range(len(wl)) if b["centre_nm"] - b["half_width_nm"] - 1e-9 <= wl[i] <= b["centre_nm"] + b["half_width_nm"] + 1e-9]
    for i in pts:
        assert h <= i < len(wl) - h
        w[i - h:i + h + 1] += float(k["scale"]) * c / len(pts)
    return w


def research_model_kernels(doc, wl=WL):
    """{component name or None: absorbance kernel} of a regression [absorbance, savgol] model or a consensus file."""
    ops = [op["op"] for op in doc["preprocessing"]]
    if doc.get("kind") == "consensus":
        assert ops == ["absorbance"], ops
        out = {}
        for c in doc["consensus"]["components"]:
            w = np.zeros(len(wl))
            for lam, b in zip(c["feature"]["wavelengths_nm"], c["feature"]["weights"]):
                w[int(np.flatnonzero(np.abs(wl - lam) <= 1e-6)[0])] += b
            out[c["name"]] = w
        return out
    assert ops == ["absorbance", "savgol"], ops
    sg = doc["preprocessing"][1]
    c = _research_sg(sg["window"], sg["polyorder"], sg.get("deriv", 0), sg.get("delta", 1.0))
    h = int(sg["window"]) // 2
    w = np.zeros(len(wl))
    for lam, b in zip(doc["features"]["wavelengths_nm"], doc["regression"]["coefficients"]):
        i = int(np.flatnonzero(np.abs(wl - lam) <= 1e-6)[0])
        assert h <= i < len(wl) - h
        w[i - h:i + h + 1] += b * c
    return {None: w}


def _research_blur_matrix(wl, a, b, sigma, truncate, radius):
    n = b - a
    B = np.zeros((n, n))
    r = math.ceil(truncate * sigma) if radius == "ceil" else math.floor(truncate * sigma + 0.5)
    for i in range(a, b):
        js = range(max(a, i - r), min(b, i + r + 1))
        t = np.array([math.exp(-0.5 * ((wl[j] - wl[i]) / sigma) ** 2) for j in js])
        B[i - a, [j - a for j in js]] = t / t.sum()
    return B


def _research_seg_value(v, s):
    return float(v) if isinstance(v, (int, float)) else float(v[s])


def _research_d2(w):
    nz = np.flatnonzero(w != 0); a, b = nz[0], nz[-1]
    h = np.cumsum(np.cumsum(w[a:b + 1])); s = np.abs(h).max()
    assert abs(h[-1]) < 1e-9 * s and abs(h[-2]) < 1e-9 * s, "kernel is not a second difference"
    return h[:-2]


def _research_var_ratio(h, correlation_nm, step=1.0):
    """var(h . n) / var(d2 . n) for n = white noise smoothed by the Gaussian taps, from the explicit covariance matrix
    C[i, j] = autocorrelation of the taps at lag i - j."""
    sd = correlation_nm / step
    if sd > 0:
        r = math.ceil(8.0 * sd)
        g = np.exp(-0.5 * (np.arange(-r, r + 1) / sd) ** 2); g /= g.sum()
    else:
        g = np.array([1.0])
    ac = np.correlate(g, g, "full")[len(g) - 1:]

    def quad(v):
        n = len(v)
        lag = np.abs(np.arange(n)[:, None] - np.arange(n)[None, :])
        C = np.where(lag < len(ac), ac[np.minimum(lag, len(ac) - 1)], 0.0)
        return float(v @ C @ v)
    return quad(np.asarray(h, float)) / quad(np.array([1.0, -2.0, 1.0]))


def research_gains(w, wl, joins, ops=None, correlation_nm=1.25):
    """{segment index: gain per unit N2} of an absorbance kernel through a correction (ops None = none). Each segment's
    part on the stream must be a second difference (asserted, as the p04 method requires)."""
    segs = _seg_bounds(wl, joins)
    out = {}
    for s, (a, b) in enumerate(segs):
        part = w[a:b].copy()
        if not np.any(part != 0):
            continue
        full = np.zeros(len(wl)); full[a:b] = part
        _research_d2(full)
        for op in reversed(ops or []):
            if op["op"] == "absorbance_affine":
                part = _research_seg_value(op["gain"], s) * part
            elif op["op"] == "gaussian_blur":
                sg = _research_seg_value(op["sigma_nm"], s)
                if sg > 0:
                    part = _research_blur_matrix(wl, a, b, sg, op.get("truncate", 4.0), op.get("radius", "round")).T @ part
            else:
                raise ValueError(f"research gains: no op {op['op']}")
        nz = np.flatnonzero(part)
        out[s] = 1e-5 * math.sqrt(_research_var_ratio(part[nz[0]:nz[-1] + 1], correlation_nm, wl[1] - wl[0]))
    return out


def research_window_segment(wl, joins, lo, hi):
    """The segment holding the N2 window's centres (lo <= wl < hi) and their neighbours, else None (also when the grid
    does not hold the whole window: a point below lo and one at or above hi)."""
    segs = _seg_bounds(wl, joins)
    if not (wl[0] < lo and wl[-1] >= hi):
        return None
    idx = [i for i in range(1, len(wl) - 1) if lo <= wl[i] < hi]
    if not idx:
        return None
    for s, (a, b) in enumerate(segs):
        if a <= idx[0] - 1 and idx[-1] + 1 < b:
            return s
    return None


def research_band_sd(bands_doc, band, wl, R, joins, ops=None, inoise=None):
    r = bands_doc["bands"][band]["readability"]
    g = research_gains(research_band_kernel(bands_doc, band, wl), wl, joins, ops,
                       (inoise or {}).get("correlation_nm", bands_doc["noise_model"]["correlation_nm"]))
    (s, gain), = g.items()
    lo, hi = r["noise_window_nm"]
    assert research_window_segment(wl, joins, lo, hi) == s
    fac = ((inoise or {}).get("ch_envelope_factors") or {}).get(band, r.get("envelope_factor", 1.0))
    return gain * fac * n2_loop(R, lo, hi, wl)


def research_model_sd(ng, doc, wl, R, joins, ops=None, inoise=None):
    rec = (ng.get("models", {}).get(doc["id"]) or {}).get("recorded")
    if rec is not None:
        assert ops is None
        return math.sqrt(sum((t["gain"] * n2_loop(R, *t["window_nm"], wl)) ** 2 for t in rec["terms"]))
    corr = (inoise or {}).get("correlation_nm", ng["noise_model"]["correlation_nm"])
    sds = []
    for w in research_model_kernels(doc, wl).values():
        s2 = 0.0
        for s, gain in research_gains(w, wl, joins, ops, corr).items():
            win = [x for x in ng["model_noise_windows_nm"] if research_window_segment(wl, joins, *x) == s][0]
            s2 += (gain * n2_loop(R, *win, wl)) ** 2
        sds.append(math.sqrt(s2))
    if doc.get("kind") == "consensus":
        return ng["consensus"][doc["id"]]["factor"] * float(np.median(sds))
    return sds[0]
