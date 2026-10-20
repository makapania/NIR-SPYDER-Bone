//! Noise propagation for any correction file (DECISIONS 137, Phase 2; spyder_ref `gains`): the implied SD of a model
//! reading (B6) and the readability SD of a band reading, COMPUTED from the output's own linear kernel, the correction
//! actually applied and the scan's own detector segments. Nothing is looked up by a transfer file's SHA-256.
//!
//! Noise model (the shipped one): white noise in absorbance, smoothed by a Gaussian of `correlation_nm` (1.25 nm for
//! the ASD LabSpec; a correction may give its instrument's own value in `provenance.instrument_noise.correlation_nm`),
//! independent between detector segments, measured on the AS-MEASURED scan by the canonical N2 (units 1e-5
//! absorbance). For an output with absorbance kernel w, the SD per unit N2 of one segment's part is
//!
//! ```text
//! gain = 1e-5 x sqrt( |T'w * g|^2 / |d2 * g|^2 )
//! ```
//!
//! with g the noise Gaussian (taps to +-8 sigma, in grid steps), d2 = [1, -2, 1] and T' the adjoint of the
//! correction's operators on that segment, in reverse order: a per-segment absorbance gain multiplies; a per-segment
//! Gaussian blur is its exact (segment-clipped, renormalised) blur matrix transposed. The blur acts on reflectance in
//! the engine and is linearised here, as the shipped gains were. Each segment's part uses an N2 window inside that
//! segment; parts add in quadrature (independent detectors).
//!
//! Linear only: a correction with any other operator (block SNV, ...) has no analytic gain ("non-linear
//! correction"); a model chain must be absorbance followed by Savitzky-Golay / crop steps and a linear read-out.
//! Coverage: every Savitzky-Golay window of the output (every run of a folded consensus component's wavelengths)
//! must lie inside the grid and inside ONE detector segment, and its N2 window in that same segment; otherwise the
//! output is "not available on this instrument". Each segment's part must be a second difference (zero sum and zero
//! first moment): the noise model says nothing about offsets and slopes. Held to spyder_ref at 1e-9 by the parity
//! fixture (`tests/goldens/gains_parity_v1.json`).
//!
//! The same machinery gives the noise of the checks' linear statistics (DECISIONS 137 g2: the contaminant signs, the C1
//! excess, their reduced forms, the long-wave tail reading): [`LinearStat`], one detector term at a time
//! ([`stat_term_gain`]), so the per-sign gates, the DECISIONS 107 fallbacks, the long-wave cuts and the soft-tier
//! eligibility are decided on the noise AFTER the correction applied, in SD units.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::tables::{Bands, Kernel as CheckKernel, NoiseGains};
use super::{Node, PResult};
use crate::grid::{index_of, segment_ranges, WL_TOL};
use crate::model::{Body, Model, ModelKind, YTransform};
use crate::preprocess::savgol::try_sg_weights;
use crate::preprocess::{Op, PerSegment, RadiusRule, SgMode, SgParams};

/// The ASD LabSpec correlation length (nm) when neither the table nor the correction gives one.
pub const DEFAULT_CORRELATION_NM: f64 = 1.25;
/// The noise Gaussian is cut at +-8 sigma (tails below 1e-13).
pub const TAPS_SD: f64 = 8.0;
/// A segment part is a second difference when both moments vanish to this (relative).
pub const MOMENT_TOL: f64 = 1e-9;
const GRID_TOL: f64 = 1e-6;

/// No analytic gain for this output on this scan; the text is the record's status ("not assessed: ...").
#[derive(Debug, Clone, PartialEq)]
pub struct GainError(pub String);

fn nonlinear(why: impl std::fmt::Display) -> GainError {
    GainError(format!("not assessed: {why}"))
}

fn unavailable(why: impl std::fmt::Display) -> GainError {
    GainError(format!(
        "not assessed: not available on this instrument ({why})"
    ))
}

/// A wavelength as Python prints it in these messages: integral values without a decimal point.
fn nm(x: f64) -> String {
    if x.is_finite() && x == x.trunc() && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        crate::pyfmt::float_repr(x)
    }
}

/// What a correction file says about its instrument's noise (`provenance.instrument_noise`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstrumentNoise {
    /// The noise correlation length (nm) measured on this instrument's scans.
    pub correlation_nm: Option<f64>,
    /// Per-band C-H envelope factors measured on this instrument's scans (override bands.json defaults).
    pub ch_envelope_factors: BTreeMap<String, f64>,
}

