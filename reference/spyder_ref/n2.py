"""Canonical noise measure N2 for SPYDER Bone (PLAN section 3 Step 2). THE ONLY N2 IMPLEMENTATION; never reimplement it.

Byte-for-byte the same function as the research original (planning copy, Phase 0a task A1); private file-name goldens
are not carried here (public synthetic N2 goldens are in plugins/golden_noise_public_v1.json).

Definition ("difference first, then select"):
  1. A = log10(1 / max(R, 1e-4)) on the FULL as-measured grid (index i <-> 350 + i nm).
  2. d2_i = A[i+1] - 2*A[i] + A[i-1], centred on channel i (i = 1 .. n-2).
  3. keep the centres whose wavelength lam satisfies lo <= lam < hi (2000-2100 gives 100 values).
  4. N2 = 1.4826 * median(|d2 - median(d2)|), in units of 1e-5 absorbance.
Clip 1e-4 is part of the definition (not a model clip). No resampling. Non-finite d2 values are dropped; none left -> NaN.
Input is the AS-MEASURED reflectance, never a transferred stream.
"""
from __future__ import annotations
import numpy as np

CLIP = 1e-4
MAD_K = 1.4826
UNITS = 1e5          # report in 1e-5 absorbance
WL_STD = np.arange(350, 2501)

def n2(R, lo: float = 2000, hi: float = 2100, wl=None):
    """Canonical N2 of reflectance R (1-D spectrum or 2-D rows x channels), in 1e-5 absorbance.

    Returns a float for 1-D input, an array (one value per row) for 2-D input.
    """
    R = np.asarray(R, dtype=float)
    one = R.ndim == 1
    R = np.atleast_2d(R)
    wl = WL_STD if wl is None else np.asarray(wl, dtype=float)
    if R.shape[1] != wl.size:
        raise ValueError(f"R has {R.shape[1]} channels but the grid has {wl.size}; pass wl=")
    A = np.log10(1.0 / np.clip(R, CLIP, None))
    d2 = A[:, 2:] - 2.0 * A[:, 1:-1] + A[:, :-2]
    centres = wl[1:-1]
    k = (centres >= lo) & (centres < hi)
    out = np.full(R.shape[0], np.nan)
    for j, row in enumerate(d2[:, k]):
        x = row[np.isfinite(row)]
        if x.size:
            out[j] = MAD_K * np.median(np.abs(x - np.median(x))) * UNITS
    return float(out[0]) if one else out


def n_centres(lo: float = 2000, hi: float = 2100, wl=None) -> int:
    wl = WL_STD if wl is None else np.asarray(wl, dtype=float)
    c = wl[1:-1]
    return int(((c >= lo) & (c < hi)).sum())
