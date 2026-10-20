"""Noise propagation for any correction file (DECISIONS 137, Phase 2): the implied SD of a model reading (B6) and the
readability SD of a band reading, COMPUTED from the output's own linear kernel, the correction actually applied and the
scan's own detector segments. Nothing is looked up by a transfer file's SHA-256.

Noise model (the shipped one, phase 0b A2): white noise in absorbance, smoothed by a Gaussian of `correlation_nm`
(1.25 nm for the ASD LabSpec; a correction may give its instrument's own value in
provenance.instrument_noise.correlation_nm), independent between detector segments, measured on the AS-MEASURED scan by
the canonical N2 (the robust SD of the second difference, units 1e-5 absorbance). For an output with absorbance kernel w
the SD per unit N2 of one segment's part is

    gain = 1e-5 x sqrt( |T'w * g|^2 / |d2 * g|^2 )

with g the noise Gaussian (taps to +-8 sigma, in grid steps), d2 = [1, -2, 1] and T' the adjoint of the correction's
operators on that segment, applied in reverse order: a per-segment absorbance gain multiplies; a per-segment Gaussian blur
is its exact (segment-clipped, renormalised) blur matrix transposed. The blur acts on reflectance in the engine and is
linearised here (as the shipped gains were). Each segment's part uses an N2 window inside that segment; parts add in
quadrature (independent detectors).

Linear only: a correction with any other operator (block SNV, ...) has no analytic gain ("non-linear correction"); a model
chain must be absorbance followed by Savitzky-Golay / crop steps and a linear read-out (no y transform). Coverage: every
Savitzky-Golay window of the output (every run of a consensus component's wavelengths) must lie inside the scan's grid and
inside ONE detector segment, and the N2 window it is paired with must lie in that same segment; otherwise the output is
"not available on this instrument". The kernel part in each segment must be a second difference (zero sum and zero first
moment): the noise model says nothing about offsets and slopes.

The same machinery gives the noise of the checks' linear statistics (DECISIONS 137 g2: the contaminant signs, the C1
excess, their reduced forms, the long-wave tail reading): stat_term_gain, one detector term at a time, so the per-sign
gates, the DECISIONS 107 fallbacks, the long-wave cuts and the soft-tier eligibility are decided on the noise AFTER the
correction applied, in SD units.

numpy only. tables.py and the oracle read through this module; the Rust core has the same engine
(crates/spyder-core/src/plugins/gains.rs) and both are held to 1e-9 by the parity fixture.
"""
from __future__ import annotations

import math

import numpy as np

from . import core as SR

DEFAULT_CORRELATION_NM = 1.25
TAPS_SD = 8.0                   # the noise Gaussian is cut at +-8 sigma (tails < 1e-13)
MOMENT_TOL = 1e-9               # a segment part is a second difference when both moments vanish to this (relative)
GRID_TOL = 1e-6


class GainError(SR.SpyderError):
    """No analytic gain for this output on this scan; `status` is the record's status text."""

    def __init__(self, status):
        super().__init__(status)
        self.status = status


def nonlinear(why):
    return GainError(f"not assessed: {why}")


def unavailable(why):
    return GainError(f"not assessed: not available on this instrument ({why})")


def _fmt(x):
    x = float(x)
    return str(int(x)) if x == int(x) else repr(x)


# --------------------------------------------------------------------------- the scan's layout
def layout(wl, joins):
    """(step, segments [(a, b)) of grid indices) of a scan; the grid must be uniform."""
    wl = np.asarray(wl, float)
    if len(wl) < 3:
        raise unavailable("the scan has fewer than 3 grid points")
    step = float(wl[1] - wl[0])
    if not (math.isfinite(step) and step > 0) or np.max(np.abs(np.diff(wl) - step)) > GRID_TOL:
        raise unavailable("the scan's grid is not uniform")
    joins = [float(j) for j in joins if math.isfinite(float(j))]
    return step, SR._segments(wl, joins)


def layout_key(wl, joins):
    wl = np.asarray(wl, float)
    step, segs = layout(wl, joins)
    return (len(wl), repr(float(wl[0])), repr(step), tuple(segs))


def _segment_of(segs, i):
    for s, (a, b) in enumerate(segs):
        if a <= i < b:
            return s
    return None


def _index(wl, lam):
    i = np.flatnonzero(np.abs(wl - lam) <= SR.WL_TOL)
    return int(i[0]) if len(i) == 1 else None