/// Parse an `instrument_noise` block: optional `correlation_nm` (finite, >= 0) and `ch_envelope_factors`
/// ({band: positive finite factor}). Other keys (notes, provenance) are allowed and ignored.
pub fn parse_instrument_noise(n: &Node) -> PResult<InstrumentNoise> {
    n.obj()?;
    let correlation_nm = match n.opt("correlation_nm") {
        None => None,
        Some(c) => {
            let v = c.f64()?;
            if !(v >= 0.0) {
                return Err(c.error("correlation_nm must be a finite number >= 0"));
            }
            Some(v)
        }
    };
    let mut ch_envelope_factors = BTreeMap::new();
    if let Some(f) = n.opt("ch_envelope_factors") {
        for (b, v) in f.entries()? {
            let x = v.f64()?;
            if !(x > 0.0) {
                return Err(v.error(format!(
                    "ch_envelope_factors {b}: must be a positive finite number"
                )));
            }
            ch_envelope_factors.insert(b, x);
        }
    }
    Ok(InstrumentNoise {
        correlation_nm,
        ch_envelope_factors,
    })
}

/// A correction applied before an output: its operators and instrument noise. `None` = the scan as measured.
#[derive(Debug, Clone, Copy)]
pub struct Correction<'a> {
    pub ops: &'a [Op],
    pub noise: &'a InstrumentNoise,
    /// Identity for the cache (the transfer file's SHA-256, or a golden correction's name).
    pub key: &'a str,
}

/// The scan's layout: the grid step and the detector segments `[a, b)` of grid indices.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub step: f64,
    pub segs: Vec<(usize, usize)>,
}

/// The layout of a scan; the grid must be uniform.
pub fn layout(wl: &[f64], joins: &[f64]) -> Result<Layout, GainError> {
    if wl.len() < 3 {
        return Err(unavailable("the scan has fewer than 3 grid points"));
    }
    let step = wl[1] - wl[0];
    if !(step.is_finite() && step > 0.0)
        || wl
            .windows(2)
            .any(|p| ((p[1] - p[0]) - step).abs() > GRID_TOL)
    {
        return Err(unavailable("the scan's grid is not uniform"));
    }
    Ok(Layout {
        step,
        segs: segment_ranges(wl, joins),
    })
}

fn layout_key(wl: &[f64], lay: &Layout) -> String {
    let segs: Vec<String> = lay.segs.iter().map(|(a, b)| format!("{a}-{b}")).collect();
    format!(
        "{}|{:x}|{:x}|{}",
        wl.len(),
        wl[0].to_bits(),
        lay.step.to_bits(),
        segs.join(",")
    )
}

fn segment_of(segs: &[(usize, usize)], i: usize) -> Option<usize> {
    segs.iter().position(|&(a, b)| a <= i && i < b)
}

/// A linear functional on the measured absorbance of the scan grid: dense weights and its support, index ranges
/// `[lo, hi]` that must each lie inside one detector segment (an SG window, or a run of a folded component).
#[derive(Debug, Clone, PartialEq)]
pub struct Kernel {
    pub w: Vec<f64>,
    pub atoms: Vec<(usize, usize)>,
}

/// Spread output weights through one Savitzky-Golay step (interior centre weights only). `off` = the index of the
/// step's input grid start on the scan grid.
#[allow(clippy::too_many_arguments)]
fn sg_adjoint(
    beta: &[f64],
    grid_in: &[f64],
    p: &SgParams,
    map: &dyn Fn(usize) -> usize,
    off: usize,
    atoms: &mut Vec<(usize, usize)>,
    what: &str,
) -> Result<Vec<f64>, GainError> {
    let n_in = grid_in.len();
    let w = try_sg_weights(p.window, p.polyorder, p.deriv, p.delta)
        .map_err(|e| nonlinear(format!("{what}: {e}")))?;
    let c = &w.centre;
    let h = p.window / 2;
    let mut out = vec![0.0; n_in];
    for (o, &b) in beta.iter().enumerate() {
        if b == 0.0 {
            continue;
        }
        let i = map(o);
        if i < h || i + h > n_in - 1 {
            return Err(unavailable(format!(
                "{what}: the Savitzky-Golay window at {} nm runs off the scan's grid",
                nm(grid_in[i])
            )));
        }
        for (k, ck) in c.iter().enumerate() {
            out[i - h + k] += b * ck;
        }
        atoms.push((off + i - h, off + i + h));
    }
    Ok(out)
}

