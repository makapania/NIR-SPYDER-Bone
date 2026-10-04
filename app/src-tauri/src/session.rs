//! The scans on screen (one session: an opened folder or files, or a watched folder) with their immutable
//! input bytes and their core records. The Standard / High-res switch is per session (remembered per folder by
//! the settings file); flipping it re-analyses every scan from the cached bytes (cheap). The record holds all
//! three analysis types; the app shows the radiocarbon / isotopes verdict for every analysis, with the ZooMS line
//! (DECISIONS 80 amended: no analysis picker).
//!
//! A watched folder's session also owns its JSONL session log: every analysis of a logged scan (its arrival,
//! and every reanalysis after the switch or a serial preset) is appended as a result revision, deduplicated on
//! input identity + effective configuration + dependency hashes (PLAN Step 10).
//!
//! No Tauri types here: the integration tests drive a session directly.

use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::Context;
use spyder_core::plugins::{sha256_hex, CLASS_HIRES, CLASS_STD};
use spyder_watch::session::SessionLog;

use crate::display::DrawInput;
use crate::engine::Core;

/// Who set the instrument class (DECISIONS 50): the user's switch, a known serial's preset, a preset from the
/// header's SWIR detector gains (an unlisted serial; a heuristic), or the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassSource {
    User,
    Preset,
    Header,
    Default,
}

impl ClassSource {
    /// The value recorded in the core record and the CSV (`class_source`).
    pub fn record_str(self) -> &'static str {
        match self {
            ClassSource::User => "user",
            ClassSource::Preset => "preset",
            ClassSource::Header => "header_hint",
            ClassSource::Default => "default",
        }
    }

    /// The UI's `ClassSource`.
    pub fn ui_str(self) -> &'static str {
        match self {
            ClassSource::User => "user",
            ClassSource::Preset => "serial_preset",
            ClassSource::Header => "header_preset",
            ClassSource::Default => "default",
        }
    }
}

/// The UI's class names.
pub fn ui_class(class: &str) -> &'static str {
    if class == CLASS_HIRES {
        "hires"
    } else {
        "standard"
    }
}

/// A registry class name as the core's static class constant.
fn static_class(class: &str) -> &'static str {
    if class == CLASS_HIRES {
        CLASS_HIRES
    } else {
        CLASS_STD
    }
}

/// The core's class from the UI's name.
pub fn core_class(ui: &str) -> Option<&'static str> {
    match ui {
        "standard" | "std" | CLASS_STD => Some(CLASS_STD),
        "hires" | "high-res" | CLASS_HIRES => Some(CLASS_HIRES),
        _ => None,
    }
}

/// What a scan was made from.
#[derive(Clone, Debug)]
pub enum Input {
    /// An `.asd` file's bytes (an immutable snapshot).
    Asd(Arc<[u8]>),
    /// A reflectance spectrum on the 1 nm grid (tests and golden spectra).
    Spectrum {
        wl: Vec<f64>,
        r: Vec<f64>,
        joins: Vec<f64>,
        serial: Option<u64>,
    },
    /// Not a file this app can read (from the watcher's structure check): listed, never scored.
    Unreadable(String),
}

