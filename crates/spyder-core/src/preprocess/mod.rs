//! The v1 preprocessing operator subset, engine 1.1 (PLAN section 4; 04 section 3; spyder_ref `op_*`).
//!
//! Supported: `absorbance` (declared clip), `savgol` (all spyder_ref edge modes), `crop` and feature selection,
//! segmented `gaussian_blur` (sigma per segment, never crossing a join), `absorbance_affine` (scalar or per
//! segment), block `snv` (`blocks_nm`). Reserved operators (whole-grid SNV, MSC, detrend, resample,
//! splice_correct, pds_banded, affine_reflectance) return [`OpError::UnsupportedOperator`], never a guess.
//! All maths is float64; operators are pure and act on one spectrum on a strictly increasing grid.

pub mod savgol;
mod sg_table;

use serde_json::{Map, Value};
use thiserror::Error;

use crate::grid::{segment_ranges, uniform_step, WL_TOL};
use crate::numsum::{np_mean, np_std, np_sum};
pub use savgol::{SgMode, SgWeights};

/// Engine (operator-set) version this crate implements.
pub const ENGINE_VERSION: (u32, u32) = (1, 1);

/// Operators reserved in the format but not in the v1 subset.
pub const RESERVED_OPERATORS: [&str; 7] = [
    "snv (whole grid)",
    "msc",
    "detrend",
    "resample",
    "splice_correct",
    "pds_banded",
    "affine_reflectance",
];

#[derive(Debug, Clone, PartialEq, Error)]
pub enum OpError {
    /// A reserved operator (or a reserved form of a v1 operator): typed, never approximated.
    #[error("unsupported operator {op}: {detail}")]
    UnsupportedOperator { op: String, detail: String },
    /// An operator name the format does not define.
    #[error("unknown operator {op:?}")]
    UnknownOperator { op: String },
    /// Missing, malformed or out-of-range parameters.
    #[error("invalid parameters for {op}: {reason}")]
    InvalidParameters { op: String, reason: String },
    /// The operator cannot run on this spectrum (e.g. an SNV block spanning this scan's join).
    #[error("{op} failed: {reason}")]
    Failed { op: String, reason: String },
}

fn invalid(op: &str, reason: impl Into<String>) -> OpError {
    OpError::InvalidParameters {
        op: op.to_string(),
        reason: reason.into(),
    }
}

fn failed(op: &str, reason: impl Into<String>) -> OpError {
    OpError::Failed {
        op: op.to_string(),
        reason: reason.into(),
    }
}

/// A spectrum on its grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    pub wl: Vec<f64>,
    pub x: Vec<f64>,
}

impl Spectrum {
    pub fn new(wl: Vec<f64>, x: Vec<f64>) -> Self {
        Spectrum { wl, x }
    }
}

