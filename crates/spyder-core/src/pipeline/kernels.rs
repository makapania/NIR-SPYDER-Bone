//! Spectral kernels of the engine checks (port of `reference/oracle/kernels.py`): the derivative is computed on
//! the full grid by interior convolution with the spyder_ref Savitzky-Golay centre weights (the frozen table),
//! scaled; the first/last window//2 channels are NaN (a read touching one is NaN and the consumer reports
//! "not assessed").

use crate::numsum::np_mean;
use crate::plugins::tables::Kernel;
use crate::preprocess::{absorbance, savgol::try_sg_weights};

/// scale x SG(log10(1/max(R, clip))) on the full grid; NaN on the edge channels.
pub fn derivative(r: &[f64], k: &Kernel) -> Vec<f64> {
    let n = r.len();
    let a = absorbance(r, k.absorbance_clip_min);
    let p = &k.savgol;
    let Ok(w) = try_sg_weights(p.window, p.polyorder, p.deriv, p.delta) else {
        return vec![f64::NAN; n];
    };
    let c = &w.centre;
    let win = c.len();
    let h = win / 2;
    let mut y = vec![f64::NAN; n];
    if n < win {
        return y;
    }
    for (j, yj) in y.iter_mut().enumerate().take(n - h).skip(h) {
        let mut acc = 0.0;
        for (kk, ck) in c.iter().enumerate() {
            acc += ck * a[j - h + kk];
        }
        *yj = acc * k.scale;
    }
    y
}

/// Index of `lam` on the grid (|diff| <= 1e-6), if exactly one.
pub fn idx(wl: &[f64], lam: f64) -> Option<usize> {
    crate::grid::index_of(wl, lam)
}

/// Mean of y over the inclusive wavelength window [lo, hi] (numpy summation); NaN if off the grid.
pub fn window_mean(y: &[f64], wl: &[f64], lo: f64, hi: f64) -> f64 {
    match (idx(wl, lo), idx(wl, hi)) {
        (Some(a), Some(b)) if a <= b => np_mean(&y[a..=b]),
        _ => f64::NAN,
    }
}

/// Mean over centre +/- half_width nm (inclusive).
pub fn band_reading(y: &[f64], wl: &[f64], centre: f64, half_width: f64) -> f64 {
    window_mean(y, wl, centre - half_width, centre + half_width)
}

/// Sharp index: mean y[c-h..c+h] - 0.5 (mean y[c-f1..c-f0] + mean y[c+f0..c+f1]).
pub fn sharp(y: &[f64], wl: &[f64], c: f64, h: f64, f0: f64, f1: f64) -> f64 {
    window_mean(y, wl, c - h, c + h)
        - 0.5 * (window_mean(y, wl, c - f1, c - f0) + window_mean(y, wl, c + f0, c + f1))
}

/// numpy `np.max` over the inclusive window (NaN propagates).
pub fn window_max(y: &[f64], wl: &[f64], lo: f64, hi: f64) -> f64 {
    match (idx(wl, lo), idx(wl, hi)) {
        (Some(a), Some(b)) if a <= b => {
            let mut m = f64::NEG_INFINITY;
            for &v in &y[a..=b] {
                if v.is_nan() {
                    return f64::NAN;
                }
                if v > m {
                    m = v;
                }
            }
            m
        }
        _ => f64::NAN,
    }
}

/// A band's reading r_b: the mean over centre +/- half_width of E, or of the projected E when the band names a
/// projection in the band table (the OH-corrected 1545 nm trough; oracle `kernels.band_value`). NaN if the band or
/// its projection is missing.
pub fn band_value(e: &[f64], wl: &[f64], bands: &crate::plugins::tables::Bands, id: &str) -> f64 {
    let Some(b) = bands.bands.get(id) else {
        return f64::NAN;
    };
    let h = b.half_width_nm as f64;
    match &b.projection {
        None => band_reading(e, wl, b.centre_nm, h),
        Some(p) => {
            let Some(pj) = bands.projections.get(p) else {
                return f64::NAN;
            };
            let Some(k) = bands.kernels.get(&pj.kernel) else {
                return f64::NAN;
            };
            band_reading(&pj.apply(e, wl, k.scale), wl, b.centre_nm, h)
        }
    }
}
