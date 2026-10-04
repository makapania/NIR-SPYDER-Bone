//! Model predictions for one scan (PLAN section 3 Steps 3, 4 and 6): for every selected model, pick the
//! transfer by the Step 4 rule, run the model on the standard-resolution-equivalent stream (the scan itself, or
//! the transferred spectrum), check grid eligibility, and record the result with the model's id@version and
//! file SHA-256 and the transfer's id@version, SHA-256 and "provisional" flag. Transferred spectra are cached
//! per transfer, since several models share one.
//!
//! The noise SD of each reading (`noise_gains.json`, keyed by model id and the transfer FILE SHA-256 actually
//! applied) is reported as a number; the B6 rule itself belongs to the Phase 3 pipeline.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{Model, ModelKind, Prediction};
use crate::plugins::registry::Registry;
use crate::plugins::tables::NO_TRANSFER;
use crate::plugins::{ScanContext, MODEL_FORMAT};
use crate::preprocess::Spectrum;
use crate::status::Status;
use crate::transfer::{select_pinned, Selection, Transfer};

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
    /// None = the scan as measured (same class, `also_valid_for`, or no transfer available).
    pub transfer: Option<TransferRef>,
    /// Plain-words note about the stream (e.g. "no transfer available for this instrument").
    pub transfer_note: Option<String>,
    /// assessed / not assessed: reason (grid eligibility, a chain that cannot run on this scan...).
    pub assessment: Status,
    pub prediction: Option<Prediction>,
    /// Implied SD of the reading (wt%) from the noise gains for this stream; None = not assessed.
    pub noise_sd_pct: Option<f64>,
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
        };
        let (stream, key): (Result<Spectrum, String>, Option<String>) = match select_pinned(
            m, ctx, &transfers, &pinned,
        ) {
            Selection::NotNeeded => (Ok(raw.clone()), Some(NO_TRANSFER.to_string())),
            Selection::Missing => {
                res.transfer_note = Some(format!(
                        "no transfer available for this instrument ({} to {}); the model runs on the scan as measured",
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
                    res.transfer_note = Some("high-res transfer applied (provisional)".to_string());
                }
                let s = streams
                    .entry(t.file_sha256.clone())
                    .or_insert_with(|| t.apply(&raw, ctx).map_err(|e| e.to_string()))
                    .clone();
                (s, Some(t.file_sha256.clone()))
            }
        };
        res.noise_sd_pct = match (&key, noise) {
            (Some(k), Some(ng)) => ng.implied_sd(&m.header.id, &raw, k),
            _ => None,
        };
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
                        Ok(p) if p.value.is_finite() => res.prediction = Some(p),
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