/// A value given once for the whole grid or once per detector segment.
#[derive(Debug, Clone, PartialEq)]
pub enum PerSegment {
    Scalar(f64),
    Segments(Vec<f64>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadiusRule {
    /// ceil(truncate * sigma / step) (transfer agent, 08_transfer.md).
    Ceil,
    /// floor(truncate * sigma / step + 0.5) (scipy gaussian_filter1d).
    Round,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SgParams {
    pub window: usize,
    pub polyorder: usize,
    pub deriv: usize,
    pub delta: f64,
    pub mode: SgMode,
}

/// One parsed v1 operator.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Absorbance {
        clip_min: f64,
    },
    SavGol(SgParams),
    Crop {
        lo: f64,
        hi: f64,
    },
    GaussianBlur {
        sigma_nm: PerSegment,
        truncate: f64,
        radius: RadiusRule,
    },
    AbsorbanceAffine {
        gain: PerSegment,
        offset: PerSegment,
        clip_min: f64,
    },
    BlockSnv {
        ddof: usize,
        blocks_nm: Vec<(f64, f64)>,
    },
}

// ------------------------------------------------------------------------------------------- parsing
/// Keys every operator may carry besides its parameters.
const COMMON_KEYS: [&str; 2] = ["op", "note"];

fn check_keys(op: &str, m: &Map<String, Value>, allowed: &[&str]) -> Result<(), OpError> {
    for k in m.keys() {
        if !COMMON_KEYS.contains(&k.as_str()) && !allowed.contains(&k.as_str()) {
            return Err(invalid(op, format!("unexpected key {k:?}")));
        }
    }
    Ok(())
}

fn finite_num(op: &str, m: &Map<String, Value>, key: &str) -> Result<f64, OpError> {
    let v = m
        .get(key)
        .ok_or_else(|| invalid(op, format!("missing {key:?}")))?;
    let x = v
        .as_f64()
        .ok_or_else(|| invalid(op, format!("{key:?} must be a number")))?;
    if !x.is_finite() {
        return Err(invalid(op, format!("{key:?} must be finite")));
    }
    Ok(x)
}

fn uint(op: &str, m: &Map<String, Value>, key: &str) -> Result<usize, OpError> {
    let v = m
        .get(key)
        .ok_or_else(|| invalid(op, format!("missing {key:?}")))?;
    if let Some(u) = v.as_u64() {
        return usize::try_from(u).map_err(|_| invalid(op, format!("{key:?} too large")));
    }
    // integral floats (e.g. 31.0) are accepted as Python's int() would
    match v.as_f64() {
        Some(f) if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 1e9 => Ok(f as usize),
        _ => Err(invalid(
            op,
            format!("{key:?} must be a non-negative integer"),
        )),
    }
}

fn per_segment(op: &str, v: &Value, key: &str) -> Result<PerSegment, OpError> {
    if let Some(x) = v.as_f64() {
        if !x.is_finite() {
            return Err(invalid(op, format!("{key:?} must be finite")));
        }
        return Ok(PerSegment::Scalar(x));
    }
    let arr = v
        .as_array()
        .ok_or_else(|| invalid(op, format!("{key:?} must be a number or a list")))?;
    if arr.is_empty() {
        return Err(invalid(op, format!("{key:?} list is empty")));
    }
    let mut out = Vec::with_capacity(arr.len());
    for e in arr {
        match e.as_f64() {
            Some(x) if x.is_finite() => out.push(x),
            _ => {
                return Err(invalid(
                    op,
                    format!("{key:?} values must be finite numbers"),
                ))
            }
        }
    }
    Ok(PerSegment::Segments(out))
}

fn unsupported(op: &str, detail: impl Into<String>) -> OpError {
    OpError::UnsupportedOperator {
        op: op.to_string(),
        detail: detail.into(),
    }
}

/// Parse one operator object, e.g. `{"op": "savgol", "window": 31, ...}`.
pub fn parse_op(v: &Value) -> Result<Op, OpError> {
    let m = v
        .as_object()
        .ok_or_else(|| invalid("?", "an operator must be a JSON object"))?;
    let op = m
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("?", "missing \"op\" name"))?;
    match op {
        "absorbance" => {
            check_keys(op, m, &["clip_min"])?;
            let clip_min = finite_num(op, m, "clip_min")?;
            if clip_min <= 0.0 {
                return Err(invalid(op, "clip_min must be > 0"));
            }
            Ok(Op::Absorbance { clip_min })
        }
        "savgol" => {
            check_keys(op, m, &["window", "polyorder", "deriv", "delta", "mode"])?;
            let window = uint(op, m, "window")?;
            let polyorder = uint(op, m, "polyorder")?;
            let deriv = uint(op, m, "deriv")?;
            let delta = finite_num(op, m, "delta")?;
            let mode_s = m.get("mode").and_then(Value::as_str).ok_or_else(|| {
                invalid(op, "missing \"mode\" (the edge mode is part of the model)")
            })?;
            let mode = SgMode::parse(mode_s)
                .ok_or_else(|| invalid(op, format!("unknown SG mode {mode_s:?}")))?;
            if window % 2 != 1 || window <= polyorder {
                return Err(invalid(op, "SG window must be odd and > polyorder"));
            }
            if window < 3 {
                return Err(invalid(op, "SG window must be >= 3"));
            }
            if deriv > polyorder {
                return Err(invalid(op, "deriv must be <= polyorder"));
            }
            if polyorder > 10 {
                return Err(invalid(op, "polyorder above 10 is not supported"));
            }
            if delta <= 0.0 {
                return Err(invalid(op, "delta must be > 0"));
            }
            if savgol::divisor(window / 2, deriv, delta).is_none() {
                return Err(invalid(
                    op,
                    "derivative scale h^deriv * delta^deriv is not a finite number > 0",
                ));
            }
            Ok(Op::SavGol(SgParams {
                window,
                polyorder,
                deriv,
                delta,
                mode,
            }))
        }
        "crop" => {
            check_keys(op, m, &["range_nm"])?;
            let r = m
                .get("range_nm")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid(op, "range_nm must be [lo, hi]"))?;
            let (lo, hi) = match r.as_slice() {
                [a, b] => match (a.as_f64(), b.as_f64()) {
                    (Some(a), Some(b)) if a.is_finite() && b.is_finite() && a <= b => (a, b),
                    _ => return Err(invalid(op, "range_nm must be finite with lo <= hi")),
                },
                _ => return Err(invalid(op, "range_nm must be [lo, hi]")),
            };
            Ok(Op::Crop { lo, hi })
        }
        "gaussian_blur" => {
            if m.contains_key("fwhm_nm") {
                return Err(unsupported(
                    op,
                    "the fwhm_nm form is not in the v1 subset (use sigma_nm)",
                ));
            }
            check_keys(op, m, &["sigma_nm", "truncate", "radius", "segments"])?;
            match m.get("segments").map(|s| s.as_str()) {
                None | Some(Some("from_scan")) => {}
                Some(Some("none")) => {
                    return Err(unsupported(
                        op,
                        "segments \"none\" would blur across detector joins (not in the v1 subset)",
                    ))
                }
                _ => return Err(invalid(op, "segments must be \"from_scan\"")),
            }
            let sigma_nm = per_segment(
                op,
                m.get("sigma_nm")
                    .ok_or_else(|| invalid(op, "missing \"sigma_nm\""))?,
                "sigma_nm",
            )?;
            let truncate = match m.get("truncate") {
                None => 4.0,
                Some(_) => finite_num(op, m, "truncate")?,
            };
            if truncate <= 0.0 {
                return Err(invalid(op, "truncate must be > 0"));
            }
            let radius = match m.get("radius").and_then(Value::as_str) {
                Some("ceil") => RadiusRule::Ceil,
                Some("round") => RadiusRule::Round,
                _ => {
                    return Err(invalid(
                        op,
                        "radius must be stated: \"ceil\" (v1 transfers) or \"round\"",
                    ))
                }
            };
            Ok(Op::GaussianBlur {
                sigma_nm,
                truncate,
                radius,
            })
        }
        "absorbance_affine" => {
            check_keys(op, m, &["gain", "offset", "clip_min"])?;
            let gain = per_segment(
                op,
                m.get("gain")
                    .ok_or_else(|| invalid(op, "missing \"gain\""))?,
                "gain",
            )?;
            let offset = match m.get("offset") {
                None => PerSegment::Scalar(0.0),
                Some(v) => per_segment(op, v, "offset")?,
            };
            let clip_min = finite_num(op, m, "clip_min")?;
            if clip_min <= 0.0 {
                return Err(invalid(op, "clip_min must be > 0"));
            }
            Ok(Op::AbsorbanceAffine {
                gain,
                offset,
                clip_min,
            })
        }
        "snv" => {
            let Some(blocks) = m.get("blocks_nm") else {
                return Err(unsupported(
                    op,
                    "whole-grid SNV (no blocks_nm) is reserved; v1 supports block SNV only",
                ));
            };
            check_keys(op, m, &["ddof", "blocks_nm"])?;
            let ddof = match m.get("ddof") {
                None => 0,
                Some(_) => uint(op, m, "ddof")?,
            };
            let arr = blocks
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or_else(|| invalid(op, "snv blocks_nm must be a non-empty list of [lo, hi]"))?;
            let mut out = Vec::new();
            let mut prev = f64::NEG_INFINITY;
            for b in arr {
                let pair = b.as_array().map(|p| p.as_slice());
                let (lo, hi) = match pair {
                    Some([a, c]) => match (a.as_f64(), c.as_f64()) {
                        (Some(a), Some(c)) if a.is_finite() && c.is_finite() => (a, c),
                        _ => return Err(invalid(op, format!("snv block {b} is not [lo, hi]"))),
                    },
                    _ => return Err(invalid(op, format!("snv block {b} is not [lo, hi]"))),
                };
                if lo >= hi {
                    return Err(invalid(op, format!("snv block {lo}-{hi}: lo must be < hi")));
                }
                if lo <= prev + WL_TOL {
                    return Err(invalid(
                        op,
                        "snv blocks must be sorted and must not overlap",
                    ));
                }
                prev = hi;
                out.push((lo, hi));
            }
            Ok(Op::BlockSnv {
                ddof,
                blocks_nm: out,
            })
        }
        "msc" | "detrend" | "resample" | "splice_correct" | "pds_banded" | "affine_reflectance" => {
            Err(unsupported(op, "reserved operator, not in the v1 subset"))
        }
        other => Err(OpError::UnknownOperator {
            op: other.to_string(),
        }),
    }
}

