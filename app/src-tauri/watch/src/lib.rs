//! SPYDER Bone live folder watching (PLAN v3.2 section 8 Phase 5; 03_architecture section 4).
//!
//! The user points the app at a folder: the LabSpec save folder, a folder on a Mac that files are dropped into,
//! or a network share. It is treated like any other folder (DECISIONS 38, 55): no assumption about how the
//! instrument software saves a file.
//!
//! * [`FolderWatch`]: native change events (ReadDirectoryChangesW / FSEvents / inotify through `notify`) plus a
//!   periodic rescan as the safety net, or POLL mode (automatic on network volumes, see [`volume`]).
//! * The settle rule ([`watcher`] module docs): a file is read only after its size and modification time have
//!   stayed unchanged for the settle window; it is read whole into memory (an immutable snapshot), re-checked,
//!   and checked for structural completeness ([`asd_shape`]) before it is delivered. A half-written file is
//!   never delivered as a scan.
//! * [`settings`]: the versioned settings file (watched folders, the per-folder Standard / High-res switch).
//! * [`session`]: the JSONL session log, appended per scan with fsync; a truncated last record is recovered.
//!
//! Scoring is not here: an arrival carries the path, the bytes and the file metadata, and the core scores it.

pub mod asd_shape;
pub mod filter;
pub mod session;
pub mod settings;
pub mod volume;
mod watcher;

#[cfg(target_os = "macos")]
pub mod macos;

pub use watcher::{
    ActiveMode, Arrival, FolderWatch, ModeChoice, Timing, WatchConfig, WatchEvent, WatchState,
    WatchStatus, MAX_SCAN_BYTES,
};

/// Lower-case hex of a byte slice (SHA-256 fingerprints).
pub fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

/// Milliseconds since the Unix epoch (negative before 1970).
pub fn unix_ms(t: std::time::SystemTime) -> i64 {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}
