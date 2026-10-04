//! Acquisition checks B1-B8 on the AS-MEASURED scan (PLAN section 3 Step 2; 07 section 3.2).
//!
//! Every check is a pure function returning a [`CheckResult`]: an assessment status (separate from the
//! result, PLAN section 3), an outcome when assessed, the numbers behind it, and the plain-words message.
//! B6 needs noise gains from Phase 0b; it is parameterised by [`NoiseGains`] and never Unusable.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use crate::n2::N2Set;
use crate::read::asd::{scored_mean_sd, SCORED_RANGE_NM};
use crate::read::ReadError;
use crate::scan::{Scan, ScanKind};
use crate::status::Status;

/// Severity of an assessed check (07 section 3.2). Ordered: Pass < Note < Check < Unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Note,
    Check,
    Unusable,
}

/// One check's result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckResult {
    /// "B1" ... "B8", "B6b"; B6 entries carry the model id too.
    pub id: String,
    pub name: String,
    /// Assessment status (assessed / not assessed / gated), never folded into the outcome.
    pub assessment: Status,
    /// Present only when assessed.
    pub outcome: Option<Outcome>,
    /// The numbers behind the outcome (null where undefined).
    pub values: BTreeMap<String, Value>,
    /// Plain-words message when the outcome is above Pass.
    pub message: Option<String>,
}

impl CheckResult {
    fn new(id: &str, name: &str) -> Self {
        CheckResult {
            id: id.to_string(),
            name: name.to_string(),
            assessment: Status::Assessed,
            outcome: Some(Outcome::Pass),
            values: BTreeMap::new(),
            message: None,
        }
    }

    fn not_assessed(id: &str, name: &str, reason: impl Into<String>) -> Self {
        CheckResult {
            assessment: Status::not_assessed(reason),
            outcome: None,
            ..CheckResult::new(id, name)
        }
    }

    fn val(mut self, key: &str, v: Value) -> Self {
        self.values.insert(key.to_string(), v);
        self
    }

    fn fire(mut self, outcome: Outcome, message: impl Into<String>) -> Self {
        self.outcome = Some(outcome);
        self.message = if outcome > Outcome::Pass {
            Some(message.into())
        } else {
            None
        };
        self
    }
}

/// JSON number, or null when not finite.
fn num(x: f64) -> Value {
    if x.is_finite() {
        json!(x)
    } else {
        Value::Null
    }
}

fn in_range(w: f64, lo: f64, hi: f64) -> bool {
    w >= lo && w <= hi
}

const NOT_SCORED: &str = "reference scan (not scored)";

// ---------------------------------------------------------------------------------------------- B1
pub const B1_MESSAGE_REFERENCE: &str =
    "This file has no usable white reference, so reflectance cannot be computed.";

/// B1 file/reference integrity = the Step 1 supported-input matrix (the reader's verdict).
pub fn b1_integrity(read: &Result<Scan, ReadError>) -> CheckResult {
    let c = CheckResult::new("B1", "file and reference integrity");
    match read {
        Ok(_) => c,
        Err(e) => {
            let msg = match e {
                ReadError::Truncated { .. } => "This file is incomplete (truncated).".to_string(),
                ReadError::NotAsd { .. } => "This is not an ASD spectrum file.".to_string(),
                ReadError::Unsupported { reason } => {
                    format!("This file is not in a supported format (unsupported: {reason}).")
                }
                ReadError::Invalid { .. } => B1_MESSAGE_REFERENCE.to_string(),
                ReadError::Io { reason } => format!("The file could not be read ({reason})."),
            };
            c.val("error_kind", json!(e.kind()))
                .val("error", json!(e.to_string()))
                .fire(Outcome::Unusable, msg)
        }
    }
}

// ---------------------------------------------------------------------------------------------- B2
/// Raw DN at or above this is saturated (99.2% of the 16-bit range; 07 B2).
pub const B2_SATURATION_DN: f64 = 65_000.0;
/// Identical consecutive maxima (within one detector segment) that make a flat top.
pub const B2_FLAT_TOP_RUN: usize = 3;