/// Parse a JSON list of operators.
pub fn parse_chain(v: &Value) -> Result<Vec<Op>, OpError> {
    v.as_array()
        .ok_or_else(|| invalid("chain", "a chain must be a JSON list"))?
        .iter()
        .map(parse_op)
        .collect()
}

// ------------------------------------------------------------------------------------------- operators
/// numpy `np.maximum(x, c)`: NaN propagates.
fn np_maximum(x: f64, c: f64) -> f64 {
    if x.is_nan() || x >= c {
        x
    } else {
        c
    }
}

/// A = log10(1 / max(R, clip_min)), written exactly so (spyder_ref `op_absorbance`).
pub fn absorbance(x: &[f64], clip_min: f64) -> Vec<f64> {
    x.iter()
        .map(|&r| (1.0 / np_maximum(r, clip_min)).log10())
        .collect()
}

/// Keep lo <= wl <= hi (+/- 1e-6), inclusive.
pub fn crop(s: &Spectrum, lo: f64, hi: f64) -> Result<Spectrum, OpError> {
    let keep: Vec<usize> = (0..s.wl.len())
        .filter(|&i| s.wl[i] >= lo - WL_TOL && s.wl[i] <= hi + WL_TOL)
        .collect();
    if keep.is_empty() {
        return Err(failed("crop", "crop leaves no points"));
    }
    Ok(Spectrum {
        wl: keep.iter().map(|&i| s.wl[i]).collect(),
        x: keep.iter().map(|&i| s.x[i]).collect(),
    })
}