/// One step of a model chain after the absorbance: its output grid (offset into the scan grid) and index map.
struct Stage {
    off: usize,
    len: usize,
    shift: usize,
}

/// `[(component name or None, Kernel)]` of a model on this scan grid. Err = non-linear model or a feature off the
/// grid.
pub fn model_kernels(m: &Model, wl: &[f64]) -> Result<Vec<(Option<String>, Kernel)>, GainError> {
    let id = m.header.id.as_str();
    if !matches!(m.chain.first(), Some(Op::Absorbance { .. })) {
        return Err(nonlinear(
            "non-linear model (its chain does not start in absorbance)",
        ));
    }
    if m.kind == ModelKind::Regression && m.y_transform != YTransform::None {
        return Err(nonlinear("non-linear model (transformed output)"));
    }
    // forward pass: each step's output grid is a contiguous run of the scan grid
    let mut stages: Vec<Stage> = vec![Stage {
        off: 0,
        len: wl.len(),
        shift: 0,
    }];
    for op in &m.chain[1..] {
        let prev = stages.last().map(|s| (s.off, s.len)).unwrap_or((0, 0));
        match op {
            Op::SavGol(p) => {
                let h = p.window / 2;
                if p.mode == SgMode::Valid {
                    if prev.1 < 2 * h + 1 {
                        return Err(unavailable(format!(
                            "{id}: the Savitzky-Golay window is longer than the grid"
                        )));
                    }
                    stages.push(Stage {
                        off: prev.0 + h,
                        len: prev.1 - 2 * h,
                        shift: h,
                    });
                } else {
                    stages.push(Stage {
                        off: prev.0,
                        len: prev.1,
                        shift: 0,
                    });
                }
            }
            Op::Crop { lo, hi } => {
                let g = &wl[prev.0..prev.0 + prev.1];
                let keep: Vec<usize> = (0..g.len())
                    .filter(|&i| g[i] >= lo - WL_TOL && g[i] <= hi + WL_TOL)
                    .collect();
                let (Some(&a), Some(&b)) = (keep.first(), keep.last()) else {
                    return Err(unavailable("the model's crop leaves no grid points"));
                };
                stages.push(Stage {
                    off: prev.0 + a,
                    len: b - a + 1,
                    shift: a,
                });
            }
            other => {
                return Err(nonlinear(format!(
                    "non-linear model (operator {})",
                    op_name(other)
                )))
            }
        }
    }
    let last = stages.last().map(|s| (s.off, s.len)).unwrap_or((0, 0));
    let gout = &wl[last.0..last.0 + last.1];
    let back = |mut beta: Vec<f64>, atoms: &mut Vec<(usize, usize)>, what: &str| {
        for step in (1..m.chain.len()).rev() {
            let st = &stages[step];
            let prev = &stages[step - 1];
            let gin = &wl[prev.off..prev.off + prev.len];
            let shift = st.shift;
            beta = match &m.chain[step] {
                Op::SavGol(p) => sg_adjoint(&beta, gin, p, &|o| o + shift, prev.off, atoms, what)?,
                _ => {
                    let mut b2 = vec![0.0; gin.len()];
                    for (o, &b) in beta.iter().enumerate() {
                        if b != 0.0 {
                            b2[o + shift] += b;
                        }
                    }
                    b2
                }
            };
        }
        Ok::<Vec<f64>, GainError>(beta)
    };
    let place = |wls: &[f64], coefs: &[f64], what: &str| {
        let mut beta = vec![0.0; gout.len()];
        let mut idx = Vec::new();
        for (&lam, &b) in wls.iter().zip(coefs) {
            let i = index_of(gout, lam).ok_or_else(|| {
                unavailable(format!("{what}: {} nm is not on the scan's grid", nm(lam)))
            })?;
            beta[i] += b;
            idx.push(last.0 + i);
        }
        Ok::<(Vec<f64>, Vec<usize>), GainError>((beta, idx))
    };
    let mut out = Vec::new();
    match &m.body {
        Body::Regression { coefficients, .. } => {
            let mut atoms = Vec::new();
            let (beta, _) = place(&m.features_nm, coefficients, id)?;
            let w = back(beta, &mut atoms, id)?;
            out.push((None, Kernel { w, atoms }));
        }
        Body::Consensus { components } => {
            for c in components {
                let what = format!("{id} component {}", c.name);
                let mut atoms = Vec::new();
                let (beta, mut idx) = place(&c.wavelengths_nm, &c.weights, &what)?;
                let w = back(beta, &mut atoms, &what)?;
                // the component's runs (its SG steps are folded into the weights): each must sit in one segment
                idx.sort_unstable();
                idx.dedup();
                if let Some(&first) = idx.first() {
                    let mut run = (first, first);
                    for &i in &idx[1..] {
                        if i - run.1 <= 1 {
                            run.1 = i;
                        } else {
                            atoms.push(run);
                            run = (i, i);
                        }
                    }
                    atoms.push(run);
                }
                out.push((Some(c.name.clone()), Kernel { w, atoms }));
            }
        }
    }
    Ok(out)
}

