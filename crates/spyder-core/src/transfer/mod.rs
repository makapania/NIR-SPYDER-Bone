//! Instrument transfer files (`spyder-bone/transfer`, PLAN section 3 Step 4 and section 4; 04 section 2.7):
//! an ordered operator list keyed `source_class` (+ optional `source_serial`) -> `target_class`, applied to the
//! as-measured reflectance before a model's own chain.
//!
//! Selection (spyder_ref `pick_transfer`): if the scan's class equals the model's `trained_on_class` or is in
//! its `also_valid_for`, no transfer; otherwise the most specific `active` transfer from the scan's class to the
//! model's class (a matching `source_serial` beats a class-wide one; then the highest version). None found:
//! the model still runs, with a gentle note. Format 1 never chains transfers.

use crate::model::Model;
use crate::plugins::{
    parse_header, serial_text, Header, Node, PResult, PluginError, PluginStatus, ScanContext,
    TRANSFER_FORMAT,
};
use crate::preprocess::{self, Op, OpError, Spectrum};

/// A parsed, structurally checked transfer file.
#[derive(Debug, Clone)]
pub struct Transfer {
    pub header: Header,
    /// SHA-256 of the transfer FILE (the key of `noise_gains.json` and `bands.json` gains).
    pub file_sha256: String,
    pub source_class: String,
    /// None = the whole class; Some(serial) = one unit (most specific wins).
    pub source_serial: Option<String>,
    pub target_class: String,
    /// v1 transfers are provisional: every result says so.
    pub provisional: bool,
    pub ops: Vec<Op>,
}

/// Parse and check a transfer document (sidecars already resolved).
pub fn parse_transfer(doc: &Node, file_sha256: &str) -> PResult<Transfer> {
    let header = parse_header(doc, TRANSFER_FORMAT)?;
    doc.req("title")?.str()?;
    doc.req("provenance")?.obj()?;
    doc.req("golden")?.obj()?;
    doc.req("input_quantity")?.one_of(&["reflectance"])?;
    let provisional = doc.req("provisional")?.bool()?;
    let source_class = doc.req("source_class")?.str()?.to_string();
    let target_class = doc.req("target_class")?.str()?.to_string();
    if source_class == target_class {
        return Err(PluginError::schema(
            "source_class and target_class must differ",
        ));
    }
    let source_serial = match doc.v.get("source_serial") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(serial_text(v).ok_or_else(|| {
            doc.req("source_serial")
                .map(|n| n.error("must be an integer, a string or null"))
                .unwrap_or_else(|e| e)
        })?),
    };
    let op = doc.req("operator")?;
    if op.arr()?.is_empty() {
        return Err(op.error("needs at least one operator"));
    }
    let ops = preprocess::parse_chain(op.v)?;
    Ok(Transfer {
        header,
        file_sha256: file_sha256.to_string(),
        source_class,
        source_serial,
        target_class,
        provisional,
        ops,
    })
}

impl Transfer {
    /// `id@version`.
    pub fn key(&self) -> String {
        self.header.key()
    }

    /// Apply the operator list to the as-measured spectrum, segmented at the scan's own joins.
    pub fn apply(&self, s: &Spectrum, ctx: &ScanContext) -> Result<Spectrum, OpError> {
        preprocess::run_chain(&self.ops, s, &ctx.splices_nm)
    }
}

/// What a model runs on for a given scan.
#[derive(Debug, Clone, Copy)]
pub enum Selection<'a> {
    /// The scan's class is the model's own class (or listed in `also_valid_for`): the scan as measured.
    NotNeeded,
    /// Apply this transfer first.
    Apply(&'a Transfer),
    /// No active transfer from the scan's class to the model's class: the model runs on the scan as measured,
    /// with a gentle "no transfer available for this instrument" note.
    Missing,
}

/// Choose the transfer for one model and scan from the candidate transfers (spyder_ref `pick_transfer`).
/// Only `active` transfers are ever selected automatically.
pub fn select<'a>(model: &Model, ctx: &ScanContext, transfers: &[&'a Transfer]) -> Selection<'a> {
    select_pinned(model, ctx, transfers, &[])
}

/// As [`select`], but a transfer whose id the user explicitly pinned (and the registry accepted: loaded, not
/// withdrawn) may be selected even if it is not `active` (Codex Phase 2 MEDIUM 4).
pub fn select_pinned<'a>(
    model: &Model,
    ctx: &ScanContext,
    transfers: &[&'a Transfer],
    pinned_ids: &[String],
) -> Selection<'a> {
    select_target(
        &model.trained_on_class,
        &model.also_valid_for,
        ctx,
        transfers,
        pinned_ids,
    )
}

/// The selection rule for a target class (a model's `trained_on_class`, or the standard class for the
/// standard-resolution-equivalent stream).
pub fn select_target<'a>(
    target: &str,
    also_valid_for: &[String],
    ctx: &ScanContext,
    transfers: &[&'a Transfer],
    pinned_ids: &[String],
) -> Selection<'a> {
    if ctx.instrument_class == target || also_valid_for.contains(&ctx.instrument_class) {
        return Selection::NotNeeded;
    }
    let best = transfers
        .iter()
        .filter(|t| {
            let allowed = t.header.status == PluginStatus::Active
                || (t.header.status != PluginStatus::Withdrawn
                    && pinned_ids.contains(&t.header.id));
            allowed
                && t.source_class == ctx.instrument_class
                && t.target_class == target
                && match &t.source_serial {
                    None => true,
                    Some(s) => ctx.serial.as_deref() == Some(s.as_str()),
                }
        })
        .max_by(|a, b| {
            (a.source_serial.is_some(), a.header.version)
                .cmp(&(b.source_serial.is_some(), b.header.version))
        });
    match best {
        Some(t) => Selection::Apply(t),
        None => Selection::Missing,
    }
}
