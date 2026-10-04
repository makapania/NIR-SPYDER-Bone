//! Canonical noise measure N2, a line-by-line port of `planning/work/phase0a/common/n2.py` (the only N2
//! definition; LESSONS 15). Never reimplement it elsewhere.
//!
//! 1. A = log10(1 / max(R, 1e-4)) on the FULL as-measured grid.
//! 2. d2_i = A[i+1] - 2*A[i] + A[i-1], centred on channel i (i = 1 .. n-2).
//! 3. keep the centres with lo <= lambda < hi (2000-2100 gives 100 values).
//! 4. N2 = 1.4826 * median(|d2 - median(d2)|), in units of 1e-5 absorbance.
//!
//! Non-finite d2 values are dropped before the medians; if none remain, N2 is undefined (None).
//! Input is always the AS-MEASURED reflectance, never a transferred stream.

use serde::Serialize;

pub const CLIP: f64 = 1e-4;
pub const MAD_K: f64 = 1.4826;
pub const UNITS: f64 = 1e5;

/// The windows the app uses (PLAN Step 2): SWIR2 default, two SWIR1 windows, and the long-wave policy window.
pub const WINDOWS: [(f64, f64); 4] = [
    (2000.0, 2100.0),
    (1500.0, 1600.0),
    (1500.0, 1550.0),
    (2300.0, 2400.0),
];

/// numpy `np.clip(x, lo, None)`: NaN propagates.
fn clip_lo(x: f64, lo: f64) -> f64 {
    if x.is_nan() {
        x
    } else if x < lo {
        lo
    } else {
        x
    }
}

/// numpy `np.median` of a non-empty slice (mean of the two middle values when even).
pub fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// N2 of reflectance `r` on grid `wl`, window [lo, hi), in 1e-5 absorbance. None if no finite d2 remains
/// or the grid and spectrum lengths differ.
pub fn n2(r: &[f64], wl: &[f64], lo: f64, hi: f64) -> Option<f64> {
    if r.len() != wl.len() || r.len() < 3 {
        return None;
    }
    let a: Vec<f64> = r
        .iter()
        .map(|&x| (1.0 / clip_lo(x, CLIP)).log10())
        .collect();
    let mut d2 = Vec::new();
    for i in 1..a.len() - 1 {
        let c = wl[i];
        if c >= lo && c < hi {
            let v = a[i + 1] - 2.0 * a[i] + a[i - 1];
            if v.is_finite() {
                d2.push(v);
            }
        }
    }
    if d2.is_empty() {
        return None;
    }
    let m = median(&d2);
    let dev: Vec<f64> = d2.iter().map(|x| (x - m).abs()).collect();
    Some(MAD_K * median(&dev) * UNITS)
}

/// N2 in the four app windows.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct N2Set {
    /// 2000-2100 nm (SWIR2; B6, B6b).
    pub n2_2000_2100: Option<f64>,
    /// 1500-1600 nm (SWIR1).
    pub n2_1500_1600: Option<f64>,
    /// 1500-1550 nm (SWIR1; OH-corrected 1500, F05).
    pub n2_1500_1550: Option<f64>,
    /// 2300-2400 nm (long-wave policy).
    pub n2_2300_2400: Option<f64>,
}

impl N2Set {
    pub fn compute(r: &[f64], wl: &[f64]) -> Self {
        let w = |k: usize| n2(r, wl, WINDOWS[k].0, WINDOWS[k].1);
        N2Set {
            n2_2000_2100: w(0),
            n2_1500_1600: w(1),
            n2_1500_1550: w(2),
            n2_2300_2400: w(3),
        }
    }

    /// N2 for a window given in nm, if it is one of the four.
    pub fn get(&self, window: (f64, f64)) -> Option<f64> {
        match WINDOWS.iter().position(|&w| w == window)? {
            0 => self.n2_2000_2100,
            1 => self.n2_1500_1600,
            2 => self.n2_1500_1550,
            _ => self.n2_2300_2400,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Vec<f64> {
        (0..2151).map(|i| 350.0 + i as f64).collect()
    }

    #[test]
    fn flat_is_zero_and_counts() {
        let wl = grid();
        let r = vec![0.3; wl.len()];
        assert_eq!(n2(&r, &wl, 2000.0, 2100.0), Some(0.0));
    }

    #[test]
    fn linear_trend_in_a_is_removed() {
        let wl = grid();
        let r: Vec<f64> = wl
            .iter()
            .map(|w| 10f64.powf(-(0.5 + 1e-3 * (w - 350.0))))
            .collect();
        assert!(n2(&r, &wl, 2000.0, 2100.0).unwrap().abs() < 1e-6);
    }

    #[test]
    fn spike_window_is_half_open() {
        // one spike centred at 2100 nm sits outside [2000, 2100), but its neighbours' d2 at 2099 is inside
        let wl = grid();
        let mut r = vec![0.3; wl.len()];
        r[2100 - 350] = 0.31;
        // d2 at 2099 is nonzero, every other centre in the window is 0 -> median 0, MAD 0
        assert_eq!(n2(&r, &wl, 2000.0, 2100.0), Some(0.0));
    }

    #[test]
    fn non_positive_reflectance_is_clipped() {
        let wl = grid();
        let mut r = vec![0.3; wl.len()];
        r[1700] = -0.1;
        r[1701] = -0.1;
        assert!(n2(&r, &wl, 2000.0, 2100.0).unwrap().is_finite());
    }

    #[test]
    fn nan_dropped_and_all_nan_is_none() {
        let wl = grid();
        let mut r = vec![0.3; wl.len()];
        r[1700] = f64::NAN;
        assert_eq!(n2(&r, &wl, 2000.0, 2100.0), Some(0.0));
        let r = vec![f64::NAN; wl.len()];
        assert_eq!(n2(&r, &wl, 2000.0, 2100.0), None);
    }

    #[test]
    fn medians() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
    }
}