# --------------------------------------------------------------------------- kernels (outputs folded onto absorbance)
class Kernel:
    """A linear functional on the measured absorbance of the scan grid: dense weights `w` and its support `atoms`,
    index ranges [lo, hi] that must each lie inside one detector segment (an SG window, or a run of a folded
    consensus component's wavelengths)."""

    def __init__(self, n):
        self.w = np.zeros(n)
        self.atoms = []


def _sg_adjoint(beta, grid_in, sg, h_grid_out_to_in, n_in, k, what, off=0):
    """Spread output weights through one Savitzky-Golay step (interior centre weights only). `off` = the index of
    grid_in[0] on the scan grid (the atoms are kept in scan-grid indices)."""
    out = np.zeros(n_in)
    c = SR.sg_centre_weights(int(sg["window"]), int(sg["polyorder"]), int(sg.get("deriv", 0)),
                             float(sg.get("delta", 1.0)))
    h = int(sg["window"]) // 2
    for o in np.flatnonzero(beta):
        i = h_grid_out_to_in(int(o))
        if i - h < 0 or i + h > n_in - 1:
            raise unavailable(f"{what}: the Savitzky-Golay window at {_fmt(grid_in[i])} nm runs off the scan's grid")
        out[i - h:i + h + 1] += beta[o] * c
        k.atoms.append((off + i - h, off + i + h))
    return out


def model_kernels(m, wl):
    """[(component name or None, Kernel)] of a model file on this scan grid. Raises GainError (non-linear model or a
    feature off the grid)."""
    wl = np.asarray(wl, float)
    kind = m.get("kind", "regression")
    chain = list(m.get("preprocessing", []))
    if not chain or chain[0].get("op") != "absorbance":
        raise nonlinear("non-linear model (its chain does not start in absorbance)")
    if kind == "regression":
        yt = (m.get("output") or {}).get("y_transform") or {}
        if yt.get("type", "none") != "none":
            raise nonlinear("non-linear model (transformed output)")
    elif kind != "consensus":
        raise nonlinear(f"non-linear model (kind {kind})")
    # forward pass: the grid after each step (always a contiguous run of the scan grid, starting at offs[j])
    grids, offs = [wl], [0]
    maps = []                                     # per step after absorbance: output index -> input index
    for op in chain[1:]:
        g = grids[-1]
        if op["op"] == "savgol":
            h = int(op["window"]) // 2
            if op.get("mode", "interp") == "valid":
                grids.append(g[h:len(g) - h]); offs.append(offs[-1] + h); maps.append(lambda o, h=h: o + h)
            else:
                grids.append(g); offs.append(offs[-1]); maps.append(lambda o: o)
        elif op["op"] == "crop":
            lo, hi = op["range_nm"]
            keep = np.flatnonzero((g >= lo - SR.WL_TOL) & (g <= hi + SR.WL_TOL))
            if len(keep) == 0:
                raise unavailable("the model's crop leaves no grid points")
            grids.append(g[keep[0]:keep[-1] + 1]); offs.append(offs[-1] + int(keep[0]))
            maps.append(lambda o, a=int(keep[0]): o + a)
        else:
            raise nonlinear(f"non-linear model (operator {op['op']})")
    gout = grids[-1]

    def back(beta, k, what):
        for step in range(len(chain) - 1, 0, -1):
            op = chain[step]
            gin = grids[step - 1]
            if op["op"] == "savgol":
                beta = _sg_adjoint(beta, gin, op, maps[step - 1], len(gin), k, what, offs[step - 1])
            else:                                     # crop: place the weights back on the larger grid
                b2 = np.zeros(len(gin))
                for o in np.flatnonzero(beta):
                    b2[maps[step - 1](int(o))] += beta[o]
                beta = b2
        return beta

    def place(wls, coefs, what):
        beta = np.zeros(len(gout))
        idx = []
        for lam, b in zip(wls, coefs):
            i = _index(gout, float(lam))
            if i is None:
                raise unavailable(f"{what}: {_fmt(lam)} nm is not on the scan's grid")
            beta[i] += float(b)
            idx.append(offs[-1] + i)
        return beta, idx

    out = []
    if kind == "regression":
        k = Kernel(len(wl))
        beta, _ = place(m["features"]["wavelengths_nm"], m["regression"]["coefficients"], m["id"])
        k.w = back(beta, k, m["id"])
        out.append((None, k))
    else:
        for c in m["consensus"]["components"]:
            k = Kernel(len(wl))
            f = c["feature"]
            beta, idx = place(f["wavelengths_nm"], f["weights"], f"{m['id']} component {c['name']}")
            k.w = back(beta, k, f"{m['id']} component {c['name']}")
            # the component's runs (its SG steps are folded into the weights): each must sit in one segment
            idx = sorted(set(idx))
            run = [idx[0], idx[0]]
            for i in idx[1:]:
                if i - run[1] <= 1:
                    run[1] = i
                else:
                    k.atoms.append(tuple(run)); run = [i, i]
            k.atoms.append(tuple(run))
            out.append((c["name"], k))
    return out


