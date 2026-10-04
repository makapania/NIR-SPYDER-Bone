//! Grid helpers and grid eligibility (PLAN section 3 Step 1).
//!
//! A model, band or check runs only if the file supplies its window +/- SG half-width +/- edge margin, and only
//! if its kernel support (window +/- SG half-width) does not cross a detector join it was not trained across.
//! Otherwise it is "not assessed", never a guess.

use crate::status::Status;

/// Wavelength equality tolerance (spyder_ref `WL_TOL`).
pub const WL_TOL: f64 = 1e-6;

/// Index of the grid point equal to `nm` within `WL_TOL`; None if absent or ambiguous.
pub fn index_of(wl: &[f64], nm: f64) -> Option<usize> {
    let mut found = None;
    for (i, &w) in wl.iter().enumerate() {
        if (w - nm).abs() <= WL_TOL {
            if found.is_some() {
                return None;
            }
            found = Some(i);
        }
    }
    found
}

/// Index of the last grid point with wl <= nm (+tol): the last channel of the lower detector.
pub fn last_le(wl: &[f64], nm: f64) -> Option<usize> {
    wl.iter().rposition(|&w| w <= nm + WL_TOL)
}

/// Contiguous index ranges `[a, b)` of detector segments, split after each join (wl <= join is lower).
/// Joins outside `[wl[0], wl[last])` are ignored (spyder_ref `_segments`).
pub fn segment_ranges(wl: &[f64], joins: &[f64]) -> Vec<(usize, usize)> {
    if wl.is_empty() {
        return Vec::new();
    }
    let mut js: Vec<f64> = joins.iter().copied().filter(|j| j.is_finite()).collect();
    js.sort_by(f64::total_cmp);
    let mut cuts = vec![0usize];
    let (first, last) = (wl[0], wl[wl.len() - 1]);
    for j in js {
        if first <= j && j < last {
            if let Some(i) = last_le(wl, j) {
                cuts.push(i + 1);
            }
        }
    }
    cuts.push(wl.len());
    cuts.windows(2)
        .filter(|c| c[1] > c[0])
        .map(|c| (c[0], c[1]))
        .collect()
}

/// The common step of a uniform grid (|diff - step| <= 1e-6 everywhere), else None.
pub fn uniform_step(wl: &[f64]) -> Option<f64> {
    if wl.len() < 2 {
        return None;
    }
    let step = wl[1] - wl[0];
    if !(step.is_finite() && step > 0.0) {
        return None;
    }
    if wl.windows(2).all(|p| ((p[1] - p[0]) - step).abs() <= 1e-6) {
        Some(step)
    } else {
        None
    }
}

/// What a spectral consumer (model, band, check) needs from the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Consumer {
    /// The window the consumer reads, nm, inclusive.
    pub window_nm: (f64, f64),
    /// Savitzky-Golay half-width in channels (window // 2); 0 for consumers without a kernel.
    pub sg_half_width: usize,
    /// Extra channels the file must supply beyond the kernel support.
    pub edge_margin: usize,
    /// Joins the consumer was trained across (its kernel may span these); usually empty.
    pub trained_across_joins_nm: Vec<f64>,
}

