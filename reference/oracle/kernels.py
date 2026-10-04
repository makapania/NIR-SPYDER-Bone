"""Spectral kernels used by the engine checks. Pure numpy; Savitzky-Golay weights come from spyder_ref (one SG
definition for the whole engine). Every kernel is described by a dict loaded from a shipped file:

    {"absorbance_clip": 1e-6, "savgol": {"window": 31, "polyorder": 3, "deriv": 2, "delta": 1.0}, "scale": -1e5}

The derivative is computed on the full grid by interior convolution; the first/last window//2 channels are NaN
(no consumer reads them; a read that touches one returns NaN and the consumer reports "not assessed").
"""
from __future__ import annotations

import numpy as np

from ._ref import SR

_W_CACHE: dict = {}


def sg_weights(window, polyorder, deriv, delta=1.0):
    key = (int(window), int(polyorder), int(deriv), float(delta))
    if key not in _W_CACHE:
        _W_CACHE[key] = np.asarray(SR.sg_centre_weights(*key), float)
    return _W_CACHE[key]


def absorbance(R, clip):
    """A = log10(1 / max(R, clip)), written exactly as spyder_ref.op_absorbance."""
    return np.log10(1.0 / np.maximum(np.asarray(R, float), float(clip)))


def derivative(R, kernel):
    """scale * SG(A) on the full grid (rows x channels); NaN on the h edge channels."""
    R = np.atleast_2d(np.asarray(R, float))
    A = absorbance(R, kernel["absorbance_clip"])
    sg = kernel["savgol"]
    c = sg_weights(sg["window"], sg["polyorder"], sg.get("deriv", 0), sg.get("delta", 1.0))
    W = len(c); h = W // 2; n = A.shape[1]
    Y = np.full_like(A, np.nan)
    acc = np.zeros((A.shape[0], n - 2 * h))
    for k in range(W):
        acc += c[k] * A[:, k:n - 2 * h + k]
    Y[:, h:n - h] = acc * float(kernel.get("scale", 1.0))
    return Y


def idx(wl, lam):
    i = np.flatnonzero(np.abs(np.asarray(wl) - lam) <= 1e-6)
    if len(i) != 1:
        raise ValueError(f"{lam} nm not on the grid")
    return int(i[0])


def window_mean(Y, wl, lo, hi):
    """Mean of Y over the inclusive wavelength window [lo, hi] (1-nm grid)."""
    a, b = idx(wl, lo), idx(wl, hi)
    return Y[:, a:b + 1].mean(1)


def band_reading(Y, wl, centre, half_width):
    """Mean over centre +- half_width nm (inclusive)."""
    return window_mean(Y, wl, centre - half_width, centre + half_width)


def sharp(Y, wl, centre, half, f0, f1):
    """Sharp index (band height above its two shoulders): mean Y[c-h..c+h] - 0.5*(mean Y[c-f1..c-f0] + mean Y[c+f0..c+f1])."""
    return window_mean(Y, wl, centre - half, centre + half) - 0.5 * (
        window_mean(Y, wl, centre - f1, centre - f0) + window_mean(Y, wl, centre + f0, centre + f1))


def window_max(Y, wl, lo, hi):
    a, b = idx(wl, lo), idx(wl, hi)
    return np.max(Y[:, a:b + 1], axis=1)


def projected(Y, wl, proj):
    """E with a bands.json projection applied over its window (NaN elsewhere), row by row:
    x = E / scale, x' = x - v (v . (x - c)), E' = scale x'. The dot product is summed in channel order (a plain loop),
    so the engine port reproduces it exactly."""
    Y = np.atleast_2d(np.asarray(Y, float))
    a, b = idx(wl, proj["window_nm"][0]), idx(wl, proj["window_nm"][1])
    v = np.asarray(proj["direction"], float); c = np.asarray(proj["centre"], float); sc = float(proj["scale"])
    if b - a + 1 != len(v) or len(c) != len(v):
        raise ValueError("projection: direction / centre length does not match the window")
    out = np.full_like(Y, np.nan)
    for r in range(Y.shape[0]):
        x = Y[r, a:b + 1] / sc
        d = 0.0
        for i in range(len(v)):
            d += (x[i] - c[i]) * v[i]
        out[r, a:b + 1] = sc * (x - v * d)
    return out


def band_value(Y, wl, bd):
    """A band's reading r_b (rows): mean over centre +- half_width of E, or of the projected E when the band names a
    projection (the OH-corrected 1545 nm trough)."""
    if bd.get("projection"):
        Y = projected(Y, wl, bd["projection"])
    return band_reading(Y, wl, bd["centre_nm"], bd["half_width_nm"])
