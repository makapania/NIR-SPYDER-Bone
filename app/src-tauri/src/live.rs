//! Live folder watching in the shell (PLAN v3.2 Phase 5): the IPC commands, the events the UI listens to, the
//! snapshot store, the settings file and the JSONL session log. The watching itself is `spyder-watch`.
//!
//! Events (Rust -> UI):
//! * `watch-arrival`: a settled, complete file. Carries the path, file metadata, the SHA-256, a `snapshotId`
//!   (the immutable bytes stay in the snapshot store) and the `scanId` of its result: the snapshot is analysed
//!   by spyder-core into the session BEFORE the event is sent, and the result is written into the JSONL record.
//!   The session owns the log: a reanalysis (the switch, a serial preset) appends a new result revision.
//! * `watch-incomplete`: a file that did not complete in time ("will retry when the file changes").
//! * `watch-status`: watching / paused / folder missing / stopped, the mode (native or poll) and why.
//!
//! Files: `<config>/settings.json` (versioned) and `<data>/sessions/<date>_<folder>_<hash>.jsonl`. Setting
//! `SPYDER_BONE_HOME` puts both under that folder instead (portable use, and tests that must not touch the
//! user's own settings).

use serde::Serialize;
use serde_json::json;
use spyder_watch::asd_shape::KindHint;
use spyder_watch::session::{session_file_name, SessionLog};
use spyder_watch::settings::{FolderSettings, InstrumentClass, Settings, SettingsStore};
use spyder_watch::volume::{self, VolumeKind};
use spyder_watch::{Arrival, FolderWatch, ModeChoice, WatchConfig, WatchEvent, WatchStatus};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{remembered_class, Shared};
use crate::session::{Input, NewScan};

/// Snapshots kept for the UI (each about 35 KB; the oldest are dropped first).
const SNAPSHOT_CAP: usize = 2000;

pub struct SnapshotStore {
    order: VecDeque<u64>,
    map: HashMap<u64, Arc<[u8]>>,
    next_id: u64,
}

impl SnapshotStore {
    fn new() -> Self {
        SnapshotStore {
            order: VecDeque::new(),
            map: HashMap::new(),
            next_id: 1,
        }
    }

    fn insert(&mut self, bytes: Arc<[u8]>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.map.insert(id, bytes);
        self.order.push_back(id);
        while self.order.len() > SNAPSHOT_CAP {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        id
    }

    fn get(&self, id: u64) -> Option<Arc<[u8]>> {
        self.map.get(&id).cloned()
    }
}

struct Current {
    watch: FolderWatch,
    folder: String,
    session_file: Option<String>,
}

pub struct Live {
    settings: Mutex<SettingsStore>,
    snapshots: Arc<Mutex<SnapshotStore>>,
    current: Mutex<Option<Current>>,
    sessions_dir: PathBuf,
    shared: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Live {
    pub fn new(app: &AppHandle, shared: Arc<Shared>) -> tauri::Result<Live> {
        let (config_dir, data_dir) = home_dirs(app)?;
        let settings = SettingsStore::load(config_dir.join("settings.json"));
        if let Some(n) = &settings.notice {
            eprintln!("SPYDER Bone settings: {n}");
        }
        Ok(Live {
            settings: Mutex::new(settings),
            snapshots: Arc::new(Mutex::new(SnapshotStore::new())),
            current: Mutex::new(None),
            sessions_dir: data_dir.join("sessions"),
            shared,
        })
    }

    pub fn is_watching(&self) -> bool {
        lock(&self.current).is_some()
    }

    /// Stop the running watch (if any) and forget it as the folder to resume; returns its folder.
    pub fn stop_current(&self) -> Option<String> {
        let c = lock(&self.current).take()?;
        let folder = c.folder.clone();
        c.watch.stop();
        let mut s = lock(&self.settings);
        if s.settings.resume_folder.as_deref() == Some(folder.as_str()) {
            s.settings.resume_folder = None;
            self.save_settings(&s);
        }
        Some(folder)
    }

    /// The switch the user set for this folder before, if any.
    pub fn remembered_class(&self, folder: &str) -> Option<&'static str> {
        remembered_class(
            lock(&self.settings)
                .folder(folder)
                .and_then(|f| f.instrument_class),
        )
    }

    /// Remember the user's switch for a folder.
    pub fn remember_class(&self, folder: &str, class: &str) {
        let mut s = lock(&self.settings);
        s.folder_mut(folder).instrument_class =
            Some(if class == spyder_core::plugins::CLASS_HIRES {
                InstrumentClass::Hires
            } else {
                InstrumentClass::Standard
            });
        self.save_settings(&s);
    }

