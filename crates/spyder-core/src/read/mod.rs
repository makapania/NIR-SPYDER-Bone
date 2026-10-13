//! File readers: ASD `as8` (LabSpec 4), `as7` (Trek-type handheld) and legacy v1 reflectance files (FieldSpec 3
//! `.NNN` / ViewSpec `.sco`) in the verified acquisition modes (PLAN section 3 Step 1, DECISIONS 43, 87-88). Every
//! reader returns `Err` for any malformed input; none ever panics.

pub mod asd;

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::scan::Scan;

/// Why a file could not be read. `kind()` gives a stable machine-readable tag.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ReadError {
    /// The file ends before a structure it must contain.
    #[error("truncated file: {what} needs {needed} bytes, file has {got}")]
    Truncated {
        what: &'static str,
        needed: usize,
        got: usize,
    },
    /// Not an ASD file at all (wrong magic).
    #[error("not an ASD file (magic {magic:?})")]
    NotAsd { magic: String },
    /// A real ASD file outside the supported-input matrix (e.g. an old version, the wrong data type).
    #[error("unsupported: {reason}")]
    Unsupported { reason: String },
    /// An ASD file in the supported layout whose content is unusable (e.g. reference not > 0).
    #[error("invalid: {reason}")]
    Invalid { reason: String },
    /// The file could not be read from disk.
    #[error("cannot read file: {reason}")]
    Io { reason: String },
}

impl ReadError {
    /// Stable tag: "truncated", "not_asd", "unsupported", "invalid", "io".
    pub fn kind(&self) -> &'static str {
        match self {
            ReadError::Truncated { .. } => "truncated",
            ReadError::NotAsd { .. } => "not_asd",
            ReadError::Unsupported { .. } => "unsupported",
            ReadError::Invalid { .. } => "invalid",
            ReadError::Io { .. } => "io",
        }
    }
}

/// Read a scan from bytes (the watcher passes immutable snapshots).
pub fn read_bytes(bytes: &[u8]) -> Result<Scan, ReadError> {
    asd::read_any(bytes)
}

/// Read a scan from a file on disk.
pub fn read_file(path: &Path) -> Result<Scan, ReadError> {
    let bytes = std::fs::read(path).map_err(|e| ReadError::Io {
        reason: e.to_string(),
    })?;
    read_bytes(&bytes)
}

/// Whether a file name could be an ASD scan: `.asd`, `.sco` (ViewSpec) or a three-digit extension (`.000`, the
/// FieldSpec 3 numbering), any case.
pub fn is_scan_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some((stem, ext)) = lower.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (ext == "asd"
            || ext == "sco"
            || (ext.len() == 3 && ext.bytes().all(|b| b.is_ascii_digit())))
}

/// A ViewSpec `X.sco` whose source `X` is in the same folder: the same scan, splice-corrected. The source (as
/// measured) is scored and the `.sco` is skipped, so a scan is never counted twice (DECISIONS 88).
pub fn is_shadowed_sco(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.len() <= 4 || !name.to_ascii_lowercase().ends_with(".sco") {
        return false;
    }
    let source = &name[..name.len() - 4];
    is_scan_name(source)
        && !source.to_ascii_lowercase().ends_with(".sco")
        && path.with_file_name(source).is_file()
}

/// ASD scan files (see [`is_scan_name`]) directly inside `dir`, sorted by file name. Hidden files, macOS AppleDouble
/// companions (`._name.asd`) and `.sco` files shadowed by their source ([`is_shadowed_sco`]) are skipped.
pub fn list_asd_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if is_scan_name(&name) && !is_shadowed_sco(&path) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// `true` for a symlink, or a directory that is really somewhere else (a Windows junction is not reported as a
/// symlink by `file_type()`, so it is detected by its canonical path).
pub fn is_link_or_junction(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => return true,
        Ok(_) => {}
        Err(_) => return false,
    }
    if !path.is_dir() {
        return false;
    }
    match (
        std::fs::canonicalize(path),
        path.parent().map(std::fs::canonicalize),
    ) {
        (Ok(c), Some(Ok(p))) => c.parent() != Some(p.as_path()),
        _ => false,
    }
}

/// ASD scan files in `dir` and, recursively, its sub-folders (sorted per folder, folder by folder). Symlinked and
/// junction directories are never followed and each real directory is visited once (Codex Phase 1 review,
/// MEDIUM 4: no directory cycles).
pub fn list_asd_files_recursive(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    walk_asd(dir, &mut out, &mut seen)?;
    Ok(out)
}

fn walk_asd(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
) -> std::io::Result<()> {
    if !seen.insert(std::fs::canonicalize(dir)?) {
        return Ok(());
    }
    out.extend(list_asd_files(dir)?);
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !is_link_or_junction(p))
        .collect();
    subs.sort();
    for s in subs {
        walk_asd(&s, out, seen)?;
    }
    Ok(())
}
