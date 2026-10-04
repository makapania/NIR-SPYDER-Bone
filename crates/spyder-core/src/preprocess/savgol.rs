//! Savitzky-Golay smoothing and derivatives (spyder_ref `op_savgol`, `_sg_matrix`).
//!
//! Weights: least-squares polynomial of degree P through W samples, d-th derivative, scaled by
//! 1 / (h^d * delta^d) with h = W // 2, on the conditioned offsets u = (t - h) / h. For every
//! (W, P, d) of the shipped and planned chains the unscaled matrix is frozen from spyder_ref in
//! `sg_table.rs`, so the weights are bitwise equal to spyder_ref's (PLAN section 8 Phase 1). Other
//! combinations use a Householder-QR solve (agreement ~1e-16 relative; tests/operators.rs).

use super::sg_table::SG_TABLE;

/// Edge handling (04 section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgMode {
    /// scipy default: first/last h outputs from the polynomial fitted to the first/last W samples.
    Interp,
    /// scipy padding modes (`mirror` = numpy `reflect`, `nearest` = numpy `edge`, `constant` cval 0, `wrap`).
    Mirror,
    Nearest,
    Constant,
    Wrap,
    /// Interior only; h outputs at each end set to 0 (Ryder / Unscrambler rebuild).
    ZeroEdges,
    /// Interior only; the grid shrinks by h at each end.
    Valid,
}

impl SgMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "interp" => SgMode::Interp,
            "mirror" => SgMode::Mirror,
            "nearest" => SgMode::Nearest,
            "constant" => SgMode::Constant,
            "wrap" => SgMode::Wrap,
            "zero_edges" => SgMode::ZeroEdges,
            "valid" => SgMode::Valid,
            _ => return None,
        })
    }
}

/// Weights of one (W, P, d, delta).
#[derive(Debug, Clone, PartialEq)]
pub struct SgWeights {
    pub window: usize,
    /// Centre weights: out[j] = sum_k centre[k] * x[j - h + k].
    pub centre: Vec<f64>,
    /// Rows for output positions t = 0..h-1 of the first W samples (mode interp).
    pub left: Vec<Vec<f64>>,
    /// Rows for output positions t = W-h..W-1 of the last W samples (mode interp).
    pub right: Vec<Vec<f64>>,
    /// True when the weights came from the frozen spyder_ref table.
    pub from_table: bool,
}

fn unpack(bits: &[u64], rows: usize, cols: usize) -> Vec<Vec<f64>> {
    (0..rows)
        .map(|r| {
            bits[r * cols..(r + 1) * cols]
                .iter()
                .map(|&b| f64::from_bits(b))
                .collect()
        })
        .collect()
}

/// The divisor spyder_ref applies: `h ** deriv * delta ** deriv` (Python int power exact, then float).
/// Checked integer power (Codex Phase 1 MEDIUM 1: `128u64.pow(10)` overflowed); when h^deriv does not fit in
/// u64 the float power is used (Python's exact int would round to nearly the same float). Returns None when the
/// divisor is not a finite number > 0 (callers reject the parameters).
pub fn divisor(h: usize, deriv: usize, delta: f64) -> Option<f64> {
    let hp = u32::try_from(deriv)
        .ok()
        .and_then(|d| (h as u64).checked_pow(d))
        .map(|v| v as f64)
        .unwrap_or_else(|| (h as f64).powf(deriv as f64));
    let div = hp * delta.powf(deriv as f64);
    (div.is_finite() && div > 0.0).then_some(div)
}

