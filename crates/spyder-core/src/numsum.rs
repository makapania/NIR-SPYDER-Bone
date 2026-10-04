//! numpy-compatible summation, so that means and normalisations follow numpy's rounding exactly.
//! `np.sum` over a contiguous float64 vector is numpy's `pairwise_sum` over all elements: sequential from
//! -0.0 below 8 values, 8 accumulators up to 128, recursive halving above (verified bitwise against
//! numpy 2.4 in tests/operators.rs).

const PW_BLOCKSIZE: usize = 128;

fn pairwise(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        // numpy starts at -0.0 to preserve -0.0 values
        let mut res = -0.0;
        for &v in a {
            res += v;
        }
        res
    } else if n <= PW_BLOCKSIZE {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - (n % 8) {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise(&a[..n2]) + pairwise(&a[n2..])
    }
}

/// `np.sum(a)` for a contiguous float64 vector (0.0 when empty).
pub fn np_sum(a: &[f64]) -> f64 {
    if a.is_empty() {
        0.0
    } else {
        pairwise(a)
    }
}

/// `np.mean(a)`.
pub fn np_mean(a: &[f64]) -> f64 {
    np_sum(a) / a.len() as f64
}

/// `np.std(a, ddof=ddof)`: sqrt(sum((x - mean)^2) / (n - ddof)).
pub fn np_std(a: &[f64], ddof: usize) -> f64 {
    let m = np_mean(a);
    let sq: Vec<f64> = a.iter().map(|x| (x - m) * (x - m)).collect();
    let den = a.len().saturating_sub(ddof) as f64;
    (np_sum(&sq) / den).sqrt()
}
