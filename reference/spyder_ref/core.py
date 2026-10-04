"""SPYDER Bone reference implementation of the frozen plug-in formats (numpy + stdlib only).

This is the executable specification that the app (Rust/TS) must reproduce to ~1e-9 on the golden
spectra. It deliberately avoids scipy/sklearn: every operator is written out so that a port is a
line-by-line translation. The prose specification is in docs/ (plug-in authoring guide) and reference/README.md.

Conventions
  * A spectrum is (wl, x): wl strictly increasing float64 nm, x float64 values.
  * Operators work on a batch X of shape (n_scans, n_points) sharing one grid wl.
  * Wavelength equality uses |a - b| <= WL_TOL.
  * Every operator returns (wl, X) and never modifies its input.
"""
from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path

import numpy as np

# common plug-in header names
MODEL_FORMAT = "spyder-bone/model"
TRANSFER_FORMAT = "spyder-bone/transfer"
GOLDEN_FORMAT = "spyder-bone/golden_spectra"
FORMAT_VERSION = 1          # schema version of the file kind (integer)
ENGINE_VERSION = (1, 1)     # op-set version this implementation provides; files declare engine_min
                            # 1.1 (Phase 0a A18): snv "blocks_nm" (per-block SNV); files using it declare engine_min "1.1"
WL_TOL = 1e-6

# ---- golden runner limits: owned by the ENGINE, not by the files (PLAN v2.2 section 4; Phase 0a task A10).
# A file may state a tighter tolerance, never a looser one; a looser one rejects the file.
GOLDEN_MAX_TOL = {
    "regression": {"abs": 1e-9, "rel": 1e-9},
    "consensus": {"abs": 1e-9, "rel": 1e-9},
    "classifier": {"abs": 1e-9, "rel": 1e-9},
    "rule": {"abs": 1e-9, "rel": 1e-9},
    "transfer": {"abs": 1e-9, "rel": 1e-9},
}
# expected outputs every golden case must carry, per kind (more keys are allowed if the engine knows them)
GOLDEN_REQUIRED = {
    "regression": ("value",),
    "consensus": ("value",),          # plus component:<name> for EVERY component (run_goldens)
    "classifier": ("scores", "label"),
    "rule": ("value", "label"),
    "transfer": ("transferred_at",),
}
GOLDEN_KNOWN = {
    "regression": {"value", "linear", "T2", "Q", "domain_ratio"},
    "consensus": {"value"},               # plus component:<name>
    "classifier": {"scores", "label", "probability", "T2", "Q", "domain_ratio"},
    "rule": {"value", "label"},              # plus "feature:<name>" for any declared rule feature
    "transfer": {"transferred_at"},
}
GOLDEN_NEEDS_FEATURE_CASE = ("regression", "classifier")   # at least one case carries the feature vector
GOLDEN_MIN_CASES = 3
SIDECAR_MAX_BYTES = 16 * 1024 * 1024    # dense DS matrices (~24 MB) are never bundled (PLAN section 4)


class SpyderError(ValueError):
    pass


# --------------------------------------------------------------------------- hashing
def spectrum_sha256(wl, x) -> str:
    """Content hash of one spectrum: domain tag + float64 little-endian wl bytes + value bytes."""
    h = hashlib.sha256(b"spyder-spectrum-v1\x00")
    h.update(np.ascontiguousarray(wl, dtype="<f8").tobytes())
    h.update(np.ascontiguousarray(x, dtype="<f8").tobytes())
    return h.hexdigest()


def file_sha256(path) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


# --------------------------------------------------------------------------- scan context
class ScanContext:
    """What the reader knows about a scan that preprocessing may need."""

    def __init__(self, splices_nm=(1000.0, 1800.0), instrument_class="asd.labspec4.std", serial=None):
        self.splices_nm = tuple(float(s) for s in splices_nm)
        self.instrument_class = instrument_class
        self.serial = serial

    @classmethod
    def from_json(cls, d):
        return cls(d.get("splices_nm", (1000.0, 1800.0)), d.get("instrument_class", "asd.labspec4.std"),
                   d.get("serial"))


def _idx_exact(wl, lam):
    i = np.flatnonzero(np.abs(wl - lam) <= WL_TOL)
    if len(i) != 1:
        raise SpyderError(f"wavelength {lam} nm not on grid")
    return int(i[0])


def _last_le(wl, lam):
    """Index of the last grid point with wl <= lam (+tol): the last channel of the lower detector."""
    i = np.flatnonzero(wl <= lam + WL_TOL)
    if len(i) == 0:
        raise SpyderError(f"join {lam} nm below grid")
    return int(i[-1])


def _segments(wl, joins):
    """Contiguous index ranges [a, b) of detector segments split after each join (wl <= join is lower)."""
    cuts = [0] + [_last_le(wl, j) + 1 for j in sorted(joins) if wl[0] <= j < wl[-1]] + [len(wl)]
    return [(a, b) for a, b in zip(cuts[:-1], cuts[1:]) if b > a]


# --------------------------------------------------------------------------- operators
def _line_fit_eval(Y, x_eval):
    """Closed-form least-squares line through columns of Y at x = 0..m-1, evaluated at x_eval (per row)."""
    m = Y.shape[1]
    x = np.arange(m, dtype=float)
    xb = x.mean()
    sxx = ((x - xb) ** 2).sum()
    yb = Y.mean(1)
    slope = ((Y - yb[:, None]) * (x - xb)).sum(1) / sxx
    return yb + slope * (x_eval - xb)


def op_splice_correct(wl, X, p, ctx):
    """Additive splice correction (legacy research implementation, generalised). RESERVED: not in the v1 op subset.

    joins: 'from_scan' (reader's per-file splice wavelengths) or [j1, j2] fixed.
    Lower join j1: fit a line to the first `fit_points_lower` SWIR1 channels (indices i1+1 ..),
      extrapolate one channel down (to the position of channel i1), and add (extrapolated - X[i1]) to
      every channel <= i1 (the whole VNIR segment).
    Upper join j2: fit a line to the last `fit_points_upper` SWIR1 channels (.. i2 inclusive),
      extrapolate one channel up (to the position of channel i2+1), and add (extrapolated - X[i2+1])
      to every channel > i2 (the whole SWIR2 segment).
    Legacy research setting: joins 1000/1800, fit_points_lower 5 (1001-1005), fit_points_upper 6 (1795-1800).
    """
    joins = ctx.splices_nm if p.get("joins", "from_scan") == "from_scan" else tuple(p["joins"])
    n_lo, n_hi = int(p.get("fit_points_lower", 5)), int(p.get("fit_points_upper", 6))
    j1, j2 = joins
    i1, i2 = _last_le(wl, j1), _last_le(wl, j2)
    Xc = np.array(X, dtype=float, copy=True)
    ext = _line_fit_eval(Xc[:, i1 + 1:i1 + 1 + n_lo], -1.0)
    Xc[:, :i1 + 1] += (ext - Xc[:, i1])[:, None]
    ext = _line_fit_eval(Xc[:, i2 - n_hi + 1:i2 + 1], float(n_hi))
    Xc[:, i2 + 1:] += (ext - Xc[:, i2 + 1])[:, None]
    return wl, Xc