/// Unscaled M = D @ pinv(V) by Householder QR (fallback for combinations not in the table).
/// V = [u_t^m] (W x (P+1)); pinv(V) = R^-1 Q^T; a row of M is Q (R^-T D_row).
#[allow(clippy::needless_range_loop)] // index loops mirror the matrix algebra
pub fn sg_unscaled_qr(
    window: usize,
    polyorder: usize,
    deriv: usize,
    positions: &[usize],
) -> Vec<Vec<f64>> {
    let h = (window / 2) as f64;
    let w = window;
    let p = polyorder + 1;
    // A = V, column-major-ish storage a[i][j]
    let mut a: Vec<Vec<f64>> = (0..w)
        .map(|t| {
            let u = (t as f64 - h) / h;
            (0..p).map(|m| u.powi(m as i32)).collect()
        })
        .collect();
    // Householder QR: store reflectors to build thin Q explicitly.
    let mut vs: Vec<Vec<f64>> = Vec::with_capacity(p);
    for k in 0..p {
        let norm: f64 = (k..w).map(|i| a[i][k] * a[i][k]).sum::<f64>().sqrt();
        let alpha = if a[k][k] > 0.0 { -norm } else { norm };
        let mut v = vec![0.0; w];
        for i in k..w {
            v[i] = a[i][k];
        }
        v[k] -= alpha;
        let vnorm2: f64 = v.iter().map(|x| x * x).sum();
        if vnorm2 > 0.0 {
            for j in k..p {
                let dot: f64 = (k..w).map(|i| v[i] * a[i][j]).sum();
                let f = 2.0 * dot / vnorm2;
                for i in k..w {
                    a[i][j] -= f * v[i];
                }
            }
        }
        vs.push(v);
    }
    // R = upper p x p of a; thin Q = H_0 ... H_{p-1} applied to the first p unit vectors.
    let r: Vec<Vec<f64>> = (0..p).map(|i| a[i][..p].to_vec()).collect();
    let mut q = vec![vec![0.0; p]; w]; // w x p
    for (j, row) in q.iter_mut().enumerate().take(p) {
        row[j] = 1.0;
    }
    for k in (0..p).rev() {
        let v = &vs[k];
        let vnorm2: f64 = v.iter().map(|x| x * x).sum();
        if vnorm2 == 0.0 {
            continue;
        }
        for j in 0..p {
            let dot: f64 = (k..w).map(|i| v[i] * q[i][j]).sum();
            let f = 2.0 * dot / vnorm2;
            for i in k..w {
                q[i][j] -= f * v[i];
            }
        }
    }
    let fact = |n: usize| -> f64 { (1..=n).map(|x| x as f64).product() };
    positions
        .iter()
        .map(|&pos| {
            let ue = (pos as f64 - h) / h;
            let mut dvec = vec![0.0; p];
            for (k, dv) in dvec.iter_mut().enumerate().skip(deriv) {
                *dv = fact(k) / fact(k - deriv) * ue.powi((k - deriv) as i32);
            }
            // solve R^T z = d (R^T lower triangular)
            let mut z = vec![0.0; p];
            for i in 0..p {
                let s: f64 = (0..i).map(|k| r[k][i] * z[k]).sum();
                z[i] = (dvec[i] - s) / r[i][i];
            }
            (0..w)
                .map(|t| (0..p).map(|k| q[t][k] * z[k]).sum())
                .collect()
        })
        .collect()
}

/// Savitzky-Golay weights for (window, polyorder, deriv, delta). Callers validate the parameters; an
/// unusable divisor (see [`divisor`]) or non-finite weights give an error, never a panic.
pub fn try_sg_weights(
    window: usize,
    polyorder: usize,
    deriv: usize,
    delta: f64,
) -> Result<SgWeights, String> {
    if window < 3 || window % 2 != 1 || window <= polyorder || deriv > polyorder {
        return Err("SG window must be odd, >= 3 and > polyorder, with deriv <= polyorder".into());
    }
    let h = window / 2;
    if divisor(h, deriv, delta).is_none() {
        return Err(format!(
            "SG derivative scale h^deriv * delta^deriv is not a finite number > 0 (h {h}, deriv {deriv}, delta {delta})"
        ));
    }
    let w = sg_weights(window, polyorder, deriv, delta);
    let all = w
        .centre
        .iter()
        .chain(w.left.iter().flatten())
        .chain(w.right.iter().flatten());
    if all.into_iter().any(|v| !v.is_finite()) {
        return Err(format!(
            "SG weights for window {window}, polyorder {polyorder}, deriv {deriv} are not finite"
        ));
    }
    Ok(w)
}

