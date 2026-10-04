//! The JSONL session log (PLAN Step 10; 03_architecture section 6.1): one JSON record per line, appended per
//! scan and fsynced, so a crash loses at most the record being written.
//!
//! Recovery: when a log is opened, trailing damage from a crash mid-append (a last line without its newline, or
//! a last line that does not parse) is cut off, and the cut is reported. Damage anywhere else is left in place
//! (it cannot come from an interrupted append) and the reader skips it.
//!
//! File name: `<YYYY-MM-DD>_<folder name>_<8 hex of the folder key>.jsonl` (UTC date), one per watched folder
//! per day, so re-watching the same folder the same day appends to the same file.
//!
//! Result revisions (PLAN Step 10: "reanalysis writes a new revision"): a scan record may carry a `resultKey`
//! (the caller's digest of input identity + effective configuration + dependency hashes) and a
//! `resultRevision` (1, 2, ... per path and content hash). A record whose `resultKey` is already in the file is
//! not appended again ([`SessionLog::contains_result`]); the same file analysed under another configuration is
//! appended as the next revision ([`SessionLog::next_result_revision`]).

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What recovery did when the log was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    /// Bytes cut from the end of the file.
    pub dropped_bytes: u64,
    /// Complete records kept.
    pub kept_records: usize,
}

pub struct SessionLog {
    path: PathBuf,
    file: File,
    /// File length after the last complete record.
    committed: u64,
    /// Records whose append failed; written first on the next append.
    backlog: Vec<Vec<u8>>,
    seen: HashSet<(String, String)>,
    /// `resultKey`s in the file (or waiting in the backlog).
    results: HashSet<String>,
    /// The highest result revision per (path, content hash).
    revisions: HashMap<(String, String), u32>,
    records: usize,
    /// Set when opening cut a damaged tail.
    pub recovered: Option<Recovery>,
}

/// Splits `bytes` into the length of the valid prefix (complete, parseable lines, with interior damage kept)
/// and the parsed records.
fn scan(bytes: &[u8]) -> (usize, Vec<Value>) {
    let mut records = Vec::new();
    let mut good_end = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        let Some(nl) = bytes[pos..].iter().position(|&b| b == b'\n') else {
            break; // unterminated last line: an interrupted append
        };
        let line = &bytes[pos..pos + nl];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let next = pos + nl + 1;
        if line.iter().all(|b| b.is_ascii_whitespace()) {
            good_end = next;
        } else if let Ok(v) = serde_json::from_slice::<Value>(line) {
            records.push(v);
            good_end = next;
        }
        // A bad line leaves good_end where it was; if a later line parses, good_end moves past it again, so only
        // TRAILING damage is ever cut.
        pos = next;
    }
    (good_end, records)
}

/// Reads every record of a log without modifying it (damaged lines skipped).
pub fn read_records(path: &Path) -> io::Result<Vec<Value>> {
    Ok(scan(&fs::read(path)?).1)
}

impl SessionLog {
    /// Opens (or creates) a log for appending, recovering a damaged tail first.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<SessionLog> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;
        let bytes = fs::read(&path)?;
        let (good_end, records) = scan(&bytes);
        let mut recovered = None;
        if good_end < bytes.len() {
            // Truncate through a separate write handle (append handles cannot always set_len).
            let w = OpenOptions::new().write(true).open(&path)?;
            w.set_len(good_end as u64)?;
            w.sync_all()?;
            recovered = Some(Recovery {
                dropped_bytes: (bytes.len() - good_end) as u64,
                kept_records: records.len(),
            });
        }
        let mut log = SessionLog {
            path,
            file,
            committed: good_end as u64,
            backlog: Vec::new(),
            seen: HashSet::new(),
            results: HashSet::new(),
            revisions: HashMap::new(),
            records: records.len(),
            recovered,
        };
        for r in &records {
            log.register(r);
        }
        Ok(log)
    }

    /// Indexes one record (identity, result key, result revision).
    fn register(&mut self, r: &Value) {
        if let Some(k) = r.get("resultKey").and_then(Value::as_str) {
            self.results.insert(k.to_string());
        }
        let (Some(p), Some(s)) = (
            r.get("path").and_then(Value::as_str),
            r.get("sha256").and_then(Value::as_str),
        ) else {
            return;
        };
        let id = (p.to_string(), s.to_string());
        let rev = self.revisions.entry(id.clone()).or_insert(0);
        // Records written before result revisions existed count as one revision each.
        let this = r
            .get("resultRevision")
            .and_then(Value::as_u64)
            .map_or(*rev + 1, |v| v.min(u64::from(u32::MAX)) as u32);
        *rev = (*rev).max(this);
        self.seen.insert(id);
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn records(&self) -> usize {
        self.records
    }

    /// True if a scan record with this path and content hash is already in the log.
    pub fn contains(&self, path: &str, sha256: &str) -> bool {
        self.seen.contains(&(path.to_string(), sha256.to_string()))
    }

    /// True if a record with this `resultKey` (input identity + effective configuration + dependency hashes) is
    /// already in the log.
    pub fn contains_result(&self, key: &str) -> bool {
        self.results.contains(key)
    }

    /// The result revision the next record for this path and content hash gets (1 for the first).
    pub fn next_result_revision(&self, path: &str, sha256: &str) -> u32 {
        self.revisions
            .get(&(path.to_string(), sha256.to_string()))
            .map_or(1, |r| r + 1)
    }

    /// Records waiting to be written after an earlier failure (disk full, share gone).
    pub fn backlog(&self) -> usize {
        self.backlog.len()
    }

    /// Appends one record as a single line and fsyncs it. On failure the file is cut back to the last complete
    /// record (so a partial line never joins the next one) and the record is kept for the next append.
    pub fn append(&mut self, record: &Value) -> io::Result<()> {
        self.append_all(std::slice::from_ref(record))
    }

    /// Appends several records with one write and one fsync (a reanalysis of a whole session). A record is
    /// indexed as soon as it is queued: a failed write keeps it in the backlog, so it is never queued twice.
    pub fn append_all(&mut self, records: &[Value]) -> io::Result<()> {
        for record in records {
            let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
            line.push(b'\n');
            self.backlog.push(line);
            self.register(record);
            self.records += 1;
        }
        let pending: Vec<u8> = self.backlog.concat();
        let res = self
            .file
            .write_all(&pending)
            .and_then(|_| self.file.sync_data());
        if let Err(e) = res {
            if let Ok(w) = OpenOptions::new().write(true).open(&self.path) {
                let _ = w.set_len(self.committed);
                let _ = w.sync_all();
            }
            return Err(e);
        }
        self.committed += pending.len() as u64;
        self.backlog.clear();
        Ok(())
    }
}