fn op_name(op: &Op) -> &'static str {
    match op {
        Op::Absorbance { .. } => "absorbance",
        Op::SavGol(_) => "savgol",
        Op::Crop { .. } => "crop",
        Op::GaussianBlur { .. } => "gaussian_blur",
        Op::AbsorbanceAffine { .. } => "absorbance_affine",
        Op::BlockSnv { .. } => "snv",
    }
}

/// The plain band kernel: mean over centre +- half_width (the pipeline's inclusive window) of scale x SG(absorbance).
/// A projection is ignored (the shipped rule: the projected 1545 nm reading's SD is about 10% below this).
pub fn band_kernel(bands: &Bands, band: &str, wl: &[f64]) -> Result<Kernel, GainError> {
    let b = bands
        .bands
        .get(band)
        .ok_or_else(|| unavailable(format!("band {band} is not in the band table")))?;
    let k = bands
        .kernels
        .get(&b.kernel)
        .ok_or_else(|| unavailable(format!("band {band}: unknown kernel")))?;
    let h = b.half_width_nm as f64;
    let (Some(a), Some(z)) = (index_of(wl, b.centre_nm - h), index_of(wl, b.centre_nm + h)) else {
        return Err(unavailable(format!(
            "band {band}: {}-{} nm is not on the scan's grid",
            nm(b.centre_nm - h),
            nm(b.centre_nm + h)
        )));
    };
    if z < a {
        return Err(unavailable(format!("band {band}: empty reading window")));
    }
    let mut beta = vec![0.0; wl.len()];
    let n = (z - a + 1) as f64;
    for x in &mut beta[a..=z] {
        *x = k.scale / n;
    }
    let mut atoms = Vec::new();
    let w = sg_adjoint(
        &beta,
        wl,
        &k.savgol,
        &|o| o,
        0,
        &mut atoms,
        &format!("band {band}"),
    )?;
    Ok(Kernel { w, atoms })
}

fn per_seg(v: &PerSegment, s: usize, nseg: usize, op: &str, what: &str) -> Result<f64, GainError> {
    match v {
        PerSegment::Scalar(x) => Ok(*x),
        PerSegment::Segments(vals) => {
            if vals.len() != nseg {
                return Err(unavailable(format!(
                    "the correction's {op} gives {} {what} values; the scan has {nseg} segments",
                    vals.len()
                )));
            }
            Ok(vals[s])
        }
    }
}

/// A correction step the engine does not propagate noise through (only blur and absorbance gain are linear and
/// supported; block SNV is non-linear).
fn no_propagation(op: &Op) -> GainError {
    nonlinear(format!(
        "no noise propagation through the correction's {} step",
        op_name(op)
    ))
}

/// Err when the correction has an operator without an analytic gain.
pub fn check_linear(ops: Option<&[Op]>) -> Result<(), GainError> {
    for op in ops.unwrap_or(&[]) {
        match op {
            Op::GaussianBlur { .. } | Op::AbsorbanceAffine { .. } => {}
            Op::BlockSnv { .. } => return Err(nonlinear("non-linear correction")),
            other => return Err(no_propagation(other)),
        }
    }
    Ok(())
}