/// Feature selection: the values at exactly these wavelengths (each must be on the grid within 1e-6 nm).
pub fn select_wavelengths(s: &Spectrum, wavelengths_nm: &[f64]) -> Result<Vec<f64>, OpError> {
    wavelengths_nm
        .iter()
        .map(|&w| {
            crate::grid::index_of(&s.wl, w)
                .map(|i| s.x[i])
                .ok_or_else(|| failed("select", format!("wavelength {w} nm not on the grid")))
        })
        .collect()
}

fn per_channel(
    op: &str,
    v: &PerSegment,
    segs: &[(usize, usize)],
    n: usize,
) -> Result<Vec<f64>, OpError> {
    match v {
        PerSegment::Scalar(x) => Ok(vec![*x; n]),
        PerSegment::Segments(vals) => {
            if vals.len() != segs.len() {
                return Err(failed(
                    op,
                    format!(
                        "{} per-segment values, the scan has {} segments",
                        vals.len(),
                        segs.len()
                    ),
                ));
            }
            let mut out = vec![0.0; n];
            for (&(a, b), &x) in segs.iter().zip(vals) {
                out[a..b].fill(x);
            }
            Ok(out)
        }
    }
}

/// Savitzky-Golay on one spectrum (spyder_ref `op_savgol`).
pub fn savgol(s: &Spectrum, p: &SgParams) -> Result<Spectrum, OpError> {
    let n = s.x.len();
    if p.window > n {
        return Err(failed("savgol", "SG window longer than spectrum"));
    }
    let w = savgol::try_sg_weights(p.window, p.polyorder, p.deriv, p.delta)
        .map_err(|e| failed("savgol", e))?;
    let (y, keep) = savgol::apply(&s.x, &w, p.mode);
    Ok(Spectrum {
        wl: s.wl[keep].to_vec(),
        x: y,
    })
}

/// Segmented Gaussian blur (spyder_ref `op_gaussian_blur`, sigma form): kernel centred on each output
/// channel, taps only inside that channel's detector segment, renormalised there; sigma <= 0 copies.
pub fn gaussian_blur(
    s: &Spectrum,
    joins: &[f64],
    sigma_nm: &PerSegment,
    truncate: f64,
    radius: RadiusRule,
) -> Result<Spectrum, OpError> {
    let op = "gaussian_blur";
    let n = s.wl.len();
    let step =
        uniform_step(&s.wl).ok_or_else(|| failed(op, "gaussian_blur needs a uniform grid"))?;
    let segs = segment_ranges(&s.wl, joins);
    let sig = per_channel(op, sigma_nm, &segs, n)?;
    let mut y = s.x.clone();
    for &(a, b) in &segs {
        for i in a..b {
            let sg = sig[i];
            if sg <= 0.0 {
                continue;
            }
            let rf = match radius {
                RadiusRule::Ceil => (truncate * sg / step).ceil(),
                RadiusRule::Round => (truncate * sg / step + 0.5).floor(),
            };
            if !rf.is_finite() {
                return Err(failed(op, "kernel radius is not finite"));
            }
            // Codex Phase 1 MEDIUM 3: a radius wider than the segment is valid (spyder_ref clips the taps to
            // the segment and renormalises); clamp it to the grid length only to keep the index arithmetic safe.
            let r = rf.min(n as f64) as usize;
            let lo = a.max(i.saturating_sub(r));
            let hi = b.min(i + r + 1);
            let w: Vec<f64> = (lo..hi)
                .map(|j| {
                    let z = (s.wl[j] - s.wl[i]) / sg;
                    (-0.5 * (z * z)).exp()
                })
                .collect();
            let num: f64 = w.iter().zip(&s.x[lo..hi]).map(|(wj, xj)| wj * xj).sum();
            y[i] = num / np_sum(&w);
        }
    }
    Ok(Spectrum {
        wl: s.wl.clone(),
        x: y,
    })
}