/// `<YYYY-MM-DD>_<folder name>_<hash8>.jsonl` for `folder` on the UTC date of `now`.
pub fn session_file_name(folder: &Path, now: SystemTime) -> String {
    let (y, m, d) = civil_date(crate::unix_ms(now).div_euclid(86_400_000));
    let name: String = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".into())
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let name = if name.is_empty() {
        "folder".to_string()
    } else {
        name
    };
    let key = crate::settings::folder_key(&folder.to_string_lossy());
    let h = Sha256::digest(key.as_bytes());
    format!("{y:04}-{m:02}-{d:02}_{name}_{}.jsonl", crate::hex(&h[..4]))
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar (H. Hinnant's algorithm).
pub fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("spyder-session-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("s.jsonl")
    }

    #[test]
    fn append_and_reopen() {
        let p = tmp("append");
        let mut log = SessionLog::open(&p).unwrap();
        log.append(&json!({"kind":"scan","path":"a.asd","sha256":"01"}))
            .unwrap();
        log.append(&json!({"kind":"scan","path":"b.asd","sha256":"02"}))
            .unwrap();
        drop(log);
        let log = SessionLog::open(&p).unwrap();
        assert_eq!(log.records(), 2);
        assert!(log.recovered.is_none());
        assert!(log.contains("a.asd", "01"));
        assert!(!log.contains("a.asd", "02"));
    }

    #[test]
    fn result_keys_and_revisions_survive_reopening() {
        let p = tmp("revisions");
        let mut log = SessionLog::open(&p).unwrap();
        // a record from before result revisions existed counts as revision 1
        log.append(&json!({"kind":"scan","path":"a.asd","sha256":"01"}))
            .unwrap();
        assert_eq!(log.next_result_revision("a.asd", "01"), 2);
        assert_eq!(log.next_result_revision("a.asd", "02"), 1);
        log.append_all(&[
            json!({"kind":"scan","path":"a.asd","sha256":"01","resultKey":"k-std","resultRevision":2}),
            json!({"kind":"scan","path":"a.asd","sha256":"01","resultKey":"k-hires","resultRevision":3}),
        ])
        .unwrap();
        drop(log);
        let log = SessionLog::open(&p).unwrap();
        assert_eq!(log.records(), 3);
        assert!(log.contains_result("k-std") && log.contains_result("k-hires"));
        assert!(!log.contains_result("k-other"));
        assert_eq!(log.next_result_revision("a.asd", "01"), 4);
    }

    #[test]
    fn truncated_last_record_is_recovered() {
        let p = tmp("trunc");
        let mut log = SessionLog::open(&p).unwrap();
        log.append(&json!({"kind":"scan","path":"a.asd","sha256":"01"}))
            .unwrap();
        drop(log);
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(br#"{"kind":"scan","path":"b.as"#).unwrap();
        drop(f);
        let mut log = SessionLog::open(&p).unwrap();
        assert_eq!(
            log.recovered,
            Some(Recovery {
                dropped_bytes: 27,
                kept_records: 1
            })
        );
        log.append(&json!({"kind":"scan","path":"c.asd","sha256":"03"}))
            .unwrap();
        drop(log);
        let r = read_records(&p).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[1]["path"], "c.asd");
    }

    #[test]
    fn terminated_but_garbled_tail_is_cut_and_interior_damage_kept() {
        let p = tmp("garbled");
        fs::write(&p, b"{\"a\":1}\nGARBAGE\n{\"b\":2}\n\0\0\0\0\n").unwrap();
        let log = SessionLog::open(&p).unwrap();
        assert_eq!(log.recovered.as_ref().unwrap().dropped_bytes, 5);
        assert_eq!(log.records(), 2);
        assert_eq!(fs::read(&p).unwrap(), b"{\"a\":1}\nGARBAGE\n{\"b\":2}\n");
    }

    #[test]
    fn file_names() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_791_000_000);
        let n = session_file_name(Path::new("/Volumes/lab/Drop Folder"), t);
        assert!(n.starts_with("2026-10-03_Drop_Folder_"), "{n}");
        assert!(n.ends_with(".jsonl"));
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(-1), (1969, 12, 31));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
    }
}