#[derive(Clone, Debug)]
pub struct NewScan {
    pub path: String,
    pub file: String,
    pub input: Input,
    pub arrived_seq: Option<u64>,
    pub revision: u32,
    pub modified_ms: Option<i64>,
    /// The file facts written into every JSONL record of this scan (`path` and `sha256` at least); None: the
    /// scan is not logged (opened files).
    pub log_meta: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub path: String,
    pub file: String,
    pub input: Input,
    pub arrived_seq: Option<u64>,
    pub revision: u32,
    pub modified_ms: Option<i64>,
    /// The class the file's header SWIR gains suggest (registry `header_hint`), if any. A heuristic: it presets
    /// the switch for an unlisted serial and adds a gentle note, never more.
    pub hint: Option<&'static str>,
    /// The core record for `cfg` (None: not scored: unreadable input, or no verdict model).
    pub record: Option<Arc<Obj>>,
    cfg: Option<(&'static str, ClassSource)>,
    /// Milliseconds the last analysis took (the performance budget).
    pub score_ms: f64,
    log_meta: Option<Value>,
    /// The configuration whose result was last handed to the session log.
    logged: Option<(&'static str, ClassSource)>,
}

impl Entry {
    /// The as-measured spectrum to draw (None: unreadable).
    pub fn draw(&self) -> Option<OwnedDraw> {
        match &self.input {
            Input::Asd(b) => {
                let s = spyder_core::read_bytes(b).ok()?;
                Some(OwnedDraw {
                    serial: Some(u64::from(s.header.serial)),
                    wl: s.wavelengths_nm,
                    r: s.reflectance,
                    joins: s.splices_nm,
                })
            }
            Input::Spectrum {
                wl,
                r,
                joins,
                serial,
            } => Some(OwnedDraw {
                wl: wl.clone(),
                r: r.clone(),
                joins: joins.clone(),
                serial: *serial,
            }),
            Input::Unreadable(_) => None,
        }
    }

    /// The header's SWIR1 and SWIR2 gains (u16 at bytes 436 and 438 of the 484-byte ASD header); None: not an
    /// `as8` file (the only version the core reads; older versions' gains may mean something else) or too short. Read from the header alone, so a hint never depends on the rest of the file.
    pub fn swir_gains(&self) -> Option<(u64, u64)> {
        let Input::Asd(b) = &self.input else {
            return None;
        };
        if b.len() < 484 || !b.starts_with(b"as8") {
            return None;
        }
        let g = |o: usize| u64::from(u16::from_le_bytes([b[o], b[o + 1]]));
        Some((g(436), g(438)))
    }

    /// The serial the record read from the file.
    pub fn serial(&self) -> Option<u64> {
        let r = self.record.as_ref()?;
        let v = r
            .get("instrument")
            .and_then(V::as_obj)
            .and_then(|o| o.get("serial"))
            .or_else(|| {
                r.get("input")
                    .and_then(V::as_obj)
                    .and_then(|o| o.get("serial"))
            })?;
        v.as_f64().filter(|x| *x > 0.0).map(|x| x as u64)
    }
}

pub struct OwnedDraw {
    pub wl: Vec<f64>,
    pub r: Vec<f64>,
    pub joins: Vec<f64>,
    pub serial: Option<u64>,
}

impl OwnedDraw {
    pub fn as_input(&self) -> DrawInput<'_> {
        DrawInput {
            wl: &self.wl,
            r: &self.r,
            joins: &self.joins,
            serial: self.serial,
        }
    }
}

pub struct Session {
    /// Changes whenever a new session replaces this one (a watcher of an old folder must not add here).
    pub gen: u64,
    pub folder: Option<String>,
    /// A watched folder (arrival order matters) rather than opened files.
    pub live: bool,
    pub class: &'static str,
    pub source: ClassSource,
    pub entries: Vec<Entry>,
    next: u64,
    log: Option<SessionLog>,
}

/// The JSONL dedup key of one result: input identity (path, content hash) + effective configuration (class and
/// its source, engine version) + every dependency hash the record names.
pub fn result_key(path: &str, sha256: &str, class: &str, source: &str, record: &Value) -> String {
    let basis = json!([
        path,
        sha256,
        class,
        source,
        record.get("oracle_version"),
        record.get("dependencies"),
    ]);
    sha256_hex(basis.to_string().as_bytes())
}

fn score(core: &Core, e: &Entry, class: &str, source: ClassSource) -> Option<Obj> {
    match &e.input {
        Input::Asd(b) => core.analyse_bytes(b, &e.file, class, source.record_str()),
        Input::Spectrum {
            wl,
            r,
            joins,
            serial,
        } => core.engine().map(|eng| {
            eng.analyse_spectrum(
                wl,
                r,
                &Context {
                    instrument_class: class.to_string(),
                    class_source: source.record_str().to_string(),
                    serial: *serial,
                    splices_nm: joins.clone(),
                },
            )
        }),
        Input::Unreadable(_) => None,
    }
}

impl Session {
    /// A new, empty session. `remembered`: the folder's remembered switch (the user set it before).
    pub fn new(
        gen: u64,
        folder: Option<String>,
        live: bool,
        remembered: Option<&'static str>,
    ) -> Session {
        Session {
            gen,
            folder,
            live,
            class: remembered.unwrap_or(CLASS_STD),
            source: if remembered.is_some() {
                ClassSource::User
            } else {
                ClassSource::Default
            },
            entries: Vec::new(),
            next: 1,
            log: None,
        }
    }

    /// Give the session its JSONL session log (a watched folder).
    pub fn with_log(mut self, log: Option<SessionLog>) -> Session {
        self.log = log;
        self
    }

    /// The session log's path, if this session writes one.
    pub fn log_path(&self) -> Option<&std::path::Path> {
        self.log.as_ref().map(SessionLog::path)
    }

    /// Append a result revision for every logged scan whose analysis changed since it was last logged (its
    /// arrival, or a reanalysis), unless the log already holds that exact result (same input, configuration and
    /// dependency hashes: a folder re-watched under the same switch). One write and one fsync for the batch.
    fn log_results(&mut self) {
        let Some(log) = self.log.as_mut() else {
            return;
        };
        let mut out = Vec::new();
        for e in &mut self.entries {
            let (Some(meta), Some(cfg)) = (&e.log_meta, e.cfg) else {
                continue;
            };
            if e.logged == Some(cfg) {
                continue;
            }
            let reason = if e.logged.is_some() {
                "reanalysis"
            } else {
                "arrival"
            };
            e.logged = Some(cfg);
            let result = e
                .record
                .as_deref()
                .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            let (class, source) = (ui_class(cfg.0), cfg.1.ui_str());
            let sha = meta.get("sha256").and_then(Value::as_str).unwrap_or("");
            let key = result_key(&e.path, sha, class, source, &result);
            if log.contains_result(&key) {
                continue;
            }
            let mut rec = json!({
                "v": 1,
                "kind": "scan",
                "reason": reason,
                "loggedMs": spyder_watch::unix_ms(std::time::SystemTime::now()),
            });
            if let (Some(r), Some(m)) = (rec.as_object_mut(), meta.as_object()) {
                for (k, v) in m {
                    r.entry(k.clone()).or_insert_with(|| v.clone());
                }
                r.insert("path".into(), Value::from(e.path.clone()));
                r.insert("sha256".into(), Value::from(sha));
                // The spyder-core record (all three analysis types) under this configuration.
                r.insert("resultClass".into(), Value::from(class));
                r.insert("classSource".into(), Value::from(source));
                r.insert("resultKey".into(), Value::from(key));
                r.insert(
                    "resultRevision".into(),
                    Value::from(log.next_result_revision(&e.path, sha)),
                );
                r.insert("result".into(), result);
            }
            out.push(rec);
        }
        // One path per session entry, so the revisions numbered above cannot collide within the batch.
        if !out.is_empty() {
            if let Err(err) = log.append_all(&out) {
                eprintln!(
                    "SPYDER Bone: session log append failed ({err}); kept for the next append"
                );
            }
        }
    }