/// A = log10(1/max(R, clip)); A' = gain*A + offset; R' = 10^(-A') (spyder_ref `op_absorbance_affine`).
pub fn absorbance_affine(
    s: &Spectrum,
    joins: &[f64],
    gain: &PerSegment,
    offset: &PerSegment,
    clip_min: f64,
) -> Result<Spectrum, OpError> {
    let op = "absorbance_affine";
    let segs = segment_ranges(&s.wl, joins);
    let n = s.wl.len();
    let g = per_channel(op, gain, &segs, n)?;
    let o = per_channel(op, offset, &segs, n)?;
    let a = absorbance(&s.x, clip_min);
    let x = (0..n).map(|i| 10f64.powf(-(g[i] * a[i] + o[i]))).collect();
    Ok(Spectrum {
        wl: s.wl.clone(),
        x,
    })
}

/// Block SNV (engine 1.1, spyder_ref `op_snv` with `blocks_nm`): each block normalised by its own mean and
/// std (ddof); the output grid is the block points in order; a block spanning this scan's join is an error.
pub fn block_snv(
    s: &Spectrum,
    joins: &[f64],
    ddof: usize,
    blocks_nm: &[(f64, f64)],
) -> Result<Spectrum, OpError> {
    let op = "snv";
    let mut wl = Vec::new();
    let mut x = Vec::new();
    for &(lo, hi) in blocks_nm {
        let k: Vec<usize> = (0..s.wl.len())
            .filter(|&i| s.wl[i] >= lo - WL_TOL && s.wl[i] <= hi + WL_TOL)
            .collect();
        if k.len() < 3 {
            return Err(failed(
                op,
                format!(
                    "SNV block {lo}-{hi} nm has {} grid points (needs >= 3)",
                    k.len()
                ),
            ));
        }
        if ddof >= k.len() {
            return Err(failed(op, "ddof must be smaller than the block length"));
        }
        for &j in joins {
            let below = k.iter().any(|&i| s.wl[i] <= j + WL_TOL);
            let above = k.iter().any(|&i| s.wl[i] > j + WL_TOL);
            if below && above {
                return Err(failed(
                    op,
                    format!("SNV block {lo}-{hi} nm spans this scan's detector join at {j} nm"),
                ));
            }
        }
        let v: Vec<f64> = k.iter().map(|&i| s.x[i]).collect();
        let m = np_mean(&v);
        let sd = np_std(&v, ddof);
        if !sd.is_finite() || sd == 0.0 {
            return Err(failed(op, "SNV of a flat block"));
        }
        for (&i, &vi) in k.iter().zip(&v) {
            wl.push(s.wl[i]);
            x.push((vi - m) / sd);
        }
    }
    Ok(Spectrum { wl, x })
}

/// Apply one operator. `joins` are the scan's own detector joins (nm).
pub fn apply_op(op: &Op, s: &Spectrum, joins: &[f64]) -> Result<Spectrum, OpError> {
    if s.wl.len() != s.x.len() || s.wl.is_empty() {
        return Err(failed(
            "chain",
            "grid and values differ in length or are empty",
        ));
    }
    match op {
        Op::Absorbance { clip_min } => Ok(Spectrum {
            wl: s.wl.clone(),
            x: absorbance(&s.x, *clip_min),
        }),
        Op::SavGol(p) => savgol(s, p),
        Op::Crop { lo, hi } => crop(s, *lo, *hi),
        Op::GaussianBlur {
            sigma_nm,
            truncate,
            radius,
        } => gaussian_blur(s, joins, sigma_nm, *truncate, *radius),
        Op::AbsorbanceAffine {
            gain,
            offset,
            clip_min,
        } => absorbance_affine(s, joins, gain, offset, *clip_min),
        Op::BlockSnv { ddof, blocks_nm } => block_snv(s, joins, *ddof, blocks_nm),
    }
}

/// Run a chain of operators.
pub fn run_chain(chain: &[Op], s: &Spectrum, joins: &[f64]) -> Result<Spectrum, OpError> {
    let mut cur = s.clone();
    for op in chain {
        cur = apply_op(op, &cur, joins)?;
    }
    Ok(cur)
}