def band_kernel(bands_doc, band, wl):
    """The plain band kernel on this grid: mean over centre +- half_width of scale x SG(absorbance). A projection is
    ignored (the shipped rule: the projected 1545 nm reading's SD is about 10% below this, so the gate is cautious)."""
    wl = np.asarray(wl, float)
    b = bands_doc["bands"][band]
    kd = bands_doc["kernels"][b["kernel"]]
    c, hw = float(b["centre_nm"]), float(b["half_width_nm"])
    a, z = _index(wl, c - hw), _index(wl, c + hw)          # the pipeline's reading: inclusive window on the grid
    if a is None or z is None or z < a:
        raise unavailable(f"band {band}: {_fmt(c - hw)}-{_fmt(c + hw)} nm is not on the scan's grid")
    beta = np.zeros(len(wl))
    beta[a:z + 1] = float(kd["scale"]) / (z - a + 1)
    k = Kernel(len(wl))
    k.w = _sg_adjoint(beta, wl, dict(kd["savgol"]), lambda o: o, len(wl), k, f"band {band}")
    return k


# --------------------------------------------------------------------------- the correction, per segment
def correction_of(tdoc):
    """(operator list, instrument_noise dict) of a transfer document; (None, {}) for no correction."""
    if tdoc is None or tdoc == "none":
        return None, {}
    prov = tdoc.get("provenance") or {}
    return list(tdoc["operator"]), dict(prov.get("instrument_noise") or {})


def _per_seg(v, s, nseg, op, what):
    if isinstance(v, (int, float)) and not isinstance(v, bool):
        return float(v)
    if len(v) != nseg:
        raise unavailable(f"the correction's {op} gives {len(v)} {what} values; the scan has {nseg} segments")
    return float(v[s])


def _check_linear(ops):
    """Only blur and absorbance gain are linear and propagated; block SNV is non-linear; anything else has no noise
    propagation."""
    for op in ops or []:
        if op["op"] not in ("gaussian_blur", "absorbance_affine"):
            if op["op"] == "snv":
                raise nonlinear("non-linear correction")
            raise nonlinear(f"no noise propagation through the correction's {op['op']} step")


def _back_correction(h, a, b, wl, s, nseg, ops):
    """h (dense over the whole grid, nonzero only in [a, b)) through the adjoint of the correction on segment s."""
    step = float(wl[1] - wl[0])
    for op in reversed(ops or []):
        if op["op"] == "absorbance_affine":
            h = _per_seg(op["gain"], s, nseg, op["op"], "gain") * h
        else:                                          # gaussian_blur: exact clipped, renormalised taps, transposed
            if "sigma_nm" not in op:
                raise nonlinear("no noise propagation through the correction's gaussian_blur step (it gives no sigma_nm)")
            sg = _per_seg(op["sigma_nm"], s, nseg, op["op"], "sigma")
            if sg <= 0:
                continue
            trunc = float(op.get("truncate", 4.0))
            rule = op.get("radius", "round")
            r = int(math.ceil(trunc * sg / step)) if rule == "ceil" else int(math.floor(trunc * sg / step + 0.5))
            out = np.zeros_like(h)
            for i in np.flatnonzero(h):
                lo, hi = max(a, i - r), min(b, i + r + 1)
                t = np.exp(-0.5 * ((wl[lo:hi] - wl[i]) / sg) ** 2)
                out[lo:hi] += h[i] * t / t.sum()
            h = out
    return h


def noise_taps(correlation_nm, step):
    sd = float(correlation_nm) / step
    if not sd > 0:
        return np.array([1.0])
    r = int(math.ceil(TAPS_SD * sd))
    t = np.arange(-r, r + 1, dtype=float)
    return np.exp(-0.5 * (t / sd) ** 2)


def _var_ratio(h, g):
    u = np.convolve(h, g)
    v = np.convolve(np.array([1.0, -2.0, 1.0]), g)
    return float(np.dot(u, u) / np.dot(v, v))


