//! Instrument transfer files (`spyder-bone/transfer`, PLAN section 3 Step 4 and section 4; 04 section 2.7):
//! an ordered operator list keyed `source_class` (+ optional `source_serial`) -> `target_class`, applied to the
//! as-measured reflectance before a model's own chain.
//!
//! Selection (spyder_ref `pick_transfer`; DECISIONS 137): the candidates are the registry's selection (one version per
//! id: versions are compared only within an id), in precedence order (bundled files before user files, then by id). A
//! file for this unit (`source_serial` = the scan's serial, compared as text) from the scan's class to the model's
//! class wins, even for the model's own class (a lab's own standard-resolution unit file); otherwise, if the scan's
//! class equals the model's `trained_on_class` or is in its `also_valid_for`, no transfer; otherwise the first
//! class-wide transfer from the scan's class to the model's class. None found: the model still runs, with a gentle
//! note. Format 1 never chains transfers.
//!
//! A correction file may also say whether its visible levels were fitted (`provenance.visible_levels`; the heat check
//! reads them), carry its own second-opinion gap (`provenance.second_opinion_gap`, from >= 20 scans) and per-reader
//! additive offsets (`reader_offsets`, wt%).

use crate::model::Model;
use crate::plugins::gains::{parse_instrument_noise, Correction, InstrumentNoise};
use crate::plugins::{
    parse_header, serial_text, Header, Node, PResult, PluginError, PluginStatus, ScanContext,
    TRANSFER_FORMAT,
};
use crate::preprocess::{self, Op, OpError, Spectrum};

/// A parsed, structurally checked transfer file.
#[derive(Debug, Clone)]
pub struct Transfer {
    pub header: Header,
    /// SHA-256 of the transfer FILE (its identity in records and exports).
    pub file_sha256: String,
    pub source_class: String,
    /// None = the whole class; Some(serial) = one unit (most specific wins).
    pub source_serial: Option<String>,
    pub target_class: String,
    /// v1 transfers are provisional: every result says so.
    pub provisional: bool,
    pub ops: Vec<Op>,
    /// What the file says about its instrument's noise (`provenance.instrument_noise`): the noise correlation length
    /// and C-H envelope factors measured on that instrument's scans. Empty = the noise tables' defaults.
    pub instrument_noise: InstrumentNoise,
    /// Whether the correction's visible levels (VNIR gain and offset, the 1250-1300 nm level) were fitted
    /// (`provenance.visible_levels`; absent = fitted: every correction made before the field fitted them; null or any
    /// other value refuses the file). Not fitted: the heat check reads the scan as measured.
    pub visible_levels_fitted: bool,
    /// The correction's own second-opinion gap (`provenance.second_opinion_gap.points`, estimated from >= 20 scans).
    pub second_opinion_gap: Option<f64>,
    /// Additive offsets (wt%) for the readings made through this correction, by model id (`reader_offsets`).
    pub reader_offsets: std::collections::BTreeMap<String, f64>,
}

/// A larger additive reader offset (wt%) is refused.
pub const READER_OFFSET_MAX: f64 = 5.0;
/// A correction's own second-opinion gap needs at least this many scans behind it.
pub const SECOND_OPINION_MIN_SCANS: u64 = 20;
/// The load error for a `provenance.visible_levels` that is present but neither "fitted" nor "not_fitted" (null
/// included); the same text as the reference's.
pub const VISIBLE_LEVELS_ERROR: &str =
    r#"provenance.visible_levels must be "fitted" or "not_fitted""#;