def op_crop(wl, X, p, ctx):
    lo, hi = p["range_nm"]
    k = (wl >= lo - WL_TOL) & (wl <= hi + WL_TOL)
    if not k.any():
        raise SpyderError("crop leaves no points")
    return wl[k], X[:, k]


def op_absorbance(wl, X, p, ctx):
    """A = log10(1 / max(R, clip_min)). Written exactly so; -log10(R) differs in the last ulp."""
    c = float(p.get("clip_min", 1e-3))
    return wl, np.log10(1.0 / np.maximum(X, c))


# ---- Savitzky-Golay -------------------------------------------------------
def _sg_matrix(window, polyorder, deriv, delta, positions):
    """Rows: SG weights that evaluate the deriv-th derivative of the LS polynomial fitted to `window`
    consecutive samples (sample offsets t = 0..window-1) at the given sample offsets.
    Uses u = (t - h)/h in [-1, 1] for conditioning; d/dt = (1/h) d/du; physical scale 1/delta^deriv."""
    h = window // 2
    t = np.arange(window, dtype=float)
    u = (t - h) / h
    m = np.arange(polyorder + 1)
    V = u[:, None] ** m[None, :]                          # (window, p+1)
    pinv = np.linalg.pinv(V)                              # (p+1, window); app: QR solve
    ue = (np.asarray(positions, float) - h) / h
    D = np.zeros((len(ue), polyorder + 1))
    for k in range(deriv, polyorder + 1):
        D[:, k] = math.factorial(k) / math.factorial(k - deriv) * ue ** (k - deriv)
    return D @ pinv / (h ** deriv * delta ** deriv)


