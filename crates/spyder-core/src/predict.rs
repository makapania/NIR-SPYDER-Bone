//! Model predictions for one scan (PLAN section 3 Steps 3, 4 and 6): for every selected model, pick the
//! transfer by the Step 4 rule and the pipeline's per-reader stream policy (a model a profile lists as reading the scan
//! as measured, S1_R2, never goes through a correction), run the model on the standard-resolution-equivalent stream
//! (the scan itself, or the transferred spectrum), check grid eligibility, and record the result with the model's
//! id@version and file SHA-256 and the transfer's id@version, SHA-256 and "provisional" flag. Transferred spectra are
//! cached per transfer, since several models share one.
//!
//! The noise SD of each reading (computed from the model, the transfer actually applied and the scan's own detector
//! segments under `noise_gains.json`'s noise model) is reported as a number; the B6 rule belongs to the pipeline.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{Model, ModelKind, Prediction};
use crate::plugins::registry::Registry;
use crate::plugins::{ScanContext, MODEL_FORMAT};
use crate::preprocess::Spectrum;
use crate::status::Status;
use crate::transfer::{select_for_reader, Selection, Transfer};

/// The transfer applied before a model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransferRef {
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub provisional: bool,
}

/// One model's result on one scan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelResult {
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub kind: ModelKind,
    pub role: String,
    pub short_name: Option<String>,
    /// None = the scan as measured (same class, `also_valid_for`, a reader that reads as measured, or no transfer
    /// available).
    pub transfer: Option<TransferRef>,
    /// Plain-words note about the stream (e.g. "no transfer available for this instrument").
    pub transfer_note: Option<String>,
    /// assessed / not assessed: reason (grid eligibility, a chain that cannot run on this scan...).
    pub assessment: Status,
    pub prediction: Option<Prediction>,
    /// Implied SD of the reading (wt%) computed for this stream; None = not assessed (see `noise_status`).
    pub noise_sd_pct: Option<f64>,
    /// "assessed" or why the noise SD could not be computed (a non-linear correction, not available on this
    /// instrument, ...).
    pub noise_status: String,
    /// The correction's additive offset (wt%) for this reader (`reader_offsets`), when it carries one; for a consensus,
    /// its components' offsets (added before the median) by component name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader_offset: Option<f64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub component_offsets: BTreeMap<String, f64>,
}

impl ModelResult {
    pub fn key(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

/// Predictions of every selected model on one scan (`wl`, `r` = the AS-MEASURED reflectance).
/// Consensus files come first, then the other models in id order.
pub fn predict_scan(reg: &Registry, wl: &[f64], r: &[f64], ctx: &ScanContext) -> Vec<ModelResult> {
    let raw = Spectrum::new(wl.to_vec(), r.to_vec());
    let transfers: Vec<&Transfer> = reg.transfers();
    let pinned = reg.pinned_transfer_ids();
    let mut streams: BTreeMap<String, Result<Spectrum, String>> = BTreeMap::new();
    let mut models: Vec<&Model> = reg.models();
    models.sort_by_key(|m| (m.kind != ModelKind::Consensus, m.header.id.clone()));
    let noise = reg.noise_gains();
    let mut out = Vec::new();
    for m in models {
        let sha256 = reg
            .selected_entry(MODEL_FORMAT, &m.header.id)
            .map(|e| e.sha256.clone())
            .unwrap_or_default();
        let mut res = ModelResult {
            id: m.header.id.clone(),
            version: m.header.version.to_string(),
            sha256,
            kind: m.kind,
            role: m.role.clone(),
            short_name: m.short_name.clone(),
            transfer: None,
            transfer_note: None,
            assessment: Status::Assessed,
            prediction: None,
            noise_sd_pct: None,
            noise_status: "not assessed: no noise table loaded".to_string(),
            reader_offset: None,
            component_offsets: BTreeMap::new(),
        };
        // the pipeline's per-reader stream policy: S1_R2 (reads as measured) never goes through a correction, not even
        // a unit file or another instrument's file
        let sel = select_for_reader(
            reg.reads_as_measured(&m.header.id),
            m,
            ctx,
            &transfers,
            &pinned,
        );
        let offsets = match sel {
            Selection::Apply(t) => Some(&t.reader_offsets),
            _ => None,
        };
        let (stream, corr) = match sel {
            Selection::NotNeeded => (Ok(raw.clone()), None),
            Selection::Missing => {
                res.transfer_note = Some(format!(
                    "no correction for this instrument is installed ({} to {}); the model runs on the scan as \
                     measured (uncalibrated)",
                    ctx.instrument_class, m.trained_on_class
                ));
                (Ok(raw.clone()), None)
            }
            Selection::Apply(t) => {
                res.transfer = Some(TransferRef {
                    id: t.header.id.clone(),
                    version: t.header.version.to_string(),
                    sha256: t.file_sha256.clone(),
                    provisional: t.provisional,
                });
                if t.provisional {
                    res.transfer_note =
                        Some(format!("correction {} applied (provisional)", t.key()));
                }
                let s: Result<Spectrum, String> = streams
                    .entry(t.file_sha256.clone())
                    .or_insert_with(|| t.apply(&raw, ctx).map_err(|e| e.to_string()))
                    .clone();
                (s, Some(t.correction()))
            }
        };
        if let Some(ng) = noise {
            let isd = ng.implied_sd(
                Some(&reg.gain_cache),
                m,
                wl,
                &ctx.splices_nm,
                corr,
                &mut |lo, hi| crate::n2::n2(r, wl, lo, hi).unwrap_or(f64::NAN),
            );
            res.noise_sd_pct = isd.sd;
            res.noise_status = isd.status;
        }
        match stream {
            Err(e) => {
                res.assessment =
                    Status::not_assessed(format!("the transfer cannot run on this scan: {e}"))
            }
            Ok(s) => {
                let el = m.eligibility(&s.wl, &ctx.splices_nm);
                if !el.is_assessed() {
                    res.assessment = el;
                } else {
                    match m.predict(&s, ctx) {
                        Ok(mut p) if p.value.is_finite() => {
                            if let Some(o) = offsets.filter(|o| !o.is_empty()) {
                                let (own, comps) = m.add_reader_offsets(&mut p, o);
                                res.reader_offset = own;
                                res.component_offsets = comps.into_iter().collect();
                            }
                            res.prediction = Some(p)
                        }
                        Ok(_) => {
                            res.assessment = Status::not_assessed(
                                "the model gives no finite reading on this scan",
                            )
                        }
                        Err(e) => {
                            res.assessment = Status::not_assessed(format!(
                                "the model cannot run on this scan: {e}"
                            ))
                        }
                    }
                }
            }
        }
        out.push(res);
    }
    out
}
