//! Plug-in files (PLAN v3.2 section 4; 03 section 5; 04 section 2): discovery, schema-level validation,
//! precedence and pins, sidecars, and the hardened golden runner.
//!
//! * [`json`]: strict JSON loading (finite numbers only), `.npy` sidecar resolution, path confinement, and the
//!   [`Node`] accessor used for hand-written schema validation (every error names its JSON path).
//! * [`npy`]: the `.npy` reader (little-endian float64, C order, 1-D/2-D, never pickles).
//! * [`golden`]: golden spectra files and the golden runner for models, consensus files and transfers.
//! * [`tables`]: `bands.json`, `noise_gains.json` (with their goldens) and the instrument registry.
//! * [`checks`], [`profiles`], [`refset`]: engine-check parameter files, analysis profiles and reference sets
//!   (loaded and validated; their evaluation is Phase 3).
//! * [`registry`]: discovery over the bundled and user folders, cross-file checks, precedence, pins and the
//!   startup-error state.
//!
//! Every plug-in is identified by its file SHA-256; a file never needs to contain its own hash.

// `!(x > 0.0)` is deliberate: it rejects NaN as well as non-positive values.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod checks;
pub mod golden;
pub mod json;
pub mod npy;
pub mod profiles;
pub mod refset;
pub mod registry;
pub mod tables;

use std::fmt;

use serde::Serialize;
use sha2::{Digest, Sha256};

pub use json::Node;

/// Engine (operator-set) version this crate implements; files declare `engine_min`.
pub const ENGINE_VERSION: (u32, u32) = crate::preprocess::ENGINE_VERSION;
/// Every plug-in kind is at `format_version` 1.
pub const FORMAT_VERSION: u64 = 1;
/// Sidecars above this size are refused (dense DS matrices, about 24 MB, are never bundled).
pub const SIDECAR_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Plug-in JSON files above this size are refused before parsing (the largest shipped file is about 1.5 MB).
pub const PLUGIN_MAX_BYTES: u64 = 64 * 1024 * 1024;

pub const MODEL_FORMAT: &str = "spyder-bone/model";
pub const TRANSFER_FORMAT: &str = "spyder-bone/transfer";
pub const GOLDEN_SPECTRA_FORMAT: &str = "spyder-bone/golden_spectra";
pub const BANDS_FORMAT: &str = "spyder-bone/bands";
pub const NOISE_GAINS_FORMAT: &str = "spyder-bone/noise_gains";
pub const INSTRUMENT_FORMAT: &str = "spyder-bone/instrument";
pub const CATALOG_FORMAT: &str = "spyder-bone/catalog";
pub const ENGINE_CHECK_FORMAT: &str = "spyder-bone/engine_check";
pub const PROFILE_FORMAT: &str = "spyder-bone/analysis_profile";
pub const REFERENCE_SET_FORMAT: &str = "spyder-bone/reference_set";

/// What went wrong with a plug-in file (a stable tag for summaries; the message is plain words).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Cannot read the file.
    Io,
    /// Not valid JSON, or a non-finite number.
    Json,
    /// Fails schema-level validation (missing field, wrong type, bad value).
    Schema,
    /// Uses a reserved operator or model kind (never a guess).
    Unsupported,
    /// Needs a newer engine (`engine_min` above this engine's op set).
    NeedsNewerEngine,
    /// A sidecar is missing, outside the package, too large, tampered or malformed.
    Sidecar,
    /// The goldens cannot test anything or are malformed (empty, too few cases, unknown outputs...).
    GoldenInvalid,
    /// A golden comparison failed.
    GoldenFailed,
    /// Same id and version as another file with different contents (both rejected).
    Conflict,
    /// A cross-file reference does not resolve (consensus component, catalog entry, profile model...).
    CrossFile,
}

/// A plug-in problem: kind + plain-words message.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PluginError {
    pub kind: ErrorKind,
    pub message: String,
}

impl PluginError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        PluginError {
            kind,
            message: message.into(),
        }
    }
    pub fn schema(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Schema, message)
    }
    pub fn golden(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::GoldenInvalid, message)
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PluginError {}

impl From<crate::preprocess::OpError> for PluginError {
    fn from(e: crate::preprocess::OpError) -> Self {
        use crate::preprocess::OpError;
        let kind = match e {
            OpError::UnsupportedOperator { .. } => ErrorKind::Unsupported,
            OpError::UnknownOperator { .. } => ErrorKind::Unsupported,
            _ => ErrorKind::Schema,
        };
        PluginError::new(kind, e.to_string())
    }
}

pub type PResult<T> = Result<T, PluginError>;

/// Semantic version `MAJOR.MINOR.PATCH`, integer-parsable (selection compares integer tuples).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// Parse `^[0-9]+\.[0-9]+\.[0-9]+$`.
    pub fn parse(s: &str) -> Option<Version> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let mut v = [0u64; 3];
        for (k, p) in parts.iter().enumerate() {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            v[k] = p.parse().ok()?;
        }
        Some(Version(v[0], v[1], v[2]))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

/// `engine_min`: `^[0-9]+\.[0-9]+$`.
pub fn parse_engine(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once('.')?;
    let ok = |p: &str| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit());
    if !ok(a) || !ok(b) {
        return None;
    }
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// Plug-in status. Only `active` files are selected automatically; `withdrawn` files are kept so that old
/// results stay interpretable, but are never used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    Active,
    Experimental,
    Demo,
    Withdrawn,
}