/// Parse and check a transfer document (sidecars already resolved).
pub fn parse_transfer(doc: &Node, file_sha256: &str) -> PResult<Transfer> {
    let header = parse_header(doc, TRANSFER_FORMAT)?;
    doc.req("title")?.str()?;
    let prov = doc.req("provenance")?;
    prov.obj()?;
    let instrument_noise = match prov.opt("instrument_noise") {
        None => InstrumentNoise::default(),
        Some(n) => parse_instrument_noise(&n)?,
    };
    doc.req("golden")?.obj()?;
    doc.req("input_quantity")?.one_of(&["reflectance"])?;
    let provisional = doc.req("provisional")?.bool()?;
    let source_class = doc.req("source_class")?.str()?.to_string();
    let target_class = doc.req("target_class")?.str()?.to_string();
    let source_serial = match doc.v.get("source_serial") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(serial_text(v).ok_or_else(|| {
            doc.req("source_serial")
                .map(|n| n.error("must be an integer, a string or null"))
                .unwrap_or_else(|e| e)
        })?),
    };
    if source_class == target_class && source_serial.is_none() {
        return Err(PluginError::schema(
            "source_class and target_class must differ (except in a file for one unit: source_serial)",
        ));
    }
    // absent = fitted; anything but "fitted" / "not_fitted" (null included) is refused, with the reference's message
    // (spyder_ref check_transfer), so the two engines never read heat on different streams
    let visible_levels_fitted = match prov.v.get("visible_levels") {
        None => true,
        Some(serde_json::Value::String(s)) if s == "fitted" => true,
        Some(serde_json::Value::String(s)) if s == "not_fitted" => false,
        Some(_) => return Err(PluginError::schema(VISIBLE_LEVELS_ERROR)),
    };
    let second_opinion_gap = match prov.opt("second_opinion_gap") {
        None => None,
        Some(g) => {
            g.obj()?;
            let pts = g.req("points")?;
            let v = pts.f64()?;
            if v.is_nan() || v <= 0.0 {
                return Err(pts.error("must be a positive finite number"));
            }
            let n = g.req("n_scans")?;
            if n.v.as_u64().is_none_or(|x| x < SECOND_OPINION_MIN_SCANS) {
                return Err(n.error(format!(
                    "must be an integer >= {SECOND_OPINION_MIN_SCANS} (a gap estimated from fewer scans is not used)"
                )));
            }
            Some(v)
        }
    };
    let mut reader_offsets = std::collections::BTreeMap::new();
    if let Some(ro) = doc.opt("reader_offsets") {
        for (mid, v) in ro.entries()? {
            let x = v.f64()?;
            if x.is_nan() || x.abs() > READER_OFFSET_MAX {
                return Err(v.error(format!(
                    "a reader offset must be a finite number of at most {READER_OFFSET_MAX} wt% either way"
                )));
            }
            reader_offsets.insert(mid, x);
        }
    }
    let op = doc.req("operator")?;
    if op.arr()?.is_empty() {
        return Err(op.error("needs at least one operator"));
    }
    let ops = preprocess::parse_chain(op.v)?;
    // a zero gain erases the spectrum (and every band and noise term with it): refused at load, in both engines
    for o in &ops {
        if let Op::AbsorbanceAffine { gain, .. } = o {
            let vals: &[f64] = match gain {
                preprocess::PerSegment::Scalar(x) => std::slice::from_ref(x),
                preprocess::PerSegment::Segments(v) => v,
            };
            if vals.iter().any(|x| !x.is_finite() || *x == 0.0) {
                return Err(op.error(
                    "absorbance_affine gain must be a finite non-zero number (a zero gain erases the spectrum)",
                ));
            }
        }
    }
    Ok(Transfer {
        header,
        file_sha256: file_sha256.to_string(),
        source_class,
        source_serial,
        target_class,
        provisional,
        ops,
        instrument_noise,
        visible_levels_fitted,
        second_opinion_gap,
        reader_offsets,
    })
}

impl Transfer {
    /// `id@version`.
    pub fn key(&self) -> String {
        self.header.key()
    }

    /// This transfer as the gain engine sees it (its operators, instrument noise and file SHA-256).
    pub fn correction(&self) -> Correction<'_> {
        Correction {
            ops: &self.ops,
            noise: &self.instrument_noise,
            key: &self.file_sha256,
        }
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

/// The stream one reader reads: the per-reader stream policy, shared by `spyder predict` (`predict_scan`) and the
/// pipeline (oracle `_models`, spyder_ref `pick_reader_transfer`). A reader whose profile spec says
/// `reads: "as_measured"` (S1_R2, transfer-free) reads the scan as measured for every class, even when a unit file or
/// another instrument's correction matches; every other reader goes through [`select_pinned`].
pub fn select_for_reader<'a>(
    reads_as_measured: bool,
    model: &Model,
    ctx: &ScanContext,
    transfers: &[&'a Transfer],
    pinned_ids: &[String],
) -> Selection<'a> {
    if reads_as_measured {
        Selection::NotNeeded
    } else {
        select_pinned(model, ctx, transfers, pinned_ids)
    }
}

/// The selection rule for a target class (a model's `trained_on_class`, or the standard class for the
/// standard-resolution-equivalent stream). `transfers`: one per id (the registry's selection), in precedence order
/// (bundled before user, then by id); the first match wins, so versions are never compared across ids.
pub fn select_target<'a>(
    target: &str,
    also_valid_for: &[String],
    ctx: &ScanContext,
    transfers: &[&'a Transfer],
    pinned_ids: &[String],
) -> Selection<'a> {
    let cands: Vec<&'a Transfer> = transfers
        .iter()
        .copied()
        .filter(|t| {
            let allowed = t.header.status == PluginStatus::Active
                || (t.header.status != PluginStatus::Withdrawn
                    && pinned_ids.contains(&t.header.id));
            allowed && t.source_class == ctx.instrument_class && t.target_class == target
        })
        .collect();
    // a file for this unit wins, even for the model's own class (a lab's own unit file)
    if let Some(t) = cands
        .iter()
        .find(|t| t.source_serial.is_some() && t.source_serial.as_deref() == ctx.serial.as_deref())
    {
        return Selection::Apply(t);
    }
    if ctx.instrument_class == target || also_valid_for.contains(&ctx.instrument_class) {
        return Selection::NotNeeded;
    }
    match cands.iter().find(|t| t.source_serial.is_none()) {
        Some(t) => Selection::Apply(t),
        None => Selection::Missing,
    }
}