/// Grid eligibility of one consumer on a scan's grid and joins.
pub fn eligibility(wl: &[f64], joins: &[f64], c: &Consumer) -> Status {
    let (lo, hi) = c.window_nm;
    if !(lo.is_finite() && hi.is_finite() && lo <= hi) {
        return Status::not_assessed(format!("invalid window {lo}-{hi} nm"));
    }
    let Some(step) = uniform_step(wl) else {
        return Status::not_assessed("the scan's grid is not uniform");
    };
    let half = c.sg_half_width as f64 * step;
    let reach = (c.sg_half_width + c.edge_margin) as f64 * step;
    let (first, last) = (wl[0], wl[wl.len() - 1]);
    if first > lo - reach + WL_TOL || last < hi + reach - WL_TOL {
        return Status::not_assessed(format!(
            "the file covers {first}-{last} nm; this needs {}-{} nm",
            lo - reach,
            hi + reach
        ));
    }
    if !wl.iter().any(|&w| w >= lo - WL_TOL && w <= hi + WL_TOL) {
        return Status::not_assessed(format!("no grid point in {lo}-{hi} nm"));
    }
    let (s_lo, s_hi) = (lo - half, hi + half);
    for &j in joins {
        let below = wl
            .iter()
            .any(|&w| w >= s_lo - WL_TOL && w <= s_hi + WL_TOL && w <= j + WL_TOL);
        let above = wl
            .iter()
            .any(|&w| w >= s_lo - WL_TOL && w <= s_hi + WL_TOL && w > j + WL_TOL);
        let trained = c
            .trained_across_joins_nm
            .iter()
            .any(|&t| (t - j).abs() <= WL_TOL);
        if below && above && !trained {
            return Status::not_assessed(format!(
                "kernel support {s_lo}-{s_hi} nm crosses the detector join at {j} nm"
            ));
        }
    }
    Status::Assessed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn std_grid() -> Vec<f64> {
        (0..2151).map(|i| 350.0 + i as f64).collect()
    }

    fn consumer(lo: f64, hi: f64, h: usize) -> Consumer {
        Consumer {
            window_nm: (lo, hi),
            sg_half_width: h,
            edge_margin: 0,
            trained_across_joins_nm: vec![],
        }
    }

    #[test]
    fn segments_follow_lower_convention() {
        let wl = std_grid();
        let s = segment_ranges(&wl, &[1000.0, 1800.0]);
        assert_eq!(s, vec![(0, 651), (651, 1451), (1451, 2151)]);
        assert_eq!(wl[650], 1000.0);
        assert_eq!(wl[1450], 1800.0);
        // joins outside the grid are ignored; a join at the last point does not split
        assert_eq!(segment_ranges(&wl, &[100.0, 2500.0]), vec![(0, 2151)]);
    }

    #[test]
    fn ryder_window_eligible() {
        let wl = std_grid();
        let j = [1000.0, 1800.0];
        assert!(eligibility(&wl, &j, &consumer(2030.0, 2060.0, 15)).is_assessed());
        assert!(eligibility(&wl, &j, &consumer(1500.0, 1550.0, 15)).is_assessed());
    }

    #[test]
    fn join_crossing_not_assessed() {
        let wl = std_grid();
        let j = [1000.0, 1800.0];
        // C-H 1728 band +/- 2 nm with SG31: support 1711-1745, fine; a window ending at 1790 reaches 1805
        assert!(eligibility(&wl, &j, &consumer(1726.0, 1730.0, 15)).is_assessed());
        let s = eligibility(&wl, &j, &consumer(1770.0, 1790.0, 15));
        assert!(matches!(s, Status::NotAssessed { .. }), "{s:?}");
        // support ending exactly at the join (1785 + 15 = 1800) stays in the lower detector
        assert!(eligibility(&wl, &j, &consumer(1770.0, 1785.0, 15)).is_assessed());
        // ... one channel more crosses it
        assert!(!eligibility(&wl, &j, &consumer(1770.0, 1786.0, 15)).is_assessed());
        // trained across the join: allowed
        let mut c = consumer(1770.0, 1790.0, 15);
        c.trained_across_joins_nm = vec![1800.0];
        assert!(eligibility(&wl, &j, &c).is_assessed());
    }

    #[test]
    fn grid_coverage() {
        let wl: Vec<f64> = (0..300).map(|i| 1900.0 + i as f64).collect(); // 1900-2199
        let j = [1000.0, 1800.0];
        assert!(eligibility(&wl, &j, &consumer(2030.0, 2060.0, 15)).is_assessed());
        // 2160-2180 +/- 15 = 2145-2195: supplied
        let mut c = consumer(2160.0, 2180.0, 15);
        assert!(eligibility(&wl, &j, &c).is_assessed());
        // ... but not with an edge margin of 5 channels (needs 2200)
        c.edge_margin = 5;
        assert!(!eligibility(&wl, &j, &c).is_assessed());
        // ... and not when the window itself runs to 2190 (needs 2205)
        assert!(!eligibility(&wl, &j, &consumer(2170.0, 2190.0, 15)).is_assessed());
        // the low end too
        assert!(!eligibility(&wl, &j, &consumer(1905.0, 1920.0, 15)).is_assessed());
    }
}
