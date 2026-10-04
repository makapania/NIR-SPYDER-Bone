//! The app's scoring operations, free of Tauri so that tests drive exactly what the commands run:
//! reading files into a session, the UI's results for one analysis type, the display arrays and the export.

use std::path::{Path, PathBuf};

use serde::Serialize;
use spyder_core::pipeline::export::{csv_row, csv_text};
use spyder_core::pipeline::val::Obj;
use spyder_core::pipeline::ANALYSES;

use crate::display::{clamp_smoothing, RefMeta};
use crate::engine::Core;
use crate::mapping::{scan_result, ScanResult};
use crate::session::{ui_class, Input, NewScan, Session};

/// The UI's `SessionInfo`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub folder: String,
    pub watching: bool,
    pub instrument_class: &'static str,
    pub class_source: &'static str,
    pub serial: Option<u64>,
    pub serial_class: Option<&'static str>,
    pub example: bool,
    pub count: usize,
}

pub fn session_info(core: &Core, s: &Session, watching: bool) -> SessionInfo {
    let (serial, serial_class) = s.serial_info(core);
    SessionInfo {
        folder: s.folder.clone().unwrap_or_default(),
        watching,
        instrument_class: ui_class(s.class),
        class_source: s.source.ui_str(),
        serial,
        serial_class,
        example: false,
        count: s.entries.len(),
    }
}

/// Whether a file name is an `.asd` scan (case-insensitive), as the watcher decides.
pub fn is_asd(p: &Path) -> bool {
    p.file_name()
        .is_some_and(spyder_watch::filter::is_candidate_name)
}

/// The `.asd` files directly in a folder (not sub-folders: DECISIONS 64), sorted by name.
pub fn folder_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let rd =
        std::fs::read_dir(folder).map_err(|e| format!("cannot open {}: {e}", folder.display()))?;
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_asd(p))
        .collect();
    v.sort();
    Ok(v)
}

fn modified_ms(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(spyder_watch::unix_ms(m))
}

/// One file read into a new scan (an unreadable file is listed, never silently dropped).
pub fn read_scan(p: &Path) -> NewScan {
    let file = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Same cap as the watcher: a file far larger than any ASD scan is listed, never read into memory.
    let input = match std::fs::metadata(p) {
        Ok(m) if m.len() > spyder_watch::MAX_SCAN_BYTES => Input::Unreadable(format!(
            "{} MB is far larger than any ASD scan; not read",
            m.len() / (1024 * 1024)
        )),
        _ => match std::fs::read(p) {
            Ok(b) => Input::Asd(b.into()),
            Err(e) => Input::Unreadable(format!("cannot read file: {e}")),
        },
    };
    NewScan {
        path: p.to_string_lossy().into_owned(),
        file,
        input,
        arrived_seq: None,
        revision: 1,
        modified_ms: modified_ms(p),
        log_meta: None,
    }
}

/// Add files to a session (each analysed under the session's class).
pub fn add_files(core: &Core, s: &mut Session, files: &[PathBuf]) -> usize {
    for p in files {
        s.add(core, read_scan(p));
    }
    files.len()
}

/// The UI's results for one analysis type, in session order (the UI sorts).
pub fn results(core: &Core, s: &Session, analysis: &str) -> Vec<ScanResult> {
    let a = if ANALYSES.contains(&analysis) {
        analysis
    } else {
        "radiocarbon"
    };
    s.entries
        .iter()
        .map(|e| scan_result(core, e, a, s.class, s.source))
        .collect()
}

/// The core's CSV (UTF-8 with BOM; byte-identical to `spyder analyse --csv`) for the session's scored scans,
/// under the current class and the given analysis type. Returns (text, rows).
pub fn export_csv(s: &Session, analysis: &str) -> (String, usize) {
    let rows: Vec<Obj> = s
        .entries
        .iter()
        .filter_map(|e| e.record.as_deref())
        .map(|r| csv_row(r, analysis))
        .collect();
    (csv_text(&rows), rows.len())
}

/// The eight display views of one scan (`display::VIEW_ORDER`), n values each.
pub fn scan_views(core: &Core, s: &Session, id: &str, smoothing: u32) -> Result<Vec<f64>, String> {
    let e = s
        .entry(id)
        .ok_or_else(|| format!("no scan {id} in this session"))?;
    let reg = core.registry().ok_or("no verdict model is loaded")?;
    let eng = core.engine().ok_or("no verdict model is loaded")?;
    let d = e.draw().ok_or("this file cannot be drawn")?;
    Ok(core.display.scan_views(
        reg,
        &eng,
        &d.as_input(),
        s.class,
        clamp_smoothing(smoothing),
    ))
}

/// The reference spectra's meta and their views.
pub fn reference_views(core: &Core, smoothing: u32) -> (Vec<RefMeta>, Vec<f64>) {
    match core.registry() {
        Some(reg) => core
            .display
            .reference_views(reg, clamp_smoothing(smoothing)),
        None => (Vec::new(), Vec::new()),
    }
}

#[cfg(test)]
mod size_cap_tests {
    use super::*;

    #[test]
    fn a_file_larger_than_any_scan_is_listed_unread() {
        let dir = std::env::temp_dir().join(format!("spyder_cap_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("huge.asd");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(spyder_watch::MAX_SCAN_BYTES + 1).unwrap(); // sparse: nothing is written
        drop(f);
        let s = read_scan(&big);
        let _ = std::fs::remove_dir_all(&dir);
        match s.input {
            Input::Unreadable(r) => assert!(r.contains("far larger than any ASD scan"), "{r}"),
            _ => panic!("an oversized file must not be read"),
        }
    }
}