impl PluginStatus {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "active" => PluginStatus::Active,
            "experimental" => PluginStatus::Experimental,
            "demo" => PluginStatus::Demo,
            "withdrawn" => PluginStatus::Withdrawn,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            PluginStatus::Active => "active",
            PluginStatus::Experimental => "experimental",
            PluginStatus::Demo => "demo",
            PluginStatus::Withdrawn => "withdrawn",
        }
    }
}

/// The common header of a versioned plug-in file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Header {
    pub format: String,
    pub id: String,
    pub version: Version,
    pub engine_min: (u32, u32),
    pub status: PluginStatus,
    pub title: Option<String>,
}

impl Header {
    /// `id@version`.
    pub fn key(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

/// Parse and check the common header: format, integer `format_version` 1, id (non-empty, optional pattern
/// check by the caller), semver version, `engine_min` not above this engine, status.
pub fn parse_header(doc: &Node, format: &str) -> PResult<Header> {
    let f = doc.req("format")?.str()?;
    if f != format {
        return Err(PluginError::schema(format!(
            "{}: not a {format} file (format {f:?})",
            doc.path()
        )));
    }
    let fv = doc.req("format_version")?;
    if fv.v.as_u64() != Some(FORMAT_VERSION) {
        return Err(PluginError::schema(format!(
            "unsupported format_version {} (this engine reads format_version 1)",
            fv.v
        )));
    }
    let em_s = doc.req("engine_min")?.str()?;
    let engine_min = parse_engine(em_s).ok_or_else(|| {
        PluginError::schema(format!("engine_min {em_s:?} must look like \"1.1\""))
    })?;
    if engine_min > ENGINE_VERSION {
        return Err(PluginError::new(
            ErrorKind::NeedsNewerEngine,
            format!(
                "needs a newer SPYDER Bone (op set {em_s}; this engine provides {}.{})",
                ENGINE_VERSION.0, ENGINE_VERSION.1
            ),
        ));
    }
    let id = doc.req("id")?.str()?.to_string();
    if id.is_empty() {
        return Err(PluginError::schema("id must not be empty"));
    }
    let vs = doc.req("version")?.str()?;
    let version = Version::parse(vs).ok_or_else(|| {
        PluginError::schema(format!(
            "version {vs:?} must be integer semver MAJOR.MINOR.PATCH"
        ))
    })?;
    let st = doc.req("status")?.str()?;
    let status = PluginStatus::parse(st).ok_or_else(|| {
        PluginError::schema(format!(
            "status {st:?} must be active, experimental, demo or withdrawn"
        ))
    })?;
    let title = doc
        .opt("title")
        .map(|t| t.str().map(str::to_string))
        .transpose()?;
    Ok(Header {
        format: format.to_string(),
        id,
        version,
        engine_min,
        status,
        title,
    })
}

/// The first `n` characters of a text, for messages (never slices inside a UTF-8 character: Codex Phase 2
/// HIGH 1; a malformed file must be disabled, never panic).
pub fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Lower-case hex SHA-256 of bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

/// Content hash of one spectrum (04 section 6.2):
/// sha256(b"spyder-spectrum-v1\0" + float64-LE wavelengths + float64-LE values).
pub fn spectrum_sha256(wl: &[f64], x: &[f64]) -> String {
    let mut h = Sha256::new();
    h.update(b"spyder-spectrum-v1\0");
    for v in wl {
        h.update(v.to_le_bytes());
    }
    for v in x {
        h.update(v.to_le_bytes());
    }
    hex(&h.finalize())
}

/// `true` for 64 lower-case hex characters.
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// The standard-resolution instrument class (PLAN section 7).
pub const CLASS_STD: &str = "asd.labspec4.std";
/// The high-resolution instrument class.
pub const CLASS_HIRES: &str = "asd.labspec4.hires";

/// What preprocessing and transfer selection need to know about a scan (spyder_ref `ScanContext`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScanContext {
    /// The file's own detector joins (nm).
    pub splices_nm: Vec<f64>,
    /// The instrument class (the user's Standard / High-res switch, DECISIONS 50).
    pub instrument_class: String,
    /// The instrument serial, as text (transfer files may key on it).
    pub serial: Option<String>,
}

impl ScanContext {
    pub fn new(splices_nm: Vec<f64>, instrument_class: &str, serial: Option<String>) -> Self {
        ScanContext {
            splices_nm,
            instrument_class: instrument_class.to_string(),
            serial,
        }
    }

    /// From a golden case's `scan_context` (defaults: joins 1000/1800, class std, no serial).
    pub fn from_json(n: Option<&Node>) -> PResult<Self> {
        let mut c = ScanContext::new(vec![1000.0, 1800.0], CLASS_STD, None);
        let Some(n) = n else { return Ok(c) };
        n.obj()?;
        if let Some(s) = n.opt("splices_nm") {
            c.splices_nm = s.vec_f64()?;
        }
        if let Some(s) = n.opt("instrument_class") {
            c.instrument_class = s.str()?.to_string();
        }
        if let Some(s) = n.opt("serial") {
            c.serial = serial_text(s.v);
        }
        Ok(c)
    }
}

/// A serial given as an integer or a string, as text; None for null or anything else.
pub fn serial_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => n.as_u64().map(|u| u.to_string()),
        _ => None,
    }
}