/// A segment part (dense over the grid, nonzero only in `[a, b)`) through the adjoint of the correction.
fn back_correction(
    mut h: Vec<f64>,
    a: usize,
    b: usize,
    wl: &[f64],
    s: usize,
    nseg: usize,
    ops: &[Op],
) -> Result<Vec<f64>, GainError> {
    let step = wl[1] - wl[0];
    for op in ops.iter().rev() {
        match op {
            Op::AbsorbanceAffine { gain, .. } => {
                let g = per_seg(gain, s, nseg, "absorbance_affine", "gain")?;
                for x in &mut h {
                    *x *= g;
                }
            }
            Op::GaussianBlur {
                sigma_nm,
                truncate,
                radius,
            } => {
                let sg = per_seg(sigma_nm, s, nseg, "gaussian_blur", "sigma")?;
                if sg <= 0.0 {
                    continue;
                }
                let rf = match radius {
                    RadiusRule::Ceil => (truncate * sg / step).ceil(),
                    RadiusRule::Round => (truncate * sg / step + 0.5).floor(),
                };
                let r = if rf.is_finite() && rf >= 0.0 {
                    rf.min(wl.len() as f64) as usize
                } else {
                    return Err(no_propagation(op));
                };
                let mut out = vec![0.0; h.len()];
                for i in a..b {
                    let hi_ = h[i];
                    if hi_ == 0.0 {
                        continue;
                    }
                    let lo = a.max(i.saturating_sub(r));
                    let hi = b.min(i + r + 1);
                    let t: Vec<f64> = (lo..hi)
                        .map(|j| {
                            let z = (wl[j] - wl[i]) / sg;
                            (-0.5 * (z * z)).exp()
                        })
                        .collect();
                    let sum: f64 = t.iter().sum();
                    for (j, tj) in (lo..hi).zip(&t) {
                        out[j] += hi_ * tj / sum;
                    }
                }
                h = out;
            }
            Op::BlockSnv { .. } => return Err(nonlinear("non-linear correction")),
            other => return Err(no_propagation(other)),
        }
    }
    Ok(h)
}

/// The noise Gaussian's taps (unnormalised) for a correlation length and grid step.
pub fn noise_taps(correlation_nm: f64, step: f64) -> Vec<f64> {
    let sd = correlation_nm / step;
    if !(sd > 0.0) || !sd.is_finite() {
        return vec![1.0];
    }
    let r = (TAPS_SD * sd).ceil() as i64;
    (-r..=r)
        .map(|t| {
            let z = t as f64 / sd;
            (-0.5 * (z * z)).exp()
        })
        .collect()
}