    fn save_settings(&self, s: &SettingsStore) {
        if let Err(e) = s.save() {
            eprintln!("SPYDER Bone: settings not saved ({e})");
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrivalDto {
    pub seq: u64,
    pub snapshot_id: u64,
    pub folder: String,
    pub path: String,
    pub file: String,
    pub size: u64,
    pub modified_ms: Option<i64>,
    pub received_ms: i64,
    pub sha256: String,
    pub revision: u32,
    pub existing: bool,
    pub same_content_as: Option<String>,
    pub recognised: bool,
    /// "sample" or "white_reference_save" (structure only; the core has the final word).
    pub kind_hint: Option<&'static str>,
    pub detail: String,
    /// The session scan this arrival became (None: the session was replaced meanwhile).
    pub scan_id: Option<String>,
    /// Milliseconds spyder-core took to analyse the snapshot.
    pub score_ms: Option<f64>,
}

fn kind_str(k: Option<KindHint>) -> Option<&'static str> {
    k.map(|k| match k {
        KindHint::Sample => "sample",
        KindHint::WhiteReferenceSave => "white_reference_save",
    })
}

impl ArrivalDto {
    fn new(a: &Arrival, snapshot_id: u64, folder: &str) -> Self {
        ArrivalDto {
            seq: a.seq,
            snapshot_id,
            folder: folder.to_string(),
            path: a.path.to_string_lossy().into_owned(),
            file: a.file_name.clone(),
            size: a.size,
            modified_ms: a.modified_ms,
            received_ms: spyder_watch::unix_ms(SystemTime::now()),
            sha256: a.sha256.clone(),
            revision: a.revision,
            existing: a.existing,
            same_content_as: a
                .same_content_as
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            recognised: a.recognised,
            kind_hint: kind_str(a.kind_hint),
            detail: a.detail.clone(),
            scan_id: None,
            score_ms: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncompleteDto {
    pub path: String,
    pub file: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDto {
    #[serde(flatten)]
    pub status: WatchStatus,
    pub session_file: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeDto {
    pub folder: String,
    pub exists: bool,
    /// Candidate .asd files already in the folder.
    pub existing_count: usize,
    pub volume: VolumeKind,
    pub volume_detail: String,
    pub remembered: Option<FolderSettings>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub settings: Settings,
    pub notice: Option<String>,
    pub path: String,
}

fn count_candidates(folder: &Path) -> usize {
    std::fs::read_dir(folder)
        .map(|rd| {
            rd.flatten()
                .filter(|e| spyder_watch::filter::is_candidate_name(&e.file_name()))
                .filter(|e| {
                    e.metadata()
                        .is_ok_and(|m| m.is_file() && !spyder_watch::filter::is_hidden_meta(&m))
                })
                .count()
        })
        .unwrap_or(0)
}

#[tauri::command]
pub fn watch_probe(live: State<'_, Live>, folder: String) -> ProbeDto {
    let p = PathBuf::from(&folder);
    let vol = volume::detect(&p);
    ProbeDto {
        exists: p.is_dir(),
        existing_count: count_candidates(&p),
        volume: vol.kind,
        volume_detail: vol.detail,
        remembered: lock(&live.settings).folder(&folder).cloned(),
        folder,
    }
}

/// Starts watching `folder` (stopping any other watch). `include_existing` and `mode` default to what the
/// folder remembers (include: yes; mode: auto) and are remembered for next time.
/// Async for the same reason as [`watch_stop`]: starting a watch stops any other one first.
#[tauri::command]
pub async fn watch_start(
    app: AppHandle,
    live: State<'_, Live>,
    folder: String,
    include_existing: Option<bool>,
    mode: Option<ModeChoice>,
) -> Result<StatusDto, String> {
    start_inner(&app, &live, folder, include_existing, mode)
}

fn start_inner(
    app: &AppHandle,
    live: &Live,
    folder: String,
    include_existing: Option<bool>,
    mode: Option<ModeChoice>,
) -> Result<StatusDto, String> {
    // Stop the previous watch first (joins its thread).
    let old = lock(&live.current).take();
    if let Some(old) = old {
        old.watch.stop();
    }
    let (include, mode, remembered) = {
        let mut s = lock(&live.settings);
        let f = s.folder_mut(&folder);
        let remembered = remembered_class(f.instrument_class);
        if let Some(i) = include_existing {
            f.include_existing = Some(i);
        }
        if let Some(m) = mode {
            f.watch_mode = m;
        }
        let out = (f.include_existing.unwrap_or(true), f.watch_mode, remembered);
        s.settings.resume_folder = Some(folder.clone());
        live.save_settings(&s);
        out
    };

    let folder_path = PathBuf::from(&folder);
    #[cfg(target_os = "macos")]
    let folder_path = spyder_watch::macos::resolve_folder_access(&folder_path);

    let session_path = live
        .sessions_dir
        .join(session_file_name(&folder_path, SystemTime::now()));
    let session = match SessionLog::open(&session_path) {
        Ok(mut log) => {
            if let Some(r) = &log.recovered {
                eprintln!(
                    "SPYDER Bone: session log {} recovered ({} damaged bytes cut, {} records kept)",
                    session_path.display(),
                    r.dropped_bytes,
                    r.kept_records
                );
            }
            let _ = log.append(&json!({
                "v": 1,
                "kind": "watch_start",
                "atMs": spyder_watch::unix_ms(SystemTime::now()),
                "folder": folder,
                "includeExisting": include,
                "mode": mode,
                "app": "SPYDER Bone",
                "appVersion": env!("CARGO_PKG_VERSION"),
            }));
            Some(log)
        }
        Err(e) => {
            eprintln!("SPYDER Bone: session log unavailable ({e}); scans are not autosaved");
            None
        }
    };
    let session_file = session
        .as_ref()
        .map(|s| s.path().to_string_lossy().into_owned());

    // A new session for this folder; the watcher scores into it (and only into it). It owns the session log.
    let gen = live
        .shared
        .new_session(Some(folder.clone()), true, remembered, session);

    let cfg = WatchConfig {
        mode,
        include_existing: include,
        ..WatchConfig::new(folder_path)
    };
    let sink = make_sink(
        app.clone(),
        live.snapshots.clone(),
        folder.clone(),
        session_file.clone(),
        (live.shared.clone(), gen),
    );
    let watch = FolderWatch::start(cfg, sink).map_err(|e| format!("Cannot watch {folder}: {e}"))?;
    let status = StatusDto {
        status: watch.status(),
        session_file: session_file.clone(),
    };
    *lock(&live.current) = Some(Current {
        watch,
        folder,
        session_file,
    });
    Ok(status)
}

/// The file facts every JSONL record of an arrival carries (the session adds the result and its revision).
fn log_meta(dto: &ArrivalDto) -> serde_json::Value {
    json!({
        "seq": dto.seq,
        "receivedMs": dto.received_ms,
        "folder": dto.folder,
        "path": dto.path,
        "file": dto.file,
        "size": dto.size,
        "modifiedMs": dto.modified_ms,
        "sha256": dto.sha256,
        "revision": dto.revision,
        "existing": dto.existing,
        "recognised": dto.recognised,
        "kindHint": dto.kind_hint,
        "detail": dto.detail,
    })
}

/// Analyse an arrival's snapshot into the session (if it is still this watch's session) and log its result.
/// Returns the scan id and the analysis time.
fn score_arrival(
    shared: &Shared,
    gen: u64,
    a: &Arrival,
    meta: serde_json::Value,
) -> Option<(String, f64)> {
    let mut s = lock(&shared.session);
    if s.gen != gen {
        return None;
    }
    let input = if a.recognised {
        Input::Asd(a.bytes.clone())
    } else {
        Input::Unreadable(a.detail.clone())
    };
    let id = s.add(
        &shared.core,
        NewScan {
            path: a.path.to_string_lossy().into_owned(),
            file: a.file_name.clone(),
            input,
            arrived_seq: Some(a.seq),
            revision: a.revision,
            modified_ms: a.modified_ms,
            log_meta: Some(meta),
        },
    );
    let ms = s.entry(&id)?.score_ms;
    Some((id, ms))
}

fn make_sink(
    app: AppHandle,
    snapshots: Arc<Mutex<SnapshotStore>>,
    folder: String,
    session_file: Option<String>,
    (shared, gen): (Arc<Shared>, u64),
) -> impl FnMut(WatchEvent) + Send + 'static {
    move |ev| match ev {
        WatchEvent::Arrived(a) => {
            let id = lock(&snapshots).insert(a.bytes.clone());
            let mut dto = ArrivalDto::new(&a, id, &folder);
            // Scored and logged (the session owns the log) before the event is sent.
            if let Some((sid, ms)) = score_arrival(&shared, gen, &a, log_meta(&dto)) {
                dto.scan_id = Some(sid);
                dto.score_ms = Some(ms);
            }
            let _ = app.emit("watch-arrival", &dto);
        }
        WatchEvent::Incomplete {
            path,
            file_name,
            reason,
        } => {
            let _ = app.emit(
                "watch-incomplete",
                &IncompleteDto {
                    path: path.to_string_lossy().into_owned(),
                    file: file_name,
                    reason,
                },
            );
        }
        WatchEvent::Status(status) => {
            let _ = app.emit(
                "watch-status",
                &StatusDto {
                    status,
                    session_file: session_file.clone(),
                },
            );
        }
    }
}

/// Config and data folders: `$SPYDER_BONE_HOME` for both (portable use and tests), else the OS app folders.
pub fn home_dirs(app: &AppHandle) -> tauri::Result<(PathBuf, PathBuf)> {
    Ok(match std::env::var_os("SPYDER_BONE_HOME") {
        Some(home) => (PathBuf::from(&home), PathBuf::from(&home)),
        None => (app.path().app_config_dir()?, app.path().app_data_dir()?),
    })
}

fn current_status(live: &Live) -> Option<StatusDto> {
    lock(&live.current).as_ref().map(|c| StatusDto {
        status: c.watch.status(),
        session_file: c.session_file.clone(),
    })
}

#[tauri::command]
pub fn watch_status(live: State<'_, Live>) -> Option<StatusDto> {
    current_status(&live)
}

#[tauri::command]
pub fn watch_pause(live: State<'_, Live>) -> Option<StatusDto> {
    if let Some(c) = lock(&live.current).as_ref() {
        c.watch.pause();
    }
    current_status(&live)
}

#[tauri::command]
pub fn watch_resume(live: State<'_, Live>) -> Option<StatusDto> {
    if let Some(c) = lock(&live.current).as_ref() {
        c.watch.resume();
        c.watch.rescan();
    }
    current_status(&live)
}

/// Stops watching and forgets the folder as the one to resume on launch. Its scans stay in the list.
/// Async so it runs off the main thread: stopping joins the watch thread, which may be finishing a file read on
/// a slow network folder, and the window must never freeze meanwhile.
#[tauri::command]
pub async fn watch_stop(live: State<'_, Live>) -> Result<(), String> {
    live.stop_current();
    Ok(())
}

/// On launch: resume watching the folder that was being watched when the app closed, with its remembered
/// choices. Returns None when there is nothing to resume.
#[tauri::command]
pub fn watch_resume_last(
    app: AppHandle,
    live: State<'_, Live>,
) -> Result<Option<StatusDto>, String> {
    if let Some(s) = current_status(&live) {
        return Ok(Some(s)); // already running (e.g. the UI reloaded)
    }
    let folder = lock(&live.settings).settings.resume_folder.clone();
    match folder {
        Some(f) => start_inner(&app, &live, f, None, None).map(Some),
        None => Ok(None),
    }
}

/// Remember the Standard / High-res switch and/or the watch mode for a folder.
#[tauri::command]
pub fn folder_settings_set(
    live: State<'_, Live>,
    folder: String,
    instrument_class: Option<InstrumentClass>,
    mode: Option<ModeChoice>,
) -> FolderSettings {
    let mut s = lock(&live.settings);
    let f = s.folder_mut(&folder);
    if let Some(c) = instrument_class {
        f.instrument_class = Some(c);
    }
    if let Some(m) = mode {
        f.watch_mode = m;
    }
    let out = f.clone();
    live.save_settings(&s);
    out
}

#[tauri::command]
pub fn settings_get(live: State<'_, Live>) -> SettingsDto {
    let s = lock(&live.settings);
    SettingsDto {
        settings: s.settings.clone(),
        notice: s.notice.clone(),
        path: s.path().to_string_lossy().into_owned(),
    }
}

/// The immutable bytes of an arrival, as raw bytes over the binary IPC path.
#[tauri::command]
pub fn snapshot_bytes(live: State<'_, Live>, id: u64) -> Result<tauri::ipc::Response, String> {
    let bytes = lock(&live.snapshots)
        .get(id)
        .ok_or_else(|| format!("snapshot {id} is no longer held"))?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_store_is_bounded_and_keeps_bytes_immutable() {
        let mut s = SnapshotStore::new();
        let first = s.insert(Arc::from(vec![1u8, 2, 3]));
        assert_eq!(&s.get(first).unwrap()[..], &[1, 2, 3]);
        for i in 0..SNAPSHOT_CAP {
            s.insert(Arc::from(vec![i as u8]));
        }
        assert!(s.get(first).is_none());
        assert_eq!(s.map.len(), SNAPSHOT_CAP);
    }

    #[test]
    fn arrival_dto_is_camel_case() {
        let a = Arrival {
            seq: 3,
            path: PathBuf::from("D:/LabSpec/Spectrum00041.asd"),
            file_name: "Spectrum00041.asd".into(),
            bytes: Arc::from(vec![0u8; 4]),
            sha256: "ab".into(),
            size: 4,
            modified_ms: Some(1),
            revision: 2,
            existing: false,
            same_content_as: None,
            recognised: true,
            kind_hint: Some(KindHint::WhiteReferenceSave),
            detail: "as8".into(),
        };
        let v = serde_json::to_value(ArrivalDto::new(&a, 9, "D:/LabSpec")).unwrap();
        assert!(v["scanId"].is_null());
        assert_eq!(v["snapshotId"], 9);
        assert_eq!(v["kindHint"], "white_reference_save");
        assert_eq!(v["file"], "Spectrum00041.asd");
        assert_eq!(v["revision"], 2);
    }
}