def sg_centre_weights(window, polyorder, deriv, delta=1.0):
    return _sg_matrix(window, polyorder, deriv, delta, [window // 2])[0]


def op_savgol(wl, X, p, ctx):
    """Savitzky-Golay smoothing/derivative with exact edge handling.

    mode: 'interp'     scipy.signal.savgol_filter default: interior by convolution; first/last h
                       outputs from the polynomial fitted to the first/last `window` samples.
          'mirror' | 'nearest' | 'constant' | 'wrap'   scipy padding modes (constant: cval 0).
          'zero_edges' interior only; first/last h outputs set to 0 (Ryder/Unscrambler rebuild,
                       research implementation).
          'valid'      interior only; first/last h points removed (grid shrinks).
    delta: sample spacing passed to the derivative scaling (scipy `delta`); the frozen file stores
           the number that training used (1.0 for all current models on the 1-nm grid).
    """
    W, P, d = int(p["window"]), int(p["polyorder"]), int(p.get("deriv", 0))
    delta, mode = float(p.get("delta", 1.0)), p.get("mode", "interp")
    if W % 2 != 1 or W <= P:
        raise SpyderError("SG window must be odd and > polyorder")
    n = X.shape[1]
    if W > n:
        raise SpyderError("SG window longer than spectrum")
    h = W // 2
    c = sg_centre_weights(W, P, d, delta)
    if mode in ("interp", "zero_edges", "valid"):
        # interior: out[j] = sum_k c[k] * x[j - h + k],  j = h .. n-h-1
        Y = np.zeros_like(X, dtype=float)
        for k in range(W):
            Y[:, h:n - h] += c[k] * X[:, k:n - 2 * h + k]
        if mode == "interp":
            EL = _sg_matrix(W, P, d, delta, np.arange(h))              # evaluated at t = 0..h-1
            ER = _sg_matrix(W, P, d, delta, np.arange(W - h, W))       # evaluated at t = W-h..W-1
            Y[:, :h] = X[:, :W] @ EL.T
            Y[:, n - h:] = X[:, n - W:] @ ER.T
        elif mode == "valid":
            return wl[h:n - h], Y[:, h:n - h]
        return wl, Y
    pad = {"mirror": "reflect", "nearest": "edge", "constant": "constant", "wrap": "wrap"}.get(mode)
    if pad is None:
        raise SpyderError(f"unknown SG mode {mode}")
    # scipy 'mirror' == numpy 'reflect' (edge sample not repeated); scipy 'nearest' == numpy 'edge'
    Xp = np.pad(X, ((0, 0), (h, h)), mode=pad)
    Y = np.zeros_like(X, dtype=float)
    for k in range(W):
        Y += c[k] * Xp[:, k:k + n]
    return wl, Y


def _snv_rows(X, ddof):
    m = X.mean(1, keepdims=True)
    s = X.std(1, ddof=ddof, keepdims=True)
    if np.any(~np.isfinite(s)) or np.any(s == 0):
        raise SpyderError("SNV of a flat block")
    return (X - m) / s


def _block_mask(wl, lo, hi):
    return (wl >= lo - WL_TOL) & (wl <= hi + WL_TOL)


def op_snv(wl, X, p, ctx):
    """Standard normal variate, row-wise (x - mean) / std, std = sqrt(sum((x - mean)^2) / (n - ddof)).

    Block form (op set 1.1; the only form in the v1 subset; Phase 0a A18):
      {"op": "snv", "ddof": 0, "blocks_nm": [[lo1, hi1], [lo2, hi2], ...]}
      * block k = the grid points with lo_k - WL_TOL <= wl <= hi_k + WL_TOL; blocks sorted, non-overlapping, >= 3 points.
      * each scan is normalised SEPARATELY inside each block with that block's own mean and population std (ddof 0).
      * the output grid is the concatenation of the block points in wavelength order; points outside every block
        are dropped (so model features must lie inside a block). One block = crop to it, then whole-grid SNV.
      * a block holding grid points on both sides of one of THIS scan's detector joins (reader's per-file joins;
        lower detector = wl <= join) is an error for that scan: the block would mix two detectors with different
        gains (A16a, A16b). The app shows the model as "not available for this scan", never a guess.
      * std 0 (a flat block) is an error.
      The statistics are part of the model: check_model() requires the preceding Savitzky-Golay step to have run on
      a grid that contains every block plus h = window // 2 channels on each side, so no SG edge value (whose
      definition depends on the SG mode) can enter a block (the 04 section 5 "edge trap").
    Legacy form (op set 1.0, research files only, NOT in the v1 subset): no blocks_nm = all points of the current grid.
    """
    ddof = int(p.get("ddof", 0))
    blocks = p.get("blocks_nm")
    if blocks is None:
        return wl, _snv_rows(X, ddof)
    _check_snv_blocks(blocks)
    idx, parts = [], []
    for lo, hi in blocks:
        k = np.flatnonzero(_block_mask(wl, lo, hi))
        if len(k) < 3:
            raise SpyderError(f"SNV block {lo}-{hi} nm has {len(k)} grid points (needs >= 3)")
        for j in ctx.splices_nm:          # lower detector = wl <= join (as _segments)
            if np.any(wl[k] <= j + WL_TOL) and np.any(wl[k] > j + WL_TOL):
                raise SpyderError(f"SNV block {lo}-{hi} nm spans this scan's detector join at {j} nm")
        idx.append(k); parts.append(_snv_rows(X[:, k], ddof))
    return wl[np.concatenate(idx)], np.hstack(parts)


def _check_snv_blocks(blocks):
    if not isinstance(blocks, list) or not blocks:
        raise SpyderError("snv blocks_nm must be a non-empty list of [lo, hi]")
    prev = -math.inf
    for b in blocks:
        if not (isinstance(b, (list, tuple)) and len(b) == 2 and all(isinstance(v, (int, float)) and not isinstance(v, bool)
                                                                    and math.isfinite(v) for v in b)):
            raise SpyderError(f"snv block {b!r} is not [lo, hi]")
        lo, hi = float(b[0]), float(b[1])
        if not lo < hi:
            raise SpyderError(f"snv block {lo}-{hi}: lo must be < hi")
        if lo <= prev + WL_TOL:
            raise SpyderError("snv blocks must be sorted and must not overlap")
        prev = hi


def op_msc(wl, X, p, ctx):
    """MSC against a frozen reference (legacy research implementation). RESERVED: not in the v1 op subset."""
    ref = np.asarray(p["reference"], float)
    if len(ref) != X.shape[1]:
        raise SpyderError("MSC reference length mismatch")
    r = ref - ref.mean()
    xm = X.mean(1, keepdims=True)
    b = ((X - xm) @ r / (r @ r))[:, None]
    a = xm - b * ref.mean()
    return wl, (X - a) / b


def op_detrend(wl, X, p, ctx):
    """Subtract each spectrum's LS polynomial (degree) in u = (wl - mid)/half_range."""
    deg = int(p.get("degree", 1))
    u = (wl - 0.5 * (wl[0] + wl[-1])) / (0.5 * (wl[-1] - wl[0]))
    V = u[:, None] ** np.arange(deg + 1)[None, :]
    coef = np.linalg.pinv(V) @ X.T
    return wl, X - (V @ coef).T


def op_resample(wl, X, p, ctx):
    """Piecewise-linear interpolation onto a target grid; no extrapolation."""
    g = p["to_grid"]
    tgt = np.asarray(g["wavelengths_nm"], float) if "wavelengths_nm" in g else \
        g["start_nm"] + g["step_nm"] * np.arange(int(g["n"]))
    if tgt[0] < wl[0] - WL_TOL or tgt[-1] > wl[-1] + WL_TOL:
        raise SpyderError("resample would extrapolate")
    return tgt, np.vstack([np.interp(tgt, wl, row) for row in X])


def _fwhm_at(spec, lam):
    if isinstance(spec, (int, float)):
        return np.full_like(lam, float(spec))
    if spec.get("type") == "piecewise_linear":
        return np.interp(lam, spec["knots_nm"], spec["values_nm"])   # constant beyond the end knots
    raise SpyderError("unknown fwhm spec")


def op_gaussian_blur(wl, X, p, ctx):
    """Gaussian blur (resolution degradation) inside each detector segment, kernel centred on each OUTPUT channel.

    Width, one of:
      sigma_nm: number, or list with one sigma per segment (VNIR, SWIR1, SWIR2)   [transfer agent's form]
      fwhm_nm:  number or piecewise_linear in wavelength; sigma = fwhm / (2 sqrt(2 ln 2))
    radius r_i (channels): 'ceil'  -> ceil(truncate * sigma_i / step)            [transfer agent, 08_transfer.md]
                           'round' -> floor(truncate * sigma_i / step + 0.5)     [scipy gaussian_filter1d; default]
    weights w_j = exp(-0.5 ((wl_j - wl_i)/sigma_i)^2) over |j - i| <= r_i and j in the same segment
    ('segments': 'from_scan' = reader's splice joins, wl <= join is the lower segment; 'none' = whole grid);
    out_i = (sum_j w_j x_j) / (sum_j w_j)   (renormalised at segment ends; taps never cross a join).
    sigma <= 0 leaves the channel unchanged. Requires a uniform grid.
    """
    step = float(np.median(np.diff(wl)))
    if np.max(np.abs(np.diff(wl) - step)) > 1e-6:
        raise SpyderError("gaussian_blur needs a uniform grid")
    segs = _segments(wl, ctx.splices_nm) if p.get("segments", "from_scan") == "from_scan" else [(0, len(wl))]
    if "sigma_nm" in p:
        sv = p["sigma_nm"]
        if isinstance(sv, (int, float)):
            sig = np.full(len(wl), float(sv))
        else:
            if len(sv) != len(segs):
                raise SpyderError(f"gaussian_blur has {len(sv)} sigmas, scan has {len(segs)} segments")
            sig = np.zeros(len(wl))
            for (a, b), v in zip(segs, sv):
                sig[a:b] = float(v)
    else:
        sig = _fwhm_at(p["fwhm_nm"], wl) / (2.0 * math.sqrt(2.0 * math.log(2.0)))
    trunc = float(p.get("truncate", 4.0))
    rule = p.get("radius", "round")
    Y = np.array(X, dtype=float, copy=True)
    for a, b in segs:
        for i in range(a, b):
            if sig[i] <= 0:
                continue
            r = int(math.ceil(trunc * sig[i] / step)) if rule == "ceil" else int(math.floor(trunc * sig[i] / step + 0.5))
            lo, hi = max(a, i - r), min(b, i + r + 1)
            w = np.exp(-0.5 * ((wl[lo:hi] - wl[i]) / sig[i]) ** 2)
            Y[:, i] = (X[:, lo:hi] @ w) / w.sum()
    return wl, Y


def _per_segment(v, segs, n, name):
    if isinstance(v, (int, float)):
        return np.full(n, float(v))
    if len(v) != len(segs):
        raise SpyderError(f"{name} has {len(v)} values, scan has {len(segs)} segments")
    out = np.zeros(n)
    for (a, b), x in zip(segs, v):
        out[a:b] = float(x)
    return out


def op_absorbance_affine(wl, X, p, ctx):
    """Affine map in absorbance, returning reflectance (transfer agent's step 2, 08_transfer.md section 7):
    A = log10(1 / max(R, clip_min)); A' = gain * A + offset; R' = 10 ** (-A').
    gain / offset: number, or one value per detector segment (joins from the scan)."""
    segs = _segments(wl, ctx.splices_nm)
    g = _per_segment(p["gain"], segs, len(wl), "gain")
    o = _per_segment(p.get("offset", 0.0), segs, len(wl), "offset")
    A = np.log10(1.0 / np.maximum(X, float(p["clip_min"])))
    return wl, 10.0 ** (-(g * A + o))


def op_affine_reflectance(wl, X, p, ctx):
    """Per detector segment R' = a_s * R + b_s (segments split after each join, wl <= join is lower;
    'from_scan' joins). p['segments'] = [{'a':..,'b':..}, ...] in wavelength order (VNIR, SWIR1, SWIR2)."""
    joins = ctx.splices_nm if p.get("joins", "from_scan") == "from_scan" else p["joins"]
    segs = _segments(wl, joins)
    co = p["segments"]
    if len(co) != len(segs):
        raise SpyderError(f"affine_reflectance has {len(co)} segment pairs, scan has {len(segs)} segments")
    Y = np.array(X, dtype=float, copy=True)
    for (a0, b0), c in zip(segs, co):
        Y[:, a0:b0] = float(c["a"]) * X[:, a0:b0] + float(c["b"])
    return wl, Y


def op_pds_banded(wl, X, p, ctx):
    """Piecewise direct standardisation, dasp calibration_transfer.apply_pds (lines 248-313) layout:
    B is (n_points, 2k+1); output_i = sum_j X[i-k+j] * B[i, j] over in-range j, + offset_i."""
    tw = np.asarray(p["wavelengths_nm"], float)
    if len(tw) != len(wl) or np.max(np.abs(tw - wl)) > WL_TOL:
        raise SpyderError("PDS grid mismatch")
    B = np.asarray(p["B"], float)
    k = B.shape[1] // 2
    off = np.asarray(p.get("offset", np.zeros(len(wl))), float)
    n = X.shape[1]
    Y = np.empty_like(X, dtype=float)
    for i in range(n):
        s, e = max(0, i - k), min(n, i + k + 1)
        o = s - (i - k)
        Y[:, i] = X[:, s:e] @ B[i, o:o + (e - s)] + off[i]
    return wl, Y


OPS = {
    "splice_correct": op_splice_correct, "crop": op_crop, "absorbance": op_absorbance,
    "savgol": op_savgol, "snv": op_snv, "msc": op_msc, "detrend": op_detrend,
    "resample": op_resample, "gaussian_blur": op_gaussian_blur, "pds_banded": op_pds_banded,
    "affine_reflectance": op_affine_reflectance, "absorbance_affine": op_absorbance_affine,
}


def run_chain(chain, wl, X, ctx):
    wl = np.asarray(wl, float)
    X = np.atleast_2d(np.asarray(X, float))
    for step in chain:
        fn = OPS.get(step["op"])
        if fn is None:
            raise SpyderError(f"unknown op {step['op']}")
        wl, X = fn(wl, X, step, ctx)
    return wl, X


# --------------------------------------------------------------------------- model files
def package_path(base, name, what="file"):
    """Resolve a file name given inside a plug-in against the plug-in's own folder, and refuse anything that
    could leave it: absolute paths, drive letters, backslashes, '..' segments, and symlinks resolving outside."""
    if not isinstance(name, str) or not name or "\\" in name or ":" in name:
        raise SpyderError(f"{what} path {name!r} is not a plain relative path")
    p = Path(name)
    if p.is_absolute() or p.anchor or any(part in ("..", ".", "") for part in name.split("/")):
        raise SpyderError(f"{what} path {name!r} must stay inside the package folder")
    root = Path(base).resolve()
    f = (root / p).resolve()
    if f != root and root not in f.parents:
        raise SpyderError(f"{what} path {name!r} resolves outside the package folder")
    if not f.is_file():
        raise SpyderError(f"{what} {name!r} not found in the package folder")
    return f


def _resolve_sidecars(o, base):
    """Replace {"npy": "name.npy", "sha256": "..."} by the array (little-endian float64 .npy, no pickles).
    The path must resolve inside the package folder, the file must be at most SIDECAR_MAX_BYTES, match its
    sha256, hold a 1-D or 2-D little-endian float64 array, and contain only finite values."""
    if isinstance(o, dict):
        if set(o) == {"npy", "sha256"}:
            if not str(o["npy"]).endswith(".npy"):
                raise SpyderError(f"sidecar {o['npy']!r} is not a .npy file")
            f = package_path(base, o["npy"], "sidecar")
            if f.stat().st_size > SIDECAR_MAX_BYTES:
                raise SpyderError(f"sidecar {o['npy']} is {f.stat().st_size} bytes; limit {SIDECAR_MAX_BYTES}")
            if file_sha256(f) != o["sha256"]:
                raise SpyderError(f"sidecar {o['npy']} sha256 mismatch")
            a = np.load(f, allow_pickle=False)
            if a.dtype != np.dtype("<f8"):
                raise SpyderError("sidecar must be little-endian float64")
            if a.ndim not in (1, 2) or a.size == 0:
                raise SpyderError(f"sidecar {o['npy']} must be a non-empty vector or matrix")
            if not np.all(np.isfinite(a)):
                raise SpyderError(f"sidecar {o['npy']} holds non-finite values")
            return a
        return {k: _resolve_sidecars(v, base) for k, v in o.items()}
    if isinstance(o, list):
        return [_resolve_sidecars(v, base) for v in o]
    return o


def _no_const(c):
    raise SpyderError(f"{c} in JSON")


def _check_finite(o, where="$"):
    """Every JSON number must be finite. json accepts e.g. 1e999 and turns it into inf; NaN/Infinity literals are
    already refused by parse_constant."""
    if isinstance(o, float):
        if not math.isfinite(o):
            raise SpyderError(f"non-finite number at {where}")
    elif isinstance(o, dict):
        for k, v in o.items():
            _check_finite(v, f"{where}.{k}")
    elif isinstance(o, list):
        for i, v in enumerate(o):
            _check_finite(v, f"{where}[{i}]")


def load_json(path):
    with open(path, "r", encoding="utf-8") as f:
        doc = json.load(f, parse_constant=_no_const)
    _check_finite(doc)
    return _resolve_sidecars(doc, Path(path).parent)


def _check_header(d, fmt):
    if d.get("format") != fmt:
        raise SpyderError(f"not a {fmt} file")
    if d.get("format_version") != FORMAT_VERSION:
        raise SpyderError("unsupported format_version")
    em = tuple(int(x) for x in str(d.get("engine_min", "1.0")).split("."))
    if em > ENGINE_VERSION:
        raise SpyderError(f"needs a newer engine (op set {d['engine_min']})")


def _check_vec(v, n, what):
    a = np.asarray(v, dtype=float)
    if a.ndim != 1 or len(a) != n:
        raise SpyderError(f"{what}: expected a vector of {n} numbers, got shape {a.shape}")
    if not np.all(np.isfinite(a)):
        raise SpyderError(f"{what}: non-finite value")
    return a


def check_model(m):
    """Structural checks the app must also make before accepting a model file."""
    _check_header(m, MODEL_FORMAT)
    for k in ("id", "version", "title", "kind", "instrument", "input", "preprocessing", "features", "golden"):
        if k not in m:
            raise SpyderError(f"missing {k}")
    p = len(m["features"]["wavelengths_nm"])
    _vec = lambda v, n, what: _check_vec(v, n, what)
    if m["kind"] == "consensus" and p != 0:
        raise SpyderError("a consensus file declares no top-level features (each component carries its own)")
    if m["kind"] == "regression":
        L = m["regression"]
        if len(L["coefficients"]) != p or len(L.get("x_center", [0] * p)) != p:
            raise SpyderError("coefficient length mismatch")
        _vec(L["coefficients"], p, "regression.coefficients")
        if L.get("x_center") is not None:
            _vec(L["x_center"], p, "regression.x_center")
        if p == 0:
            raise SpyderError("regression with no features")
    elif m["kind"] == "classifier":
        for s in m["classifier"]["scores"]:
            if len(s["coefficients"]) != p:
                raise SpyderError("score coefficient length mismatch")
            _vec(s["coefficients"], p, "classifier score coefficients")
    elif m["kind"] == "consensus":
        _check_consensus(m)
    elif m["kind"] != "rule":
        raise SpyderError(f"unknown kind {m['kind']}")
    d = m.get("domain") or {}
    if d.get("method") == "pls_t2_q":
        if d.get("x_center") is not None:
            _vec(d["x_center"], p, "domain.x_center")
        for c in d["components"]:
            _vec(c["weights_r"], p, "domain weights_r")
            _vec(c["loadings_p"], p, "domain loadings_p")
    for op in m["preprocessing"]:
        if op["op"] not in OPS:
            raise SpyderError(f"unknown op {op['op']}")
    _check_snv_chain(m)
    return m


_SHA_HEX = set("0123456789abcdef")


def _check_consensus(m):
    """Structural checks for kind 'consensus' (engine 1.1; Phase 0b): the median of an odd number (>= 3) of linear
    components, each the exact fold of a shipped model into one linear functional of the processed spectrum.

      "consensus": {"combine": "median",
                    "components": [{"name": "wc2045", "id": "...", "version": "1.0.0", "sha256": "<file sha256>",
                                    "feature": {"type": "linear", "wavelengths_nm": [...], "weights": [...],
                                                "offset": 0.0}}, ...]}
    value = median over the components of offset_k + sum_i w_ki * Z(lambda_ki), Z = the file's processed spectrum.
    No expression language: the combine rule is fixed by name."""
    em = tuple(int(x) for x in str(m.get("engine_min", "1.0")).split("."))
    if em < (1, 1):
        raise SpyderError("kind consensus needs engine_min 1.1")
    c = m.get("consensus")
    if not isinstance(c, dict) or c.get("combine") != "median":
        raise SpyderError("consensus.combine must be 'median'")
    comps = c.get("components")
    if not isinstance(comps, list) or len(comps) < 3 or len(comps) % 2 != 1:
        raise SpyderError("consensus needs an odd number (>= 3) of components")
    names = set()
    for k in comps:
        for f in ("name", "id", "version", "sha256", "feature"):
            if f not in k:
                raise SpyderError(f"consensus component missing {f}")
        if k["name"] in names or not isinstance(k["name"], str) or not k["name"]:
            raise SpyderError(f"consensus component name {k['name']!r} empty or repeated")
        names.add(k["name"])
        if not (isinstance(k["sha256"], str) and len(k["sha256"]) == 64 and set(k["sha256"]) <= _SHA_HEX):
            raise SpyderError(f"consensus component {k['name']}: sha256 must be 64 lower-case hex characters")
        f = k["feature"]
        if f.get("type") != "linear":
            raise SpyderError(f"consensus component {k['name']}: feature type must be 'linear'")
        n = len(f.get("wavelengths_nm", []))
        if n == 0:
            raise SpyderError(f"consensus component {k['name']}: no wavelengths")
        _check_vec(f["wavelengths_nm"], n, f"component {k['name']} wavelengths_nm")
        _check_vec(f["weights"], n, f"component {k['name']} weights")
        if isinstance(f.get("offset", 0.0), bool) or not math.isfinite(float(f.get("offset", 0.0))):
            raise SpyderError(f"consensus component {k['name']}: offset must be a finite number")


def _model_grid(m):
    g = m["instrument"].get("grid") or {}
    if {"start_nm", "step_nm", "n"} <= set(g):
        return float(g["start_nm"]), float(g["start_nm"]) + float(g["step_nm"]) * (int(g["n"]) - 1), float(g["step_nm"])
    lo, hi = m["input"].get("range_nm", (350.0, 2500.0))
    return float(lo), float(hi), 1.0


def _check_snv_chain(m):
    """Structural rules for block SNV (op set 1.1), checked on load (Phase 0a A18):
      1. blocks_nm well formed (sorted, non-overlapping, lo < hi); a file using it declares engine_min >= 1.1;
      2. no block holds grid points on both sides of a training detector join (instrument.training_splices_nm);
      3. the most recent Savitzky-Golay step before the SNV ran on a grid extending >= h = window // 2 channels beyond
         every block on both sides (no SG edge value, whose meaning depends on the SG mode, can enter a block);
      4. block SNV is the last preprocessing step, and every feature wavelength lies inside a block."""
    ops = m["preprocessing"]
    lo, hi, step = _model_grid(m)
    last_sg = None
    for i, op in enumerate(ops):
        if op["op"] == "crop":
            lo, hi = max(lo, float(op["range_nm"][0])), min(hi, float(op["range_nm"][1]))
        elif op["op"] == "savgol":
            h = int(op["window"]) // 2
            last_sg = (lo, hi, h * step)
            if op.get("mode") == "valid":
                lo, hi = lo + h * step, hi - h * step
        elif op["op"] == "resample":
            last_sg = None
            g = op["to_grid"]
            if "wavelengths_nm" in g:
                lo, hi = float(g["wavelengths_nm"][0]), float(g["wavelengths_nm"][-1])
            else:
                lo, hi = float(g["start_nm"]), float(g["start_nm"]) + float(g["step_nm"]) * (int(g["n"]) - 1)
        elif op["op"] == "snv" and op.get("blocks_nm") is not None:
            blocks = op["blocks_nm"]
            _check_snv_blocks(blocks)
            em = tuple(int(x) for x in str(m.get("engine_min", "1.0")).split("."))
            if em < (1, 1):
                raise SpyderError("snv blocks_nm needs engine_min 1.1 (an older engine would ignore the blocks)")
            if i != len(ops) - 1:
                raise SpyderError("block SNV must be the last preprocessing step")
            for b0, b1 in blocks:
                if b0 < lo - WL_TOL or b1 > hi + WL_TOL:
                    raise SpyderError(f"snv block {b0}-{b1} nm lies outside the grid {lo}-{hi} nm")
                if last_sg is not None:
                    g0, g1, hh = last_sg
                    if b0 - hh < g0 - WL_TOL or b1 + hh > g1 + WL_TOL:
                        raise SpyderError(f"snv block {b0}-{b1} nm is within the SG half-window ({hh:g} nm) of the "
                                          f"grid edge {g0}-{g1} nm the SG ran on: SG edge values would enter the block")
                for j in m["instrument"].get("training_splices_nm", []):
                    if b0 <= j + WL_TOL and b1 > j + WL_TOL:
                        raise SpyderError(f"snv block {b0}-{b1} nm spans the detector join at {j} nm")
            for lam in m["features"]["wavelengths_nm"]:
                if not any(b0 - WL_TOL <= lam <= b1 + WL_TOL for b0, b1 in blocks):
                    raise SpyderError(f"feature {lam} nm lies outside every SNV block")


# ---- the v1 app op subset (PLAN section 4): everything else is "unsupported operator", never a guess
V1_OPS = {"absorbance", "savgol", "crop", "gaussian_blur", "absorbance_affine", "snv"}


def v1_problems(doc):
    """Why the v1 app would refuse this model or transfer file's chain (empty list = it runs in v1). SNV is in v1 only
    in its block form with ddof 0 (A18); the whole-grid legacy SNV, MSC, detrend, resample, splice_correct,
    pds_banded and affine_reflectance stay reserved."""
    if doc.get("format") == MODEL_FORMAT:
        ops = doc["preprocessing"]
    else:
        ops = doc["operator"] if isinstance(doc.get("operator"), list) else [doc.get("operator")]
    out = []
    for op in ops:
        if op["op"] not in V1_OPS:
            out.append(f"unsupported operator {op['op']}")
        elif op["op"] == "snv" and (op.get("blocks_nm") is None or int(op.get("ddof", 0)) != 0):
            out.append("snv in v1 needs blocks_nm and ddof 0")
        elif op["op"] == "absorbance" and "clip_min" not in op:
            out.append("absorbance needs a declared clip_min")
    return out


def model_features(m, wl, X, ctx):
    wlp, Z = run_chain(m["preprocessing"], wl, X, ctx)
    idx = [_idx_exact(wlp, lam) for lam in m["features"]["wavelengths_nm"]]
    return np.asarray(m["features"]["wavelengths_nm"], float), Z[:, idx], (wlp, Z)


def _linear(F, center, coef, offset):
    c = np.zeros(F.shape[1]) if center is None else np.asarray(center, float)
    return offset + (F - c) @ np.asarray(coef, float)


def _inverse_y(v, yt):
    t = (yt or {}).get("type", "none")
    if t == "none":
        return v
    if t == "log10":
        return 10.0 ** v - float(yt.get("offset", 0.0))
    if t == "ln":
        return np.exp(v) - float(yt.get("offset", 0.0))
    if t == "sqrt":
        return np.square(v) - float(yt.get("offset", 0.0))
    raise SpyderError(f"unknown y transform {t}")


def _domain(m, F):
    d = m.get("domain")
    if not d or d.get("method") != "pls_t2_q":
        return {}
    c = np.asarray(d.get("x_center", m.get("regression", m.get("classifier", {})).get("x_center")), float)
    E = F - c
    Rw = np.array([comp["weights_r"] for comp in d["components"]], float).T      # (p, A)
    Pl = np.array([comp["loadings_p"] for comp in d["components"]], float).T
    sv = np.array([comp["score_var"] for comp in d["components"]], float)
    T = E @ Rw
    t2 = (T ** 2 / sv).sum(1)
    q = ((E - T @ Pl.T) ** 2).sum(1)
    ratio = np.maximum(t2 / d["t2_limit"], q / d["q_limit"])
    return {"T2": t2, "Q": q, "domain_ratio": ratio}


# ---- rule models ----------------------------------------------------------
def _rule_feature(spec, wl, z):
    t = spec["type"]
    sel = lambda lo, hi: (wl >= lo - WL_TOL) & (wl <= hi + WL_TOL)
    if t == "mean":
        return z[:, sel(*spec["range_nm"])].mean(1)
    if t == "value":
        return z[:, _idx_exact(wl, spec["wavelength_nm"])]
    if t == "linear":
        idx = [_idx_exact(wl, lam) for lam in spec["wavelengths_nm"]]
        return z[:, idx] @ np.asarray(spec["weights"], float) + float(spec.get("offset", 0.0))
    if t == "band_depth":
        kl, kr, kc = sel(*spec["left_nm"]), sel(*spec["right_nm"]), sel(*spec["centre_nm"])
        lL, lR = wl[kl].mean(), wl[kr].mean()
        cL, cR = z[:, kl].mean(1), z[:, kr].mean(1)
        cont = cL[:, None] + (cR - cL)[:, None] * (wl[kc] - lL)[None, :] / (lR - lL)
        if spec.get("form", "difference") == "difference":     # absorbance-like: band above continuum
            return (z[:, kc] - cont).mean(1)
        return 1.0 - (z[:, kc] / cont).mean(1)                 # reflectance-like: 1 - R/continuum
    raise SpyderError(f"unknown rule feature {t}")


def _expr(e, env):
    if isinstance(e, (int, float)):
        return float(e)
    if "ref" in e:
        return env[e["ref"]]
    a = [_expr(x, env) for x in e["args"]]
    op = e["op"]
    if op == "add": return a[0] + a[1]
    if op == "sub": return a[0] - a[1]
    if op == "mul": return a[0] * a[1]
    if op == "div": return a[0] / a[1]
    if op == "neg": return -a[0]
    if op == "abs": return np.abs(a[0])
    if op == "min": return np.minimum(a[0], a[1])
    if op == "max": return np.maximum(a[0], a[1])
    raise SpyderError(f"unknown expr op {op}")


def _levels(v, levels):
    out = []
    for x in np.atleast_1d(v):
        lab = levels[-1]["label"]
        for lv in levels[:-1]:
            if x < lv["below"]:
                lab = lv["label"]
                break
        out.append(lab)
    return out


def predict(m, wl, X, ctx):
    """Returns dict of arrays (one entry per scan)."""
    if m["kind"] == "consensus":
        wlp, Z = run_chain(m["preprocessing"], wl, X, ctx)
        out, cols = {}, []
        for k in m["consensus"]["components"]:
            v = _rule_feature(k["feature"], wlp, Z)
            out[f"component:{k['name']}"] = v
            cols.append(v)
        out["value"] = np.median(np.column_stack(cols), axis=1)     # odd count: the middle value itself, exactly
        return out
    wl_f, F, (wlp, Z) = model_features(m, wl, X, ctx)
    out = {}
    if m["kind"] == "regression":
        L = m["regression"]
        lin = _linear(F, L.get("x_center"), L["coefficients"], L["offset"])
        val = _inverse_y(lin, m.get("output", {}).get("y_transform"))
        clip = m.get("output", {}).get("clip", {})
        if clip.get("min") is not None:
            val = np.maximum(val, clip["min"])
        if clip.get("max") is not None:
            val = np.minimum(val, clip["max"])
        out.update(linear=lin, value=val)
    elif m["kind"] == "classifier":
        C = m["classifier"]
        S = np.column_stack([_linear(F, C.get("x_center"), s["coefficients"], s["offset"]) for s in C["scores"]])
        dec = C["decision"]
        if dec["type"] == "argmax":
            lab = [dec["labels"][i] for i in S.argmax(1)]
        elif dec["type"] == "cuts":                      # ordinal cut points on one score
            j = [s["name"] for s in C["scores"]].index(dec["score"])
            lab = _levels(S[:, j], [{"label": l, "below": c} for l, c in zip(dec["labels"], dec["cuts"])]
                          + [{"label": dec["labels"][-1]}])
        else:
            raise SpyderError("unknown decision")
        out.update(scores=S, label=lab)
        if C.get("probability") == "logistic" and S.shape[1] == 1:
            out["probability"] = 1.0 / (1.0 + np.exp(-S[:, 0]))
        elif C.get("probability") == "softmax":
            E = np.exp(S - S.max(1, keepdims=True))
            out["probability"] = E / E.sum(1, keepdims=True)
    else:
        R = m["rule"]
        env = {k: _rule_feature(v, wlp, Z) for k, v in R["features"].items()}
        v = _expr(R["score"], env)
        out.update(value=v, label=_levels(v, R["levels"]), **{f"feature:{k}": x for k, x in env.items()})
    out.update(_domain(m, F))
    return out


# --------------------------------------------------------------------------- transfers
def check_transfer(t):
    _check_header(t, TRANSFER_FORMAT)
    for k in ("id", "version", "source_class", "target_class", "operator", "golden"):
        if k not in t:
            raise SpyderError(f"missing {k}")
    return t


def pick_transfer(model, ctx, transfers):
    """Same class (or listed in also_valid_for) -> 'none'; else the most specific active transfer
    source_class -> trained_on_class (a matching source_serial beats a class-wide one), newest version;
    else None (the app still predicts and says so, gently). No chaining in format 1."""
    target = model["instrument"]["trained_on_class"]
    if ctx.instrument_class == target or ctx.instrument_class in model["instrument"].get("also_valid_for", []):
        return "none"
    cands = [t for t in transfers if t["source_class"] == ctx.instrument_class and t["target_class"] == target
             and t.get("status", "active") == "active" and t.get("source_serial") in (None, ctx.serial)]
    if not cands:
        return None
    return max(cands, key=lambda t: (t.get("source_serial") is not None,
                                     tuple(int(x) for x in t["version"].split("."))))


def apply_transfer(t, wl, X, ctx):
    ops = t["operator"] if isinstance(t["operator"], list) else [t["operator"]]
    return run_chain(ops, wl, X, ctx)



# --------------------------------------------------------------------------- golden tests
def load_golden_spectra(path):
    g = load_json(path)
    if g.get("format") != GOLDEN_FORMAT:
        raise SpyderError("not a golden spectra file")
    out = {}
    for s in g["spectra"]:
        x = np.asarray(s["values"], float)
        if x.ndim != 1 or x.size == 0 or not np.all(np.isfinite(x)):
            raise SpyderError(f"golden spectrum {s.get('name')} must be a non-empty finite vector")
        wl = s["wl_start_nm"] + s["wl_step_nm"] * np.arange(len(x))
        h = spectrum_sha256(wl, x)
        if h != s["sha256"]:
            raise SpyderError(f"golden spectrum {s['name']} hash mismatch")
        out[h] = (wl, x, s)
    return out


def golden_kind(doc):
    if doc.get("format") == MODEL_FORMAT:
        if doc.get("kind") not in GOLDEN_MAX_TOL:
            raise SpyderError(f"unknown kind {doc.get('kind')}")
        return doc["kind"]
    if doc.get("format") == TRANSFER_FORMAT:
        return "transfer"
    raise SpyderError("not a model or transfer file")


def golden_tolerance(doc):
    """The tolerance a file's goldens are checked at. The engine owns the maximum per kind; a file may only
    tighten it. A missing tolerance means the engine maximum; a looser one rejects the file."""
    kind = golden_kind(doc)
    mx = GOLDEN_MAX_TOL[kind]
    tol = doc["golden"].get("tolerance", mx)
    out = {}
    for k in ("abs", "rel"):
        v = tol.get(k) if isinstance(tol, dict) else None
        if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or v < 0:
            raise SpyderError(f"golden tolerance '{k}' must be a finite number >= 0")
        if v > mx[k]:
            raise SpyderError(f"golden tolerance {k}={v:g} is looser than the engine maximum {mx[k]:g} "
                              f"for kind '{kind}'")
        out[k] = float(v)
    return out


def _want_vector(case, key, want):
    """Expected numeric output as a non-empty finite 1-D vector (a scalar becomes length 1)."""
    if isinstance(want, bool) or not isinstance(want, (int, float, list)):
        raise SpyderError(f"case {case}: expected '{key}' must be a number, a list of numbers or a label")
    if isinstance(want, list) and not all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in want):
        raise SpyderError(f"case {case}: expected '{key}' must be a flat list of numbers")
    w = np.atleast_1d(np.asarray(want, float))
    if w.ndim != 1 or w.size == 0:
        raise SpyderError(f"case {case}: expected '{key}' is empty")
    if not np.all(np.isfinite(w)):
        raise SpyderError(f"case {case}: expected '{key}' is not finite")
    return w


def _compare(case, key, got, want, tol, res):
    """Append one comparison record (case, key, got, want, ok). Labels compare exactly; numbers by
    |got - want| <= abs + rel*|want| element-wise, after a dimension check and a finiteness check on got."""
    if isinstance(want, str):
        g = got[0] if isinstance(got, list) and len(got) == 1 else got
        res.append((case, key, g, want, isinstance(g, str) and g == want))
        return
    w = _want_vector(case, key, want)
    if isinstance(got, list) and got and isinstance(got[0], str):
        res.append((case, f"{key} (label where a number was expected)", got, want, False))
        return
    g = np.asarray(got, float)
    g = g.reshape(g.shape[0], -1)[0] if g.ndim >= 1 else g.reshape(1)      # this scan's outputs
    if g.size != w.size:
        res.append((case, f"{key} (dimension)", int(g.size), int(w.size), False))
        return
    if not np.all(np.isfinite(g)):
        res.append((case, f"{key} (non-finite output)", g.tolist(), want, False))
        return
    if w.size == 1:
        gv, wv = float(g[0]), float(w[0])
        res.append((case, key, gv, wv, abs(gv - wv) <= tol["abs"] + tol["rel"] * abs(wv)))
    else:
        err = float(np.max(np.abs(g - w) / (tol["abs"] + tol["rel"] * np.abs(w) + 1e-300)))
        res.append((case, f"{key}(max err/tol)", err, 1.0, err <= 1.0))


def run_goldens(doc, golden_spectra, predict_fn=None):
    """Check every golden case of a model (or transfer) file. Returns list of (case, key, got, want, ok).

    Hardened (Phase 0a A10). Raises SpyderError, i.e. the file is rejected, when the goldens cannot test
    anything or are malformed: fewer than GOLDEN_MIN_CASES cases; an empty `expected` block; a required
    output missing for the kind; an output the engine does not know; no case with the feature vector (models
    with features); a tolerance looser than the engine maximum; a non-finite or empty expected value; a
    feature vector of the wrong length; a spectrum not in the golden spectra file; a case or a whole run with
    zero comparisons. Output mismatches, wrong dimensions and non-finite outputs are failing comparisons."""
    kind = golden_kind(doc)
    tol = golden_tolerance(doc)
    cases = doc["golden"].get("cases")
    if not isinstance(cases, list) or len(cases) < GOLDEN_MIN_CASES:
        raise SpyderError(f"golden: at least {GOLDEN_MIN_CASES} cases required, found "
                          f"{len(cases) if isinstance(cases, list) else 0}")
    is_model = doc["format"] == MODEL_FORMAT
    p = len(doc["features"]["wavelengths_nm"]) if is_model else 0
    known = set(GOLDEN_KNOWN[kind])
    if kind == "rule":
        known |= {f"feature:{k}" for k in doc["rule"]["features"]}
    required = list(GOLDEN_REQUIRED[kind])
    if kind == "consensus":
        comp = [f"component:{k['name']}" for k in doc["consensus"]["components"]]
        known |= set(comp)
        required += comp
    res = []
    for ci, case in enumerate(cases):
        name = case.get("name", f"case {ci}")
        exp = case.get("expected")
        if not isinstance(exp, dict) or not exp:
            raise SpyderError(f"golden case {name}: empty 'expected' block")
        missing = [k for k in required if k not in exp]
        if missing:
            raise SpyderError(f"golden case {name}: expected outputs missing for a {kind}: {missing}")
        unknown = sorted(set(exp) - known)
        if unknown:
            raise SpyderError(f"golden case {name}: unknown expected outputs {unknown}")
        sha = case.get("spectrum_sha256")
        if sha not in golden_spectra:
            raise SpyderError(f"golden case {name}: spectrum {str(sha)[:12]} not in the golden spectra file")
        wl, x, _ = golden_spectra[sha]
        ctx = ScanContext.from_json(case.get("scan_context", {}))
        n0 = len(res)
        if is_model:
            got = predict(doc, wl, x[None, :], ctx)
        else:
            wlt, Y = apply_transfer(doc, wl, x[None, :], ctx)
            if not np.all(np.isfinite(Y)):
                res.append((name, "transferred (non-finite output)", None, None, False))
            got = {"transferred_at": {str(int(round(v))): Y[0, i] for i, v in enumerate(wlt)}}
        for key, want in exp.items():
            if key == "transferred_at":
                if not isinstance(want, dict) or not want:
                    raise SpyderError(f"golden case {name}: empty 'transferred_at'")
                for lam, wv in want.items():
                    if lam not in got[key]:
                        res.append((name, f"T@{lam} (not on the output grid)", None, wv, False))
                        continue
                    _compare(name, f"T@{lam}", got[key][lam], wv, tol, res)
                continue
            if key not in got:
                res.append((name, f"{key} (not produced by this model)", None, want, False))
                continue
            _compare(name, key, got[key], want, tol, res)
        if "features" in case:
            if not is_model or p == 0:
                raise SpyderError(f"golden case {name}: 'features' given but the file has no feature list")
            want = _want_vector(name, "features", case["features"])
            if want.size != p:
                raise SpyderError(f"golden case {name}: {want.size} features, the model declares {p}")
            _, F, _ = model_features(doc, wl, x[None, :], ctx)
            if not np.all(np.isfinite(F)):
                res.append((name, "features (non-finite)", None, None, False))
            else:
                err = float(np.max(np.abs(F[0] - want) / (tol["abs"] + tol["rel"] * np.abs(want) + 1e-300)))
                res.append((name, "features(max err/tol)", err, 1.0, err <= 1.0))
        if len(res) == n0:
            raise SpyderError(f"golden case {name}: zero comparisons")
    if not res:
        raise SpyderError("golden: zero comparisons")
    if kind in GOLDEN_NEEDS_FEATURE_CASE and p > 0 and not any("features" in c for c in cases):
        raise SpyderError("golden: no case carries the intermediate feature vector")
    return res


def validate_file(path, golden_spectra=None):
    """Load, check and run the goldens of one model or transfer file, as the app does on load.
    golden_spectra: a dict from load_golden_spectra, or None to load the file named in golden.spectra_file
    from the plug-in's own folder (path-confined). Returns (n_checks, n_failed, failures, reason);
    reason is None if the file loaded, else the rejection message (then n_failed counts as 1)."""
    try:
        doc = load_json(path)
        (check_model if doc.get("format") == MODEL_FORMAT else check_transfer)(doc)
        if golden_spectra is None:
            golden_spectra = load_golden_spectra(package_path(Path(path).parent, doc["golden"]["spectra_file"],
                                                              "golden spectra file"))
        r = run_goldens(doc, golden_spectra)
    except (SpyderError, KeyError, TypeError) as e:
        return 0, 1, [], f"{type(e).__name__}: {e}"
    bad = [x for x in r if not x[-1]]
    return len(r), len(bad), bad, None


if __name__ == "__main__":   # python spyder_ref.py [golden_spectra.json] model1.json [model2.json ...]
    import sys
    args = sys.argv[1:]
    v1 = "--v1" in args
    args = [a for a in args if a != "--v1"]
    gs = None
    if args and json.loads(Path(args[0]).read_text(encoding="utf-8")).get("format") == GOLDEN_FORMAT:
        gs = load_golden_spectra(args[0])          # legacy form: one golden spectra file for every file
        args = args[1:]
    bad = 0
    for path in args:
        n, nbad, fails, reason = validate_file(path, gs)
        bad += nbad
        if reason:
            print(f"{Path(path).name}: REJECTED ({reason})")
            continue
        print(f"{Path(path).name}: {n} checks, {nbad} failed")
        if v1:
            pr = v1_problems(load_json(path))
            bad += len(pr)
            print(f"   v1 op subset: {'OK' if not pr else '; '.join(pr)}")
        for case, key, g, w, ok in fails:
            print("   FAIL", case, key, g, w)
    sys.exit(1 if bad else 0)