fn convolve(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

fn var_ratio(h: &[f64], g: &[f64]) -> f64 {
    let u = convolve(h, g);
    let v = convolve(&[1.0, -2.0, 1.0], g);
    let uu: f64 = u.iter().map(|x| x * x).sum();
    let vv: f64 = v.iter().map(|x| x * x).sum();
    uu / vv
}

/// `[(segment index, gain per unit N2)]` of a kernel on a scan's layout through a correction (None = none).
pub fn segment_gains(
    k: &Kernel,
    wl: &[f64],
    lay: &Layout,
    ops: Option<&[Op]>,
    correlation_nm: f64,
    what: &str,
) -> Result<Vec<(usize, f64)>, GainError> {
    check_linear(ops)?;
    let segs = &lay.segs;
    for &(lo, hi) in &k.atoms {
        let (s0, s1) = (segment_of(segs, lo), segment_of(segs, hi));
        let (Some(s0), Some(s1)) = (s0, s1) else {
            return Err(unavailable(format!(
                "{what}: {}-{} nm is outside the scan",
                nm(wl[lo.min(wl.len() - 1)]),
                nm(wl[hi.min(wl.len() - 1)])
            )));
        };
        if s0 != s1 {
            let j = wl[segs[s0].1 - 1];
            return Err(unavailable(format!(
                "{what}: {}-{} nm crosses the detector join at {} nm",
                nm(wl[lo]),
                nm(wl[hi]),
                nm(j)
            )));
        }
    }
    let g = noise_taps(correlation_nm, lay.step);
    let mut out = Vec::new();
    for (s, &(a, b)) in segs.iter().enumerate() {
        let Some(first) = (a..b).find(|&i| k.w[i] != 0.0) else {
            continue;
        };
        let last = (a..b).rev().find(|&i| k.w[i] != 0.0).unwrap_or(first);
        let p = &k.w[first..=last];
        let mid = (p.len() as f64 - 1.0) / 2.0;
        let (mut m0, mut a0, mut m1, mut a1) = (0.0, 0.0, 0.0, 0.0);
        for (i, &x) in p.iter().enumerate() {
            let t = i as f64 - mid;
            m0 += x;
            a0 += x.abs();
            m1 += t * x;
            a1 += (t * x).abs();
        }
        if m0.abs() > MOMENT_TOL * a0 || m1.abs() > MOMENT_TOL * a1 {
            return Err(nonlinear(format!(
                "{what} is not a second-derivative reading (the noise model does not apply)"
            )));
        }
        let mut part = vec![0.0; wl.len()];
        part[a..b].copy_from_slice(&k.w[a..b]);
        let h = match ops {
            Some(o) => back_correction(part, a, b, wl, s, segs.len(), o)?,
            None => part,
        };
        let Some(f) = h.iter().position(|&x| x != 0.0) else {
            continue;
        };
        let l = h.iter().rposition(|&x| x != 0.0).unwrap_or(f);
        out.push((s, 1e-5 * var_ratio(&h[f..=l], &g).sqrt()));
    }
    if out.is_empty() {
        return Err(unavailable(format!("{what}: no weight on the scan")));
    }
    Ok(out)
}

/// True when the scan's grid holds the WHOLE N2 window: every centre lo <= wl < hi and both neighbours its second
/// difference reads (a grid point below lo and one at or above hi). A window the grid only partly covers has fewer
/// second differences than the noise measure assumes (one centre gives a zero MAD): not available, never "quiet".
pub fn window_covered(wl: &[f64], lo: f64, hi: f64) -> bool {
    let n = wl.len();
    n >= 3 && wl[0] < lo && wl[n - 1] >= hi && wl.iter().any(|&w| w >= lo && w < hi)
}

/// The segment holding every grid point an N2 window reads (centres lo <= wl < hi and their neighbours); None also
/// when the grid does not cover the whole window.
pub fn window_segment(wl: &[f64], segs: &[(usize, usize)], lo: f64, hi: f64) -> Option<usize> {
    if !window_covered(wl, lo, hi) {
        return None;
    }
    let c: Vec<usize> = (0..wl.len())
        .filter(|&i| wl[i] >= lo && wl[i] < hi)
        .collect();
    let (first, last) = (*c.first()?, *c.last()?);
    let s0 = segment_of(segs, first - 1)?;
    let s1 = segment_of(segs, last + 1)?;
    (s0 == s1).then_some(s0)
}

/// The correlation length for an output: the correction's instrument value, else the table's.
pub fn correlation_for(table_nm: f64, corr: Option<Correction>) -> f64 {
    corr.and_then(|c| c.noise.correlation_nm)
        .unwrap_or(table_nm)
}

/// One model's terms: per component (None for a regression) the (N2 window, gain) of each detector part.
pub type ModelTerms = Vec<(Option<String>, Vec<((f64, f64), f64)>)>;

/// The terms of a model through a correction.
pub fn model_terms(
    ng: &NoiseGains,
    m: &Model,
    wl: &[f64],
    joins: &[f64],
    corr: Option<Correction>,
) -> Result<ModelTerms, GainError> {
    let lay = layout(wl, joins)?;
    let id = m.header.id.as_str();
    if let Some(rec) = ng.models.get(id).and_then(|s| s.recorded.as_ref()) {
        if corr.is_some() {
            return Err(nonlinear(
                "non-linear model (its recorded gain holds for the scan as measured only)",
            ));
        }
        let mut terms = Vec::new();
        for t in rec {
            if !window_covered(wl, t.window_nm.0, t.window_nm.1) {
                return Err(unavailable(format!(
                    "{id}: the N2 window {}-{} nm is not covered by the scan",
                    nm(t.window_nm.0),
                    nm(t.window_nm.1)
                )));
            }
            if window_segment(wl, &lay.segs, t.window_nm.0, t.window_nm.1).is_none() {
                return Err(unavailable(format!(
                    "{id}: the N2 window {}-{} nm is not inside one detector segment",
                    nm(t.window_nm.0),
                    nm(t.window_nm.1)
                )));
            }
            terms.push((t.window_nm, t.gain));
        }
        return Ok(vec![(None, terms)]);
    }
    let cn = correlation_for(ng.correlation_nm, corr);
    let ops = corr.map(|c| c.ops);
    let mut out = Vec::new();
    for (name, k) in model_kernels(m, wl)? {
        let what = match &name {
            None => id.to_string(),
            Some(n) => format!("{id} component {n}"),
        };
        let mut terms = Vec::new();
        for (s, gain) in segment_gains(&k, wl, &lay, ops, cn, &what)? {
            let win = ng
                .model_noise_windows_nm
                .iter()
                .copied()
                .find(|w| window_segment(wl, &lay.segs, w.0, w.1) == Some(s));
            let Some(win) = win else {
                let (a, b) = lay.segs[s];
                return Err(unavailable(format!(
                    "{what}: no noise window in the detector segment {}-{} nm",
                    nm(wl[a]),
                    nm(wl[b - 1])
                )));
            };
            terms.push((win, gain));
        }
        out.push((name, terms));
    }
    Ok(out)
}

/// (N2 window, gain_E_per_N2 x envelope factor) of a band with a readability block.
pub fn band_gain(
    bands: &Bands,
    band: &str,
    wl: &[f64],
    joins: &[f64],
    corr: Option<Correction>,
) -> Result<((f64, f64), f64), GainError> {
    let r = bands
        .bands
        .get(band)
        .and_then(|b| b.readability.as_ref())
        .ok_or_else(|| unavailable(format!("band {band} has no readability block")))?;
    let lay = layout(wl, joins)?;
    let k = band_kernel(bands, band, wl)?;
    let sg = segment_gains(
        &k,
        wl,
        &lay,
        corr.map(|c| c.ops),
        correlation_for(bands.correlation_nm, corr),
        &format!("band {band}"),
    )?;
    if sg.len() != 1 {
        return Err(unavailable(format!(
            "band {band}: its kernel spans more than one detector segment"
        )));
    }
    let (s, gain) = sg[0];
    let (lo, hi) = r.noise_window_nm;
    if !window_covered(wl, lo, hi) {
        return Err(unavailable(format!(
            "band {band}: the N2 window {}-{} nm is not covered by the scan",
            nm(lo),
            nm(hi)
        )));
    }
    if window_segment(wl, &lay.segs, lo, hi) != Some(s) {
        return Err(unavailable(format!(
            "band {band}: the N2 window {}-{} nm is not in the band's detector segment",
            nm(lo),
            nm(hi)
        )));
    }
    let fac = corr
        .and_then(|c| c.noise.ch_envelope_factors.get(band).copied())
        .unwrap_or(r.envelope_factor);
    Ok(((lo, hi), gain * fac))
}

/// A linear statistic of a check (DECISIONS 137 g2): the sum over `windows` of weight x the mean of E over [lo, hi]
/// (inclusive, on the grid), E = scale x Savitzky-Golay(absorbance) with the check's kernel. A sharp index, a plaster
/// load, the C1 excess and its variants and a band reading are all of this form (their offsets do not matter for noise).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearStat {
    pub kernel: CheckKernel,
    /// (lo nm, hi nm, weight)
    pub windows: Vec<(f64, f64, f64)>,
}