def segment_gains(k, wl, joins, ops=None, correlation_nm=DEFAULT_CORRELATION_NM, what="output"):
    """[(segment index, gain per unit N2)] of a Kernel on a scan's layout through a correction (ops None = none)."""
    wl = np.asarray(wl, float)
    step, segs = layout(wl, joins)
    _check_linear(ops)
    for lo, hi in k.atoms:
        s0, s1 = _segment_of(segs, lo), _segment_of(segs, hi)
        if s0 is None or s1 is None:
            raise unavailable(f"{what}: {_fmt(wl[lo])}-{_fmt(wl[hi])} nm is outside the scan")
        if s0 != s1:
            j = wl[segs[s0][1] - 1]
            raise unavailable(f"{what}: {_fmt(wl[lo])}-{_fmt(wl[hi])} nm crosses the detector join at {_fmt(j)} nm")
    g = noise_taps(correlation_nm, step)
    out = []
    for s, (a, b) in enumerate(segs):
        part = np.zeros(len(wl))
        part[a:b] = k.w[a:b]
        if not np.any(part != 0):
            continue
        nz = np.flatnonzero(part)
        p = part[nz[0]:nz[-1] + 1]
        x = np.arange(len(p), dtype=float) - (len(p) - 1) / 2.0
        if abs(p.sum()) > MOMENT_TOL * np.abs(p).sum() or abs((x * p).sum()) > MOMENT_TOL * np.abs(x * p).sum():
            raise nonlinear(f"{what} is not a second-derivative reading (the noise model does not apply)")
        h = _back_correction(part, a, b, wl, s, len(segs), ops)
        nz = np.flatnonzero(h)
        if len(nz) == 0:                               # nothing of this part survives the correction
            continue
        out.append((s, 1e-5 * math.sqrt(_var_ratio(h[nz[0]:nz[-1] + 1], g))))
    if not out:
        raise unavailable(f"{what}: no weight on the scan")
    return out


def window_covered(wl, lo, hi):
    """True when the scan's grid holds the WHOLE N2 window: every centre lo <= wl < hi and both neighbours its second
    difference reads (a grid point below lo and one at or above hi). A window the grid only partly covers has fewer
    second differences than the noise measure assumes (one centre gives a zero MAD): not available, never "quiet"."""
    wl = np.asarray(wl, float)
    return bool(len(wl) >= 3 and wl[0] < lo and wl[-1] >= hi and np.any((wl >= lo) & (wl < hi)))


def window_segment(wl, segs, lo, hi):
    """The segment holding every grid point an N2 window reads (centres lo <= wl < hi and their neighbours), or None
    (also when the grid does not cover the whole window)."""
    wl = np.asarray(wl, float)
    if not window_covered(wl, lo, hi):
        return None
    c = np.flatnonzero((wl >= lo) & (wl < hi))
    s0, s1 = _segment_of(segs, int(c[0]) - 1), _segment_of(segs, int(c[-1]) + 1)
    return s0 if s0 is not None and s0 == s1 else None


def correlation_for(table_doc, inoise):
    v = (inoise or {}).get("correlation_nm")
    if v is None:
        v = (table_doc.get("noise_model") or {}).get("correlation_nm", DEFAULT_CORRELATION_NM)
    return float(v)


# --------------------------------------------------------------------------- the two outputs
def model_terms(ng, m, wl, joins, ops=None, inoise=None):
    """{component name or None: [(N2 window, gain)]} for a model file through a correction. Raises GainError."""
    _, segs = layout(wl, joins)
    rec = (ng.get("models", {}).get(m["id"]) or {}).get("recorded")
    if rec is not None:                                  # a non-linear model with a recorded gain (no correction only)
        if ops is not None:
            raise nonlinear("non-linear model (its recorded gain holds for the scan as measured only)")
        terms = []
        for t in rec["terms"]:
            if not window_covered(wl, *t["window_nm"]):
                raise unavailable(f"{m['id']}: the N2 window {_fmt(t['window_nm'][0])}-{_fmt(t['window_nm'][1])} nm "
                                  f"is not covered by the scan")
            if window_segment(wl, segs, *t["window_nm"]) is None:
                raise unavailable(f"{m['id']}: the N2 window {_fmt(t['window_nm'][0])}-{_fmt(t['window_nm'][1])} nm "
                                  f"is not inside one detector segment")
            terms.append((tuple(t["window_nm"]), float(t["gain"])))
        return {None: terms}
    corr = correlation_for(ng, inoise)
    out = {}
    for name, k in model_kernels(m, wl):
        what = m["id"] if name is None else f"{m['id']} component {name}"
        terms = []
        for s, gain in segment_gains(k, wl, joins, ops, corr, what):
            win = next((w for w in ng["model_noise_windows_nm"] if window_segment(wl, segs, *w) == s), None)
            if win is None:
                a, b = segs[s]
                raise unavailable(f"{what}: no noise window in the detector segment {_fmt(wl[a])}-{_fmt(wl[b - 1])} nm")
            terms.append((tuple(win), gain))
        out[name] = terms
    return out