/// Channels of one DN block that are saturated: DN >= 65000, or part of a run of >= 3 identical consecutive
/// values equal to the (positive) maximum of their detector segment.
fn saturated_channels(dn: &[f64], segments: &[(usize, usize)]) -> Vec<usize> {
    let mut out: Vec<usize> = dn
        .iter()
        .enumerate()
        .filter(|(_, &v)| v >= B2_SATURATION_DN)
        .map(|(i, _)| i)
        .collect();
    for &(a, b) in segments {
        let max = dn[a..b].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !(max.is_finite() && max > 0.0) {
            continue;
        }
        let mut i = a;
        while i < b {
            if dn[i] == max {
                let start = i;
                while i < b && dn[i] == max {
                    i += 1;
                }
                if i - start >= B2_FLAT_TOP_RUN {
                    out.extend(start..i);
                }
            } else {
                i += 1;
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// B2 saturation: Unusable when inside 1000-2450 nm, Note when only outside it.
pub fn b2_saturation(scan: &Scan) -> CheckResult {
    let c = CheckResult::new("B2", "detector saturation");
    let segs: Vec<(usize, usize)> = scan.segments.iter().map(|s| (s.start, s.end)).collect();
    let mut chans = saturated_channels(&scan.sample_dn, &segs);
    chans.extend(saturated_channels(&scan.reference_dn, &segs));
    chans.sort_unstable();
    chans.dedup();
    let (lo, hi) = SCORED_RANGE_NM;
    let wl = &scan.wavelengths_nm;
    let inside = chans.iter().filter(|&&i| in_range(wl[i], lo, hi)).count();
    let max_dn = scan
        .sample_dn
        .iter()
        .chain(&scan.reference_dn)
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let c = c
        .val("max_raw_dn", num(max_dn))
        .val("saturated_channels", json!(chans.len()))
        .val("saturated_channels_1000_2450", json!(inside))
        .val(
            "first_saturated_nm",
            chans.first().map(|&i| json!(wl[i])).unwrap_or(Value::Null),
        );
    let msg = "Too much light reached the detector (saturated). Re-take the white reference and scan again.";
    if inside > 0 {
        c.fire(Outcome::Unusable, msg)
    } else if !chans.is_empty() {
        c.fire(
            Outcome::Note,
            "Saturated below 1000 nm only; the collagen region is not affected.",
        )
    } else {
        c
    }
}

// ---------------------------------------------------------------------------------------------- B3
/// B3 impossible reflectance: R <= 0 (or not finite) anywhere in 1000-2450 nm.
pub fn b3_impossible_reflectance(scan: &Scan) -> CheckResult {
    let (id, name) = ("B3", "impossible reflectance");
    if !scan.kind.is_scored() {
        return CheckResult::not_assessed(id, name, NOT_SCORED);
    }
    let (lo, hi) = SCORED_RANGE_NM;
    let bad: Vec<f64> = scan
        .wavelengths_nm
        .iter()
        .zip(&scan.reflectance)
        .filter(|(w, r)| in_range(**w, lo, hi) && !(r.is_finite() && **r > 0.0))
        .map(|(w, _)| *w)
        .collect();
    let c = CheckResult::new(id, name)
        .val("channels_r_le_0_1000_2450", json!(bad.len()))
        .val(
            "first_nm",
            bad.first().map(|w| json!(w)).unwrap_or(Value::Null),
        );
    if bad.is_empty() {
        c
    } else {
        c.fire(
            Outcome::Unusable,
            "The reflectance values are not physical (zero or negative). Re-take the white reference.",
        )
    }
}

// ---------------------------------------------------------------------------------------------- B4, B5
pub const B4_PANEL_MEAN: (f64, f64) = (0.95, 1.05);
pub const B4_PANEL_MAX_SD: f64 = 0.03;
pub const B4_EMPTY_MAX_MEAN: f64 = 0.03;
pub const B5_DARK_MAX_MEAN: f64 = 0.08;

/// B4 panel or empty probe: mean R(1000-2450) in 0.95-1.05 with SD < 0.03, or mean R < 0.03.
/// Reference saves are excepted (not assessed: "reference scan (not scored)").
pub fn b4_panel_or_empty(scan: &Scan) -> CheckResult {
    let (id, name) = ("B4", "panel or empty probe");
    if !scan.kind.is_scored() {
        return CheckResult::not_assessed(id, name, NOT_SCORED);
    }
    let (mean, sd) = scored_mean_sd(&scan.wavelengths_nm, &scan.reflectance);
    let c = CheckResult::new(id, name)
        .val("mean_r_1000_2450", num(mean))
        .val("sd_r_1000_2450", num(sd));
    if !(mean.is_finite() && sd.is_finite()) {
        return CheckResult::not_assessed(
            id,
            name,
            "reflectance not finite in 1000-2450 nm (see B3)",
        );
    }
    let panel = mean >= B4_PANEL_MEAN.0 && mean <= B4_PANEL_MEAN.1 && sd < B4_PANEL_MAX_SD;
    if panel || mean < B4_EMPTY_MAX_MEAN {
        c.fire(
            Outcome::Unusable,
            "This looks like the white reference (or an empty probe), not bone.",
        )
    } else {
        c
    }
}

/// B5 very dark spot: mean R(1000-2450) < 0.08 (Note).
pub fn b5_dark_spot(scan: &Scan) -> CheckResult {
    let (id, name) = ("B5", "very dark spot");
    if !scan.kind.is_scored() {
        return CheckResult::not_assessed(id, name, NOT_SCORED);
    }
    let (mean, _) = scored_mean_sd(&scan.wavelengths_nm, &scan.reflectance);
    if !mean.is_finite() {
        return CheckResult::not_assessed(
            id,
            name,
            "reflectance not finite in 1000-2450 nm (see B3)",
        );
    }
    let c = CheckResult::new(id, name).val("mean_r_1000_2450", num(mean));
    if mean < B5_DARK_MAX_MEAN {
        c.fire(
            Outcome::Note,
            "Very dark spot. Predictions are less certain; a lighter area may scan better.",
        )
    } else {
        c
    }
}

// ---------------------------------------------------------------------------------------------- B6, B6b
/// One noise-gain term: % collagen per unit N2 measured in `n2_window_nm` on the as-measured scan.
#[derive(Debug, Clone, PartialEq)]
pub struct GainTerm {
    pub n2_window_nm: (f64, f64),
    pub gain: f64,
}

/// Gains of one model output on one stream. `transfer` is the transfer hash, or "none" (PLAN Step 2b:
/// gains are keyed by model id AND transfer hash, with an explicit "no transfer" entry).
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseGain {
    pub model_id: String,
    pub transfer: String,
    /// SD = sqrt(sum_k (g_k * N2_k)^2): one term for single-window models, two for F05.
    pub terms: Vec<GainTerm>,
}

/// The gains table plus the B6 rule parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseGains {
    pub entries: Vec<NoiseGain>,
    /// B6 raises a Check when the implied SD exceeds this (% collagen). PLAN: 0.5.
    pub check_above_pct: f64,
}

impl NoiseGains {
    pub fn find(&self, model_id: &str, transfer: &str) -> Option<&NoiseGain> {
        self.entries
            .iter()
            .find(|e| e.model_id == model_id && e.transfer == transfer)
    }
}

/// Implied prediction SD (% collagen) of one model from the scan's N2 values. None if a needed N2 is missing.
pub fn implied_sd(gain: &NoiseGain, n2: &N2Set) -> Option<f64> {
    if gain.terms.is_empty() {
        return None;
    }
    let mut s = 0.0;
    for t in &gain.terms {
        let v = t.gain * n2.get(t.n2_window_nm)?;
        s += v * v;
    }
    Some(s.sqrt())
}

/// CONS3 rule (provisional): factor x median of the component SDs (PLAN Step 2b: factor 1.2).
pub fn consensus_sd(component_sds: &[f64], factor: f64) -> Option<f64> {
    if component_sds.is_empty() || component_sds.iter().any(|x| !x.is_finite()) {
        return None;
    }
    Some(factor * crate::n2::median(component_sds))
}

fn b6_from_sd(model_id: &str, transfer: &str, sd: Option<f64>, threshold: f64) -> CheckResult {
    let id = "B6";
    let name = format!("noise as prediction uncertainty ({model_id})");
    let Some(sd) = sd else {
        return CheckResult::not_assessed(id, &name, "N2 undefined in a window this model needs");
    };
    let c = CheckResult::new(id, &name)
        .val("model_id", json!(model_id))
        .val("transfer", json!(transfer))
        .val("implied_sd_pct", num(sd))
        .val("check_above_pct", num(threshold));
    if sd > threshold {
        c.fire(
            Outcome::Check,
            format!("Noisy scan: this reading could be off by about \u{b1}{sd:.1}%."),
        )
    } else {
        c
    }
}

/// B6 for one model on one stream. Check if implied SD > threshold; never Unusable (not calibrated on
/// same-spot repeats). Not assessed when no gain is keyed by this model and transfer.
pub fn b6_noise(gains: &NoiseGains, model_id: &str, transfer: &str, n2: &N2Set) -> CheckResult {
    match gains.find(model_id, transfer) {
        None => CheckResult::not_assessed(
            "B6",
            &format!("noise as prediction uncertainty ({model_id})"),
            format!("no noise gain for model {model_id} with transfer {transfer}"),
        ),
        Some(g) => b6_from_sd(model_id, transfer, implied_sd(g, n2), gains.check_above_pct),
    }
}

/// B6 placeholder when no gains table is loaded (e.g. `spyder read` before Phase 0b ships `noise_gains.json`).
pub fn b6_without_gains(reason: &str) -> CheckResult {
    CheckResult::not_assessed("B6", "noise as prediction uncertainty", reason)
}

/// B6 for a consensus of components (CONS3): factor x median of the component SDs.
pub fn b6_consensus(
    gains: &NoiseGains,
    consensus_id: &str,
    component_ids: &[&str],
    transfer: &str,
    factor: f64,
    n2: &N2Set,
) -> CheckResult {
    let mut sds = Vec::new();
    for id in component_ids {
        match gains.find(id, transfer) {
            None => {
                return CheckResult::not_assessed(
                    "B6",
                    &format!("noise as prediction uncertainty ({consensus_id})"),
                    format!("no noise gain for component {id} with transfer {transfer}"),
                )
            }
            Some(g) => match implied_sd(g, n2) {
                Some(s) => sds.push(s),
                None => return b6_from_sd(consensus_id, transfer, None, gains.check_above_pct),
            },
        }
    }
    b6_from_sd(
        consensus_id,
        transfer,
        consensus_sd(&sds, factor),
        gains.check_above_pct,
    )
}

/// B6b cut on N2(2000-2100), as measured (PLAN Step 2: 60).
pub const B6B_N2_CUT: f64 = 60.0;

/// B6b too noisy for contaminant signs: N2(2000-2100) > cut -> Note, and the signs are gated.
pub fn b6b_signs_noise(n2: &N2Set, cut: f64) -> CheckResult {
    let (id, name) = ("B6b", "too noisy for contaminant signs");
    let Some(v) = n2.n2_2000_2100 else {
        return CheckResult::not_assessed(id, name, "N2(2000-2100) undefined");
    };
    let fired = v > cut;
    let c = CheckResult::new(id, name)
        .val("n2_2000_2100", num(v))
        .val("cut", num(cut))
        .val("signs_gated", json!(fired));
    if fired {
        c.fire(
            Outcome::Note,
            "Too noisy to check for coatings on this scan.",
        )
    } else {
        c
    }
}

// ---------------------------------------------------------------------------------------------- B7
/// B7 reflectance above 1: max R(1001-1800) > 1 on the as-measured scan (Note only).
pub fn b7_above_one(scan: &Scan) -> CheckResult {
    let (id, name) = ("B7", "reflectance above 1");
    if !scan.kind.is_scored() {
        return CheckResult::not_assessed(id, name, NOT_SCORED);
    }
    let max = scan
        .wavelengths_nm
        .iter()
        .zip(&scan.reflectance)
        .filter(|(w, _)| in_range(**w, 1001.0, 1800.0))
        .map(|(_, &r)| r)
        .fold(f64::NEG_INFINITY, f64::max);
    let c = CheckResult::new(id, name).val("max_r_1001_1800", num(max));
    if max > 1.0 {
        c.fire(
            Outcome::Note,
            "Brighter than the white reference (the reference may be low). Collagen estimates and coating checks are not affected.",
        )
    } else {
        c
    }
}

// ---------------------------------------------------------------------------------------------- B8
pub const B8_NOTE_PCT: f64 = 5.0;
pub const B8_CHECK_PCT: f64 = 15.0;
/// SWIR1 points fitted at the lower and upper joins (the research `splice_correct` convention: 1001-1005, 1795-1800).
pub const B8_FIT_POINTS_LOWER: usize = 5;
pub const B8_FIT_POINTS_UPPER: usize = 6;

/// Closed-form least-squares line through y at x = 0..m-1, evaluated at x_eval (spyder_ref `_line_fit_eval`).
pub fn line_fit_eval(y: &[f64], x_eval: f64) -> f64 {
    let m = y.len() as f64;
    let xb = (0..y.len()).map(|i| i as f64).sum::<f64>() / m;
    let sxx: f64 = (0..y.len()).map(|i| (i as f64 - xb).powi(2)).sum();
    let yb = y.iter().sum::<f64>() / m;
    let sxy: f64 = y
        .iter()
        .enumerate()
        .map(|(i, &v)| (v - yb) * (i as f64 - xb))
        .sum();
    yb + sxy / sxx * (x_eval - xb)
}

/// Relative splice steps (%) at the two header joins: |SWIR1 extrapolation - neighbour| / R(SWIR1 edge).
/// Lower join: line through the first 5 SWIR1 channels, extrapolated one channel down, minus R at the join.
/// Upper join: line through the last 6 SWIR1 channels, extrapolated one channel up, minus R just above it.
pub fn splice_steps_pct(scan: &Scan) -> Option<[f64; 2]> {
    if scan.splices_nm.len() != 2 {
        return None;
    }
    let wl = &scan.wavelengths_nm;
    let r = &scan.reflectance;
    let i1 = crate::grid::last_le(wl, scan.splices_nm[0])?;
    let i2 = crate::grid::last_le(wl, scan.splices_nm[1])?;
    if i1 + 1 + B8_FIT_POINTS_LOWER > r.len() || i2 + 2 > r.len() || i2 + 1 < B8_FIT_POINTS_UPPER {
        return None;
    }
    let o1 = line_fit_eval(&r[i1 + 1..i1 + 1 + B8_FIT_POINTS_LOWER], -1.0) - r[i1];
    let o2 = line_fit_eval(
        &r[i2 + 1 - B8_FIT_POINTS_UPPER..=i2],
        B8_FIT_POINTS_UPPER as f64,
    ) - r[i2 + 1];
    Some([100.0 * o1.abs() / r[i1 + 1], 100.0 * o2.abs() / r[i2]])
}

/// B8 splice step at each header join: Note > 5%, Check > 15%.
pub fn b8_splice_step(scan: &Scan) -> CheckResult {
    let (id, name) = ("B8", "splice step");
    if !scan.kind.is_scored() {
        return CheckResult::not_assessed(id, name, NOT_SCORED);
    }
    let Some(steps) = splice_steps_pct(scan) else {
        return CheckResult::not_assessed(id, name, "the scan does not have two usable joins");
    };
    let c = CheckResult::new(id, name)
        .val("join1_nm", num(scan.splices_nm[0]))
        .val("join2_nm", num(scan.splices_nm[1]))
        .val("step1_pct", num(steps[0]))
        .val("step2_pct", num(steps[1]));
    if steps.iter().any(|s| !s.is_finite()) {
        return CheckResult::not_assessed(id, name, "reflectance not finite at a join (see B3)");
    }
    let (k, worst) = if steps[0] >= steps[1] {
        (0, steps[0])
    } else {
        (1, steps[1])
    };
    let msg = format!(
        "Unusually large step where the detectors join (near {} nm). Shown for information; the models are not affected.",
        scan.splices_nm[k]
    );
    if worst > B8_CHECK_PCT {
        c.fire(Outcome::Check, msg)
    } else if worst > B8_NOTE_PCT {
        c.fire(Outcome::Note, msg)
    } else {
        c
    }
}

// ---------------------------------------------------------------------------------------------- all
/// B1-B5, B6b, B7, B8 on a read result (B6 needs gains and runs per model; see [`b6_noise`]).
/// A failed read gives B1 Unusable and the others "not assessed".
pub fn acquisition_checks(read: &Result<Scan, ReadError>, n2: Option<&N2Set>) -> Vec<CheckResult> {
    let mut out = vec![b1_integrity(read)];
    let Ok(scan) = read else {
        for (id, name) in [
            ("B2", "detector saturation"),
            ("B3", "impossible reflectance"),
            ("B4", "panel or empty probe"),
            ("B5", "very dark spot"),
            ("B6b", "too noisy for contaminant signs"),
            ("B7", "reflectance above 1"),
            ("B8", "splice step"),
        ] {
            out.push(CheckResult::not_assessed(
                id,
                name,
                "the file could not be read (B1)",
            ));
        }
        return out;
    };
    out.push(b2_saturation(scan));
    out.push(b3_impossible_reflectance(scan));
    out.push(b4_panel_or_empty(scan));
    out.push(b5_dark_spot(scan));
    out.push(match (scan.kind, n2) {
        (ScanKind::Sample, Some(n2)) => b6b_signs_noise(n2, B6B_N2_CUT),
        (ScanKind::Sample, None) => {
            CheckResult::not_assessed("B6b", "too noisy for contaminant signs", "N2 not computed")
        }
        _ => CheckResult::not_assessed("B6b", "too noisy for contaminant signs", NOT_SCORED),
    });
    out.push(b7_above_one(scan));
    out.push(b8_splice_step(scan));
    out
}

/// The worst assessed outcome (Pass if none fired).
pub fn worst_outcome(checks: &[CheckResult]) -> Outcome {
    checks
        .iter()
        .filter_map(|c| c.outcome)
        .max()
        .unwrap_or(Outcome::Pass)
}