/// The absorbance kernel of a linear statistic on a scan grid.
pub fn stat_kernel(st: &LinearStat, wl: &[f64], what: &str) -> Result<Kernel, GainError> {
    let mut beta = vec![0.0; wl.len()];
    for &(lo, hi, w) in &st.windows {
        let (Some(a), Some(z)) = (index_of(wl, lo), index_of(wl, hi)) else {
            return Err(unavailable(format!(
                "{what}: {}-{} nm is not on the scan's grid",
                nm(lo),
                nm(hi)
            )));
        };
        if z < a {
            return Err(unavailable(format!(
                "{what}: empty window {}-{} nm",
                nm(lo),
                nm(hi)
            )));
        }
        let n = (z - a + 1) as f64;
        let v = st.kernel.scale * w / n;
        for x in &mut beta[a..=z] {
            *x += v;
        }
    }
    let mut atoms = Vec::new();
    let w = sg_adjoint(&beta, wl, &st.kernel.savgol, &|o| o, 0, &mut atoms, what)?;
    Ok(Kernel { w, atoms })
}

/// The SD per unit N2 (as-measured, in the window `win`) of ONE detector term of a linear statistic: its part in the
/// detector segment that holds the N2 window, through the correction (None = the scan as measured). The window must be
/// on the grid whole (with the neighbours its second difference reads) and inside one segment, and the statistic must
/// have weight there; otherwise "not available on this instrument". A non-linear correction: "non-linear correction".
pub fn stat_term_gain(
    st: &LinearStat,
    wl: &[f64],
    joins: &[f64],
    corr: Option<Correction>,
    correlation_nm: f64,
    win: (f64, f64),
    what: &str,
) -> Result<f64, GainError> {
    let lay = layout(wl, joins)?;
    let (lo, hi) = win;
    if !window_covered(wl, lo, hi) {
        return Err(unavailable(format!(
            "{what}: the N2 window {}-{} nm is not covered by the scan",
            nm(lo),
            nm(hi)
        )));
    }
    let Some(s) = window_segment(wl, &lay.segs, lo, hi) else {
        return Err(unavailable(format!(
            "{what}: the N2 window {}-{} nm is not inside one detector segment",
            nm(lo),
            nm(hi)
        )));
    };
    let k = stat_kernel(st, wl, what)?;
    let sg = segment_gains(
        &k,
        wl,
        &lay,
        corr.map(|c| c.ops),
        correlation_for(correlation_nm, corr),
        what,
    )?;
    sg.iter()
        .find(|(seg, _)| *seg == s)
        .map(|(_, g)| *g)
        .ok_or_else(|| {
            unavailable(format!(
                "{what}: no weight in the detector segment of the N2 window {}-{} nm",
                nm(lo),
                nm(hi)
            ))
        })
}