def band_gain(bd, band, wl, joins, ops=None, inoise=None):
    """(N2 window, gain_E_per_N2 x envelope factor) of a band with a readability block. Raises GainError."""
    r = bd["bands"][band]["readability"]
    _, segs = layout(wl, joins)
    k = band_kernel(bd, band, wl)
    sg = segment_gains(k, wl, joins, ops, correlation_for(bd, inoise), f"band {band}")
    if len(sg) != 1:
        raise unavailable(f"band {band}: its kernel spans more than one detector segment")
    s, gain = sg[0]
    lo, hi = r["noise_window_nm"]
    if not window_covered(wl, lo, hi):
        raise unavailable(f"band {band}: the N2 window {_fmt(lo)}-{_fmt(hi)} nm is not covered by the scan")
    if window_segment(wl, segs, lo, hi) != s:
        raise unavailable(f"band {band}: the N2 window {_fmt(lo)}-{_fmt(hi)} nm is not in the band's detector segment")
    over = ((inoise or {}).get("ch_envelope_factors") or {}).get(band)
    fac = float(over) if over is not None else float(r.get("envelope_factor", 1.0))
    return (float(lo), float(hi)), gain * fac


# --------------------------------------------------------------------------- linear statistics of the checks
def stat_kernel(st, wl, what):
    """The absorbance Kernel of a linear statistic {"kernel": {absorbance_clip, savgol, scale}, "windows": [[lo, hi,
    weight], ...]}: the sum of weight x mean of E over [lo, hi] (inclusive, on the grid), E = scale x SG(absorbance)."""
    wl = np.asarray(wl, float)
    kd = st["kernel"]
    beta = np.zeros(len(wl))
    for lo, hi, w in st["windows"]:
        a, z = _index(wl, float(lo)), _index(wl, float(hi))
        if a is None or z is None:
            raise unavailable(f"{what}: {_fmt(lo)}-{_fmt(hi)} nm is not on the scan's grid")
        if z < a:
            raise unavailable(f"{what}: empty window {_fmt(lo)}-{_fmt(hi)} nm")
        beta[a:z + 1] += float(kd["scale"]) * float(w) / (z - a + 1)
    k = Kernel(len(wl))
    k.w = _sg_adjoint(beta, wl, dict(kd["savgol"]), lambda o: o, len(wl), k, what)
    return k


def stat_term_gain(st, wl, joins, ops=None, inoise=None, correlation_nm=DEFAULT_CORRELATION_NM, win=(2000.0, 2100.0),
                   what="statistic"):
    """The SD per unit N2 (as measured, in the window `win`) of ONE detector term of a linear statistic: its part in
    the detector segment that holds the N2 window, through the correction (ops None = as measured). The window must be
    on the grid whole and inside one segment, and the statistic must have weight there; otherwise "not available on
    this instrument". Raises GainError."""
    wl = np.asarray(wl, float)
    _, segs = layout(wl, joins)
    lo, hi = float(win[0]), float(win[1])
    if not window_covered(wl, lo, hi):
        raise unavailable(f"{what}: the N2 window {_fmt(lo)}-{_fmt(hi)} nm is not covered by the scan")
    s = window_segment(wl, segs, lo, hi)
    if s is None:
        raise unavailable(f"{what}: the N2 window {_fmt(lo)}-{_fmt(hi)} nm is not inside one detector segment")
    k = stat_kernel(st, wl, what)
    corr = (inoise or {}).get("correlation_nm")
    corr = float(correlation_nm) if corr is None else float(corr)
    for seg, g in segment_gains(k, wl, joins, ops, corr, what):
        if seg == s:
            return g
    raise unavailable(f"{what}: no weight in the detector segment of the N2 window {_fmt(lo)}-{_fmt(hi)} nm")


def rss(terms, n2_of):
    """sqrt(sum (gain x N2(window))^2); None when an N2 is not finite (noise unknown)."""
    s = 0.0
    for (lo, hi), g in terms:
        v = n2_of(lo, hi)
        if v is None or not math.isfinite(v):
            return None
        s += (g * v) ** 2
    return math.sqrt(s)