    fn score_entry(&mut self, core: &Core, i: usize) {
        let (class, source) = (self.class, self.source);
        let e = &mut self.entries[i];
        if e.cfg == Some((class, source)) {
            return;
        }
        let t0 = Instant::now();
        e.record = score(core, e, class, source).map(Arc::new);
        e.score_ms = t0.elapsed().as_secs_f64() * 1e3;
        e.cfg = Some((class, source));
    }

    /// Add (or replace, for a file rewritten on disk) one scan and analyse it. Returns its id. If the class was
    /// never set for this folder and the file's serial is a known unit of the other class, the switch is
    /// preset from it (DECISIONS 50) and the session re-analysed.
    pub fn add(&mut self, core: &Core, s: NewScan) -> String {
        // Unique across sessions (the generation is part of it), so the UI never mistakes a new session's scan
        // for an old one's.
        let id = format!("s{}-{}", self.gen, self.next);
        self.next += 1;
        self.entries.retain(|e| e.path != s.path);
        self.entries.push(Entry {
            id: id.clone(),
            path: s.path,
            file: s.file,
            input: s.input,
            arrived_seq: s.arrived_seq,
            revision: s.revision,
            modified_ms: s.modified_ms,
            hint: None,
            record: None,
            cfg: None,
            score_ms: 0.0,
            log_meta: s.log_meta,
            logged: None,
        });
        let i = self.entries.len() - 1;
        let hint = self.entries[i]
            .swir_gains()
            .and_then(|(g1, g2)| core.class_of_swir_gains(g1, g2))
            .map(static_class);
        self.entries[i].hint = hint;
        self.score_entry(core, i);
        // A listed serial presets the switch (and outranks an earlier header preset); else the first file whose
        // header gains suggest a class presets it. The user's switch is never touched.
        if matches!(self.source, ClassSource::Default | ClassSource::Header) {
            let known = self.entries[i]
                .serial()
                .and_then(|s| core.class_of_serial(s))
                .map(static_class);
            let preset = match (known, hint) {
                (Some(k), _) => Some((k, ClassSource::Preset)),
                (None, Some(h)) if self.source == ClassSource::Default => {
                    Some((h, ClassSource::Header))
                }
                _ => None,
            };
            if let Some((class, source)) = preset {
                self.class = class;
                self.source = source;
                self.rescore(core);
            }
        }
        self.log_results();
        id
    }

    /// The user's switch: re-analyse every scan.
    pub fn set_class(&mut self, core: &Core, class: &'static str) {
        self.class = class;
        self.source = ClassSource::User;
        self.rescore(core);
    }

    /// Bring every record up to the current class (parallel; each analysis is independent).
    pub fn rescore(&mut self, core: &Core) {
        let (class, source) = (self.class, self.source);
        let todo: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].cfg != Some((class, source)))
            .collect();
        if todo.len() < 4 {
            for i in todo {
                self.score_entry(core, i);
            }
            self.log_results();
            return;
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 8);
        let chunk = todo.len().div_ceil(threads);
        let entries = &self.entries;
        let results: Vec<(usize, Option<Obj>, f64)> = std::thread::scope(|sc| {
            let hs: Vec<_> = todo
                .chunks(chunk)
                .map(|ix| {
                    sc.spawn(move || {
                        ix.iter()
                            .map(|&i| {
                                let t0 = Instant::now();
                                let r = score(core, &entries[i], class, source);
                                (i, r, t0.elapsed().as_secs_f64() * 1e3)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect()
        });
        for (i, r, ms) in results {
            let e = &mut self.entries[i];
            e.record = r.map(Arc::new);
            e.score_ms = ms;
            e.cfg = Some((class, source));
        }
        self.log_results();
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The first serial in the session, and the class the registry knows it as.
    pub fn serial_info(&self, core: &Core) -> (Option<u64>, Option<&'static str>) {
        let mut first = None;
        for e in &self.entries {
            if let Some(s) = e.serial() {
                if let Some(k) = core.class_of_serial(s) {
                    return (Some(s), Some(ui_class(k)));
                }
                first.get_or_insert(s);
            }
        }
        (first, None)
    }
}