/// sqrt(sum (gain x N2(window))^2); None when an N2 is not finite (noise unknown).
pub fn rss(terms: &[((f64, f64), f64)], n2_of: &mut dyn FnMut(f64, f64) -> f64) -> Option<f64> {
    let mut s = 0.0;
    for &((lo, hi), g) in terms {
        let v = n2_of(lo, hi);
        if !v.is_finite() {
            return None;
        }
        s += (g * v) * (g * v);
    }
    Some(s.sqrt())
}

#[derive(Debug, Clone)]
enum Cached {
    Model(Result<ModelTerms, GainError>),
    Band(Result<((f64, f64), f64), GainError>),
    Stat(Result<f64, GainError>),
}

/// Computed gains, per output x correction x scan layout (the registry holds one for its lifetime: the gains of a
/// loaded plug-in set never change).
#[derive(Clone, Default)]
pub struct GainCache(Arc<Mutex<BTreeMap<String, Cached>>>);

impl std::fmt::Debug for GainCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.0.lock().map(|m| m.len()).unwrap_or(0);
        write!(f, "GainCache({n} entries)")
    }
}

impl GainCache {
    fn key(kind: &str, what: &str, wl: &[f64], joins: &[f64], corr: Option<Correction>) -> String {
        let lay = layout(wl, joins)
            .map(|l| layout_key(wl, &l))
            .unwrap_or_else(|_| format!("bad-grid-{}", wl.len()));
        format!(
            "{kind}|{what}|{}|{lay}",
            corr.map(|c| c.key).unwrap_or("none")
        )
    }

    /// [`model_terms`], cached by model id@version, correction and layout.
    pub fn model_terms(
        &self,
        ng: &NoiseGains,
        m: &Model,
        wl: &[f64],
        joins: &[f64],
        corr: Option<Correction>,
    ) -> Result<ModelTerms, GainError> {
        let k = Self::key("model", &m.key(), wl, joins, corr);
        if let Ok(map) = self.0.lock() {
            if let Some(Cached::Model(r)) = map.get(&k) {
                return r.clone();
            }
        }
        let r = model_terms(ng, m, wl, joins, corr);
        if let Ok(mut map) = self.0.lock() {
            map.insert(k, Cached::Model(r.clone()));
        }
        r
    }

    /// [`band_gain`], cached by band, correction and layout.
    pub fn band_gain(
        &self,
        bands: &Bands,
        band: &str,
        wl: &[f64],
        joins: &[f64],
        corr: Option<Correction>,
    ) -> Result<((f64, f64), f64), GainError> {
        let k = Self::key("band", band, wl, joins, corr);
        if let Ok(map) = self.0.lock() {
            if let Some(Cached::Band(r)) = map.get(&k) {
                return r.clone();
            }
        }
        let r = band_gain(bands, band, wl, joins, corr);
        if let Ok(mut map) = self.0.lock() {
            map.insert(k, Cached::Band(r.clone()));
        }
        r
    }

    /// [`stat_term_gain`], cached by the statistic's name (`what`: unique per check file and statistic), the N2
    /// window, the correction and the layout.
    #[allow(clippy::too_many_arguments)]
    pub fn stat_term_gain(
        &self,
        st: &LinearStat,
        wl: &[f64],
        joins: &[f64],
        corr: Option<Correction>,
        correlation_nm: f64,
        win: (f64, f64),
        what: &str,
    ) -> Result<f64, GainError> {
        let tag = format!(
            "{what}|{:x}-{:x}|{:x}",
            win.0.to_bits(),
            win.1.to_bits(),
            correlation_nm.to_bits()
        );
        let k = Self::key("stat", &tag, wl, joins, corr);
        if let Ok(map) = self.0.lock() {
            if let Some(Cached::Stat(r)) = map.get(&k) {
                return r.clone();
            }
        }
        let r = stat_term_gain(st, wl, joins, corr, correlation_nm, win, what);
        if let Ok(mut map) = self.0.lock() {
            map.insert(k, Cached::Stat(r.clone()));
        }
        r
    }

    pub fn len(&self) -> usize {
        self.0.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