/// Savitzky-Golay weights for (window, polyorder, deriv, delta). Callers validate the parameters (prefer
/// [`try_sg_weights`]); an unusable divisor yields NaN weights rather than a panic.
pub fn sg_weights(window: usize, polyorder: usize, deriv: usize, delta: f64) -> SgWeights {
    let h = window / 2;
    let div = divisor(h, deriv, delta).unwrap_or(f64::NAN);
    let scale = |rows: Vec<Vec<f64>>| -> Vec<Vec<f64>> {
        rows.into_iter()
            .map(|r| r.into_iter().map(|v| v / div).collect())
            .collect()
    };
    let entry = SG_TABLE
        .iter()
        .find(|e| e.window == window && e.polyorder == polyorder && e.deriv == deriv);
    let centre = match entry {
        Some(e) => scale(unpack(e.centre, 1, window)),
        None => scale(sg_unscaled_qr(window, polyorder, deriv, &[h])),
    }
    .remove(0);
    let (left, right) = match entry {
        Some(e) if !e.left.is_empty() => (
            scale(unpack(e.left, h, window)),
            scale(unpack(e.right, h, window)),
        ),
        _ => {
            let lp: Vec<usize> = (0..h).collect();
            let rp: Vec<usize> = (window - h..window).collect();
            (
                scale(sg_unscaled_qr(window, polyorder, deriv, &lp)),
                scale(sg_unscaled_qr(window, polyorder, deriv, &rp)),
            )
        }
    };
    SgWeights {
        window,
        centre,
        left,
        right,
        from_table: entry.is_some(),
    }
}

/// Whether the centre weights of this combination are frozen from spyder_ref.
pub fn in_table(window: usize, polyorder: usize, deriv: usize) -> bool {
    SG_TABLE
        .iter()
        .any(|e| e.window == window && e.polyorder == polyorder && e.deriv == deriv)
}

/// Apply SG to one spectrum. Returns the output values and, for `Valid`, the index range kept.
/// Preconditions (checked by the operator parser/runner): window odd, window > polyorder, window <= n.
pub fn apply(x: &[f64], w: &SgWeights, mode: SgMode) -> (Vec<f64>, std::ops::Range<usize>) {
    let n = x.len();
    let win = w.window;
    let h = win / 2;
    let c = &w.centre;
    match mode {
        SgMode::Interp | SgMode::ZeroEdges | SgMode::Valid => {
            let mut y = vec![0.0; n];
            // numpy: Y[:, h:n-h] += c[k] * X[:, k:n-2h+k] for k in 0..W (same accumulation order)
            for (j, yj) in y.iter_mut().enumerate().take(n - h).skip(h) {
                let mut acc = 0.0;
                for (k, ck) in c.iter().enumerate() {
                    acc += ck * x[j - h + k];
                }
                *yj = acc;
            }
            match mode {
                SgMode::Interp => {
                    for (t, row) in w.left.iter().enumerate() {
                        y[t] = row.iter().zip(&x[..win]).map(|(a, b)| a * b).sum();
                    }
                    for (t, row) in w.right.iter().enumerate() {
                        y[n - h + t] = row.iter().zip(&x[n - win..]).map(|(a, b)| a * b).sum();
                    }
                    (y, 0..n)
                }
                SgMode::Valid => (y[h..n - h].to_vec(), h..n - h),
                _ => (y, 0..n),
            }
        }
        _ => {
            let at = |i: isize| -> f64 {
                let ni = n as isize;
                if (0..ni).contains(&i) {
                    return x[i as usize];
                }
                match mode {
                    SgMode::Constant => 0.0,
                    SgMode::Nearest => x[if i < 0 { 0 } else { n - 1 }],
                    SgMode::Wrap => x[i.rem_euclid(ni) as usize],
                    // numpy 'reflect': edge sample not repeated (valid while h <= n - 1)
                    _ => {
                        let r = if i < 0 { -i } else { 2 * (ni - 1) - i };
                        x[r.clamp(0, ni - 1) as usize]
                    }
                }
            };
            let mut y = vec![0.0; n];
            for (j, yj) in y.iter_mut().enumerate() {
                let mut acc = 0.0;
                for (k, ck) in c.iter().enumerate() {
                    acc += ck * at(j as isize - h as isize + k as isize);
                }
                *yj = acc;
            }
            (y, 0..n)
        }
    }
}
