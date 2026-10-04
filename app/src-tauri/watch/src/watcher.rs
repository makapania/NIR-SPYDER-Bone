//! The folder watcher.
//!
//! ## Sources of "something changed"
//! * **Native events** (`notify`: ReadDirectoryChangesW, FSEvents, inotify), non-recursive. Events only mark a
//!   file as "dirty"; they are never trusted to say a file is complete.
//! * **Rescan**: list the folder and diff `(name, size, mtime)` against what was last seen. Every 5 s in native
//!   mode (the safety net for lost events and for event-overflow `Rescan` flags), every 2 s in POLL mode,
//!   where it is the only source. POLL is chosen automatically on network volumes ([`crate::volume`]), when
//!   native events cannot be set up, or by the user per folder.
//!
//! ## The settle rule (never parse a half-written file)
//! For each dirty file:
//! 1. **Settle**: stat it every `settle / 4`; it is ready only when `(size, mtime)` has stayed unchanged for the
//!    whole settle window (500 ms local, 2 s on network volumes or in POLL mode) AND no event has arrived for
//!    it in that window. A rewrite of a file already delivered waits twice as long.
//! 2. **Snapshot**: open with shared read (never blocks the writer on Windows), read the whole file into memory,
//!    then re-stat both the handle and the path. If anything moved, the file is not settled: start again.
//!    The bytes become an immutable `Arc<[u8]>`; everything downstream reads only this snapshot.
//! 3. **Structure** ([`crate::asd_shape`]): truncated or zero-filled (preallocated) bytes are retried with
//!    backoff. After the give-up time (15 s local, 45 s network) the file is reported "incomplete, will retry
//!    when it changes" and parked: it is retried when its stat changes, when an event arrives, and quietly
//!    every give-up period (a writer can release a lock without changing size or mtime). A file that never
//!    settles at all (it keeps changing, or events keep arriving) is reported once when the same give-up time
//!    has passed, and is still watched until it settles.
//! 4. **Fingerprint**: SHA-256. Content already delivered for this path is not delivered again; new content
//!    under the same name is a new revision ("file changed on disk").
//! 5. **Verify**: shortly after delivery the file is checked once more (re-read if its mtime was fresh), which
//!    catches a same-size rewrite inside one coarse mtime tick (FAT, some SMB servers) that stat cannot see.
//!
//! Sharing violations and other read errors are retried with backoff like an incomplete file.
//! A missing folder (deleted, unmounted share, unplugged disk) puts the watch in `folder_missing`; it is
//! re-checked at every rescan and watching resumes by itself when the folder is back. A folder replaced by a
//! new one with the same name gets fresh native events.

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::asd_shape::{self, KindHint, Shape};
use crate::filter;
use crate::volume::{self, VolumeInfo, VolumeKind};

/// Files larger than this are never ASD scans (they are about 35 KB); they are reported, not read.
pub const MAX_SCAN_BYTES: u64 = 16 * 1024 * 1024;

/// The user's watch-mode choice for a folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeChoice {
    /// Native events on local volumes, POLL on network volumes.
    #[default]
    Auto,
    Native,
    Poll,
}

/// What the watch is actually doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActiveMode {
    Native,
    Poll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchState {
    Watching,
    Paused,
    FolderMissing,
    Stopped,
}

#[derive(Clone, Debug)]
pub struct Timing {
    pub settle_local: Duration,
    pub settle_network: Duration,
    /// Safety-net rescan in native mode.
    pub rescan_interval: Duration,
    /// Rescan interval in POLL mode.
    pub poll_interval: Duration,
    pub give_up_local: Duration,
    pub give_up_network: Duration,
    pub max_backoff: Duration,
    /// Re-check each file once shortly after delivery (see the module docs, step 5).
    pub verify_after_delivery: bool,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            settle_local: Duration::from_millis(500),
            settle_network: Duration::from_secs(2),
            rescan_interval: Duration::from_secs(5),
            poll_interval: Duration::from_secs(2),
            give_up_local: Duration::from_secs(15),
            give_up_network: Duration::from_secs(45),
            max_backoff: Duration::from_secs(4),
            verify_after_delivery: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WatchConfig {
    pub folder: PathBuf,
    pub mode: ModeChoice,
    /// Deliver the scans already in the folder when watching starts (else only new or changed ones).
    pub include_existing: bool,
    pub timing: Timing,
    /// Test hook: drop every native event (simulates total event loss; the rescan must catch everything).
    #[doc(hidden)]
    pub drop_native_events: bool,
    /// Test hook: pretend the folder is on this kind of volume.
    #[doc(hidden)]
    pub force_volume: Option<VolumeKind>,
}

impl WatchConfig {
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        WatchConfig {
            folder: folder.into(),
            mode: ModeChoice::Auto,
            include_existing: true,
            timing: Timing::default(),
            drop_native_events: false,
            force_volume: None,
        }
    }
}

/// A settled, complete file: the bytes are an immutable snapshot taken after the settle rule passed.
#[derive(Clone, Debug)]
pub struct Arrival {
    /// Delivery order within this watch (1, 2, ...).
    pub seq: u64,
    pub path: PathBuf,
    pub file_name: String,
    pub bytes: Arc<[u8]>,
    pub sha256: String,
    pub size: u64,
    /// File modification time, ms since the Unix epoch.
    pub modified_ms: Option<i64>,
    /// 1 for the first content seen under this path; 2, 3, ... when the file was rewritten.
    pub revision: u32,
    /// The file was already in the folder when watching started.
    pub existing: bool,
    /// Another path in this folder delivered the identical bytes earlier (a copy or a rename).
    pub same_content_as: Option<PathBuf>,
    /// The ASD structure check passed; false means the bytes settled but are not a known ASD structure.
    pub recognised: bool,
    pub kind_hint: Option<KindHint>,
    /// Version signature ("as8") or why the bytes were not recognised.
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatus {
    pub folder: String,
    pub state: WatchState,
    pub mode: ActiveMode,
    pub volume: VolumeKind,
    pub volume_detail: String,
    /// Why this mode, in plain words.
    pub mode_reason: String,
    /// Files seen but not yet settled.
    pub pending: usize,
    pub delivered: u64,
    /// Rescan / poll interval in ms.
    pub interval_ms: u64,
    /// Latest notable event (folder gone / back, events failed), if any.
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub enum WatchEvent {
    Arrived(Arrival),
    /// A file did not become complete within the give-up time. It is retried when it changes.
    Incomplete {
        path: PathBuf,
        file_name: String,
        reason: String,
    },
    Status(WatchStatus),
}

enum Msg {
    Fs(notify::Result<notify::Event>),
    /// The sender is acknowledged once the shared status shows the change.
    Pause(Sender<()>),
    Resume(Sender<()>),
    Rescan,
    Stop,
}

/// A running watch of one folder. Dropping it stops the watch and joins its thread.
pub struct FolderWatch {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    status: Arc<Mutex<WatchStatus>>,
    /// Set on stop; the worker checks it between files, so Stop never waits for a whole batch and nothing is
    /// delivered after it.
    stopping: Arc<AtomicBool>,
}

impl FolderWatch {
    /// Starts watching. Never fails because the folder is missing (the watch waits for it); fails only if the
    /// path exists and is not a directory, or the thread cannot be spawned.
    pub fn start<F>(cfg: WatchConfig, sink: F) -> io::Result<FolderWatch>
    where
        F: FnMut(WatchEvent) + Send + 'static,
    {
        if let Ok(m) = fs::metadata(&cfg.folder) {
            if !m.is_dir() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
            }
        }
        let (tx, rx) = mpsc::channel();
        let vol = initial_volume(&cfg);
        let status = Arc::new(Mutex::new(WatchStatus {
            folder: cfg.folder.to_string_lossy().into_owned(),
            state: WatchState::Watching,
            mode: ActiveMode::Native,
            volume: vol.kind,
            volume_detail: vol.detail.clone(),
            mode_reason: String::new(),
            pending: 0,
            delivered: 0,
            interval_ms: cfg.timing.rescan_interval.as_millis() as u64,
            note: None,
        }));
        let mut worker = Worker::new(cfg, vol, tx.clone(), rx, Box::new(sink), status.clone());
        worker.set_up_mode();
        worker.publish_status();
        let stopping = worker.stopping.clone();
        let thread = std::thread::Builder::new()
            .name("spyder-watch".into())
            .spawn(move || worker.run())?;
        Ok(FolderWatch {
            tx,
            thread: Some(thread),
            status,
            stopping,
        })
    }

    /// Pauses; returns once [`FolderWatch::status`] shows it (or after 1 s).
    pub fn pause(&self) {
        let (ack, done) = mpsc::channel();
        if self.tx.send(Msg::Pause(ack)).is_ok() {
            let _ = done.recv_timeout(Duration::from_secs(1));
        }
    }

    /// Resumes (with a rescan); returns once [`FolderWatch::status`] shows it (or after 1 s).
    pub fn resume(&self) {
        let (ack, done) = mpsc::channel();
        if self.tx.send(Msg::Resume(ack)).is_ok() {
            let _ = done.recv_timeout(Duration::from_secs(1));
        }
    }

    /// Rescan now (e.g. after the app regains focus).
    pub fn rescan(&self) {
        let _ = self.tx.send(Msg::Rescan);
    }

    /// The latest status (also sent as [`WatchEvent::Status`] whenever it changes).
    pub fn status(&self) -> WatchStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn initial_volume(cfg: &WatchConfig) -> VolumeInfo {
    match cfg.force_volume {
        Some(VolumeKind::Network) => VolumeInfo {
            kind: VolumeKind::Network,
            detail: "network volume (forced)".into(),
        },
        Some(VolumeKind::Local) => VolumeInfo {
            kind: VolumeKind::Local,
            detail: "local disk (forced)".into(),
        },
        None => volume::detect(&cfg.folder),
    }
}

/// What a stat can tell about a file's content. Besides size and mtime: on Unix the inode and the change time
/// (ctime cannot be set by a copy tool, so a timestamp-preserving rewrite or a replace-by-rename still shows);
/// on Windows the creation time (a replace by rename or delete-and-copy shows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StatKey {
    size: u64,
    mtime: Option<SystemTime>,
    #[cfg(unix)]
    change: (u64, i64, i64),
    #[cfg(not(unix))]
    change: Option<SystemTime>,
}

fn key_of(m: &fs::Metadata) -> StatKey {
    #[cfg(unix)]
    let change = {
        use std::os::unix::fs::MetadataExt;
        (m.ino(), m.ctime(), m.ctime_nsec())
    };
    #[cfg(not(unix))]
    let change = m.created().ok();
    StatKey {
        size: m.len(),
        mtime: m.modified().ok(),
        change,
    }
}

struct Pending {
    first_seen: Instant,
    last_event: Instant,
    last_stat: Option<StatKey>,
    stable_since: Instant,
    attempts: u32,
    next_check: Instant,
    existing: bool,
    /// "Incomplete, will retry" was already reported for this wait (once per wait, not at every retry).
    notified: bool,
}

struct Delivered {
    sha: [u8; 32],
    stat: StatKey,
    revision: u32,
    at: SystemTime,
}

#[derive(Default)]
struct FileState {
    last_seen: Option<StatKey>,
    pending: Option<Pending>,
    delivered: Option<Delivered>,
    /// Gave up at this stat; re-armed when the stat changes, an event arrives, or `retry_at` passes.
    parked: Option<Parked>,
    /// The stat at which "incomplete" was last reported (reported once per stat, not at every retry).
    reported: Option<Option<StatKey>>,
    verify_at: Option<Instant>,
}

#[derive(Clone, Copy)]
struct Parked {
    stat: Option<StatKey>,
    retry_at: Instant,
}

enum ReadErr {
    Changed,
    Io(io::Error),
}

struct Ready {
    path: PathBuf,
    file_name: String,
    bytes: Vec<u8>,
    sha: [u8; 32],
    stat: StatKey,
    revision: u32,
    existing: bool,
    shape: Shape,
}

struct Worker {
    cfg: WatchConfig,
    folder_canon: Option<PathBuf>,
    folder_identity: Option<FolderIdentity>,
    volume: VolumeInfo,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    sink: Box<dyn FnMut(WatchEvent) + Send>,
    shared_status: Arc<Mutex<WatchStatus>>,
    native: Option<RecommendedWatcher>,
    mode: ActiveMode,
    mode_reason: String,
    paused: bool,
    missing: bool,
    note: Option<String>,
    files: HashMap<PathBuf, FileState>,
    by_sha: HashMap<[u8; 32], PathBuf>,
    seq: u64,
    next_rescan: Instant,
    initial_scan_done: bool,
    last_status_sent: Option<(WatchState, ActiveMode, usize, u64, Option<String>)>,
    last_status_at: Instant,
    stopping: Arc<AtomicBool>,
}

impl Worker {
    fn new(
        cfg: WatchConfig,
        volume: VolumeInfo,
        tx: Sender<Msg>,
        rx: Receiver<Msg>,
        sink: Box<dyn FnMut(WatchEvent) + Send>,
        shared_status: Arc<Mutex<WatchStatus>>,
    ) -> Self {
        let now = Instant::now();
        let missing = !cfg.folder.is_dir();
        Worker {
            folder_canon: fs::canonicalize(&cfg.folder).ok(),
            folder_identity: folder_identity(&cfg.folder),
            cfg,
            volume,
            tx,
            rx,
            sink,
            shared_status,
            native: None,
            mode: ActiveMode::Native,
            mode_reason: String::new(),
            paused: false,
            missing,
            note: missing
                .then(|| "folder not available yet; watching starts when it appears".to_string()),
            files: HashMap::new(),
            by_sha: HashMap::new(),
            seq: 0,
            next_rescan: now,
            initial_scan_done: false,
            last_status_sent: None,
            last_status_at: now,
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    fn network_timing(&self) -> bool {
        self.volume.kind == VolumeKind::Network || self.mode == ActiveMode::Poll
    }

    fn settle(&self) -> Duration {
        if self.network_timing() {
            self.cfg.timing.settle_network
        } else {
            self.cfg.timing.settle_local
        }
    }

    fn give_up(&self) -> Duration {
        if self.network_timing() {
            self.cfg.timing.give_up_network
        } else {
            self.cfg.timing.give_up_local
        }
    }

    fn check_interval(&self) -> Duration {
        (self.settle() / 4).max(Duration::from_millis(25))
    }

    fn rescan_interval(&self) -> Duration {
        match self.mode {
            ActiveMode::Native => self.cfg.timing.rescan_interval,
            ActiveMode::Poll => self.cfg.timing.poll_interval,
        }
    }

    fn backoff(&self, attempts: u32) -> Duration {
        let base = self.check_interval();
        (base * 2u32.saturating_pow(attempts.min(8))).min(self.cfg.timing.max_backoff.max(base))
    }

    fn state(&self) -> WatchState {
        if self.missing {
            WatchState::FolderMissing
        } else if self.paused {
            WatchState::Paused
        } else {
            WatchState::Watching
        }
    }

    /// Chooses native or POLL and (re)creates the native watcher.
    fn set_up_mode(&mut self) {
        self.native = None;
        let poll_s = self.cfg.timing.poll_interval.as_secs_f64();
        let (mut mode, mut reason) = match self.cfg.mode {
            ModeChoice::Poll => (ActiveMode::Poll, format!("poll mode chosen for this folder; checked every {poll_s:.0} s")),
            ModeChoice::Native => (ActiveMode::Native, "native change events (chosen for this folder)".to_string()),
            ModeChoice::Auto if self.volume.kind == VolumeKind::Network => (
                ActiveMode::Poll,
                format!(
                    "{}: change events are unreliable on network folders, so it is checked every {poll_s:.0} s",
                    self.volume.detail
                ),
            ),
            ModeChoice::Auto => (
                ActiveMode::Native,
                format!(
                    "{}: change events plus a rescan every {:.0} s",
                    self.volume.detail,
                    self.cfg.timing.rescan_interval.as_secs_f64()
                ),
            ),
        };
        if mode == ActiveMode::Native && !self.missing {
            match self.make_native() {
                Ok(w) => self.native = Some(w),
                Err(e) => {
                    mode = ActiveMode::Poll;
                    reason = format!(
                        "change events unavailable here ({e}); checked every {poll_s:.0} s"
                    );
                }
            }
        }
        self.mode = mode;
        self.mode_reason = reason;
    }

    fn make_native(&self) -> notify::Result<RecommendedWatcher> {
        let tx = self.tx.clone();
        let mut w = notify::recommended_watcher(move |res| {
            let _ = tx.send(Msg::Fs(res));
        })?;
        w.watch(&self.cfg.folder, RecursiveMode::NonRecursive)?;
        Ok(w)
    }

    fn run(mut self) {
        loop {
            let now = Instant::now();
            let deadline = self.next_deadline(now);
            let timeout = deadline
                .saturating_duration_since(now)
                .clamp(Duration::from_millis(1), Duration::from_millis(200));
            match self.rx.recv_timeout(timeout) {
                Ok(msg) => {
                    if !self.handle(msg) {
                        break;
                    }
                    // Drain a burst of events before doing any file work.
                    let mut n = 0;
                    let mut stop = false;
                    while n < 10_000 {
                        match self.rx.try_recv() {
                            Ok(m) => {
                                if !self.handle(m) {
                                    stop = true;
                                    break;
                                }
                                n += 1;
                            }
                            Err(_) => break,
                        }
                    }
                    if stop {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = Instant::now();
            if now >= self.next_rescan {
                self.rescan(now);
            }
            if !self.paused && !self.missing {
                self.retry_parked(Instant::now());
                self.process_due(Instant::now());
                self.process_verify(Instant::now());
            }
            self.publish_status_throttled();
        }
        self.native = None;
        self.update_shared(WatchState::Stopped);
        let st = self.status_value(WatchState::Stopped);
        (self.sink)(WatchEvent::Status(st));
    }

    fn next_deadline(&self, now: Instant) -> Instant {
        let mut d = self.next_rescan;
        if !self.paused && !self.missing {
            for f in self.files.values() {
                if let Some(p) = &f.pending {
                    d = d.min(p.next_check);
                }
                if let Some(v) = f.verify_at {
                    d = d.min(v);
                }
                if let Some(p) = f.parked {
                    d = d.min(p.retry_at);
                }
            }
        }
        d.max(now)
    }

    /// Returns false to stop.
    fn handle(&mut self, msg: Msg) -> bool {
        let now = Instant::now();
        match msg {
            Msg::Stop => return false,
            Msg::Pause(ack) => {
                self.paused = true;
                self.publish_status();
                let _ = ack.send(());
            }
            Msg::Resume(ack) => {
                self.paused = false;
                self.next_rescan = now;
                self.publish_status();
                let _ = ack.send(());
            }
            Msg::Rescan => self.next_rescan = now,
            Msg::Fs(Err(e)) => {
                // An error from the event source (overflow, watched folder gone): trust nothing, rescan.
                self.note = Some(format!(
                    "change events reported a problem ({e}); rescanning"
                ));
                self.next_rescan = now;
            }
            Msg::Fs(Ok(ev)) => {
                if self.cfg.drop_native_events {
                    return true;
                }
                if ev.need_rescan() {
                    self.next_rescan = now;
                }
                // Reads (open/close-without-write) are not changes. On Linux inotify reports them, including the
                // watcher's own snapshot reads, which would otherwise mark every delivered file pending again.
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    return true;
                }
                for p in &ev.paths {
                    if self.is_folder_itself(p) {
                        self.next_rescan = now;
                        continue;
                    }
                    let Some(name) = p.file_name() else { continue };
                    if !filter::is_candidate_name(name) {
                        continue;
                    }
                    // Non-recursive watch: only direct children. FSEvents reports canonical paths, so the key is
                    // rebuilt from the folder as the user gave it.
                    if !self.is_direct_child(p) {
                        continue;
                    }
                    let key = self.cfg.folder.join(name);
                    self.touch(key, now, false);
                }
            }
        }
        true
    }

    fn is_folder_itself(&self, p: &Path) -> bool {
        p == self.cfg.folder || self.folder_canon.as_deref() == Some(p)
    }

    fn is_direct_child(&self, p: &Path) -> bool {
        match p.parent() {
            Some(parent) => {
                parent == self.cfg.folder
                    || self.folder_canon.as_deref() == Some(parent)
                    || parent.file_name() == self.cfg.folder.file_name()
            }
            None => false,
        }
    }

    fn touch(&mut self, key: PathBuf, now: Instant, existing: bool) {
        let ci = self.check_interval();
        let f = self.files.entry(key).or_default();
        f.parked = None;
        match &mut f.pending {
            Some(p) => {
                p.last_event = now;
                p.next_check = p.next_check.min(now + ci);
            }
            None => {
                f.pending = Some(Pending {
                    first_seen: now,
                    last_event: now,
                    last_stat: None,
                    stable_since: now,
                    attempts: 0,
                    next_check: now,
                    existing,
                    notified: false,
                })
            }
        }
    }

    fn rescan(&mut self, now: Instant) {
        self.next_rescan = now + self.rescan_interval();
        let entries = match fs::read_dir(&self.cfg.folder) {
            Ok(rd) => rd,
            Err(e) => {
                if !self.missing {
                    self.missing = true;
                    self.native = None;
                    self.note = Some(format!(
                        "folder not available ({e}); watching resumes when it is back"
                    ));
                    self.publish_status();
                }
                // Check for its return at least every 2 s.
                self.next_rescan = now + self.rescan_interval().min(Duration::from_secs(2));
                return;
            }
        };
        let identity = folder_identity(&self.cfg.folder);
        if self.missing {
            self.missing = false;
            self.folder_canon = fs::canonicalize(&self.cfg.folder).ok();
            self.folder_identity = identity;
            if self.cfg.force_volume.is_none() {
                self.volume = volume::detect(&self.cfg.folder);
            }
            self.set_up_mode();
            self.note = Some("folder is available again; watching resumed".into());
            self.next_rescan = now + self.rescan_interval();
            self.publish_status();
        } else if identity.is_some() && identity != self.folder_identity {
            // Deleted and recreated between two rescans (or another volume mounted there): the old native watch is
            // attached to the old folder, and the volume kind may have changed.
            self.folder_identity = identity;
            self.folder_canon = fs::canonicalize(&self.cfg.folder).ok();
            if self.cfg.force_volume.is_none() {
                self.volume = volume::detect(&self.cfg.folder);
            }
            self.set_up_mode();
            self.note = Some("folder was replaced; watching the new one".into());
            self.publish_status();
        }

        let initial = !self.initial_scan_done;
        self.initial_scan_done = true;
        let mut present: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !filter::is_candidate_name(&name) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() || filter::is_hidden_meta(&meta) {
                continue;
            }
            let path = self.cfg.folder.join(&name);
            let st = key_of(&meta);
            present.push(path.clone());
            let f = self.files.entry(path.clone()).or_default();
            if f.last_seen == Some(st) {
                continue;
            }
            f.last_seen = Some(st);
            if initial && !self.cfg.include_existing {
                continue; // baseline: only later changes count
            }
            if f.parked.is_some_and(|p| p.stat == Some(st)) {
                continue;
            }
            if f.pending.is_none() && f.delivered.as_ref().is_some_and(|d| d.stat == st) {
                continue;
            }
            self.touch(path, now, initial);
        }
        // Forget what is gone (deliveries are kept so identical content returning is not re-delivered).
        if present.len() != self.files.len() {
            let set: std::collections::HashSet<&PathBuf> = present.iter().collect();
            for (p, f) in self.files.iter_mut() {
                if !set.contains(p) {
                    f.last_seen = None;
                    f.parked = None;
                }
            }
        }
    }

    fn process_due(&mut self, now: Instant) {
        let mut due: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(_, f)| f.pending.as_ref().is_some_and(|p| p.next_check <= now))
            .map(|(p, _)| p.clone())
            .collect();
        if due.is_empty() {
            return;
        }
        due.sort();
        let mut ready = Vec::new();
        for path in due {
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            if let Some(r) = self.check_one(&path, now) {
                ready.push(r);
            }
        }
        // Deliver a batch in acquisition order: mtime, then natural file-name order.
        ready.sort_by(|a, b| {
            a.stat
                .mtime
                .cmp(&b.stat.mtime)
                .then_with(|| filter::natural_cmp(&a.file_name, &b.file_name))
        });
        for r in ready {
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            self.deliver(r);
        }
    }

    fn check_one(&mut self, path: &Path, _batch_now: Instant) -> Option<Ready> {
        // Each file gets its own clock reading: an earlier file's slow read must not backdate this observation.
        let now = Instant::now();
        let ci = self.check_interval();
        let settle = self.settle();
        let give_up = self.give_up();
        let meta = fs::metadata(path);
        let f = self.files.get_mut(path)?;
        let p = f.pending.as_mut()?;
        let meta = match meta {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Gone (deleted, or a temp name renamed away). Nothing to do.
                f.pending = None;
                f.last_seen = None;
                return None;
            }
            Err(e) => {
                p.attempts += 1;
                let reason = format!("file information could not be read ({e})");
                return self.retry_or_park(path, now, reason, None);
            }
        };
        if !meta.is_file() || filter::is_hidden_meta(&meta) {
            f.pending = None;
            return None;
        }
        let st = key_of(&meta);
        let need = if f.delivered.is_some() {
            settle * 2
        } else {
            settle
        };
        let unsettled = if p.last_stat != Some(st) {
            p.last_stat = Some(st);
            p.stable_since = now;
            p.next_check = now + ci;
            true
        } else {
            let quiet_from = p.stable_since.max(p.last_event);
            if now < quiet_from + need {
                p.next_check = (quiet_from + need).min(now + ci);
                true
            } else {
                false
            }
        };
        if unsettled {
            // The overall deadline also runs while waiting for the file to settle: a file that keeps changing (or
            // keeps raising events) is reported once, and stays pending so it is delivered when it settles.
            if !p.notified && now.duration_since(p.first_seen) >= give_up {
                p.notified = true;
                (self.sink)(WatchEvent::Incomplete {
                    path: path.to_path_buf(),
                    file_name: file_name_of(path),
                    reason: format!(
                        "still being written (it kept changing for {} s); will retry when it settles",
                        give_up.as_secs()
                    ),
                });
            }
            return None;
        }
        if st.size == 0 {
            p.attempts += 1;
            return self.retry_or_park(path, now, "file is empty".into(), Some(st));
        }
        if st.size > MAX_SCAN_BYTES {
            let file_name = file_name_of(path);
            f.pending = None;
            f.parked = Some(Parked {
                stat: Some(st),
                retry_at: now + Duration::from_secs(3600),
            });
            (self.sink)(WatchEvent::Incomplete {
                path: path.to_path_buf(),
                file_name,
                reason: format!(
                    "{} MB is far larger than any ASD scan; not read",
                    st.size / (1024 * 1024)
                ),
            });
            return None;
        }
        match read_snapshot(path, st) {
            Err(ReadErr::Changed) => {
                p.last_stat = None;
                p.next_check = now + ci;
                None
            }
            Err(ReadErr::Io(e)) => {
                p.attempts += 1;
                let reason = if is_sharing_violation(&e) {
                    "the file is still open in another program".to_string()
                } else {
                    format!("the file could not be read ({e})")
                };
                self.retry_or_park(path, now, reason, Some(st))
            }
            Ok(bytes) => {
                let shape = asd_shape::check(&bytes);
                if let Shape::Incomplete(why) = shape {
                    p.attempts += 1;
                    return self.retry_or_park(
                        path,
                        now,
                        format!("incomplete file: {why}"),
                        Some(st),
                    );
                }
                let sha: [u8; 32] = Sha256::digest(&bytes).into();
                let existing = p.existing;
                f.pending = None;
                f.last_seen = Some(st);
                if let Some(d) = &mut f.delivered {
                    if d.sha == sha {
                        d.stat = st; // touched or metadata-only change: same content, nothing new
                        return None;
                    }
                }
                let revision = f.delivered.as_ref().map_or(1, |d| d.revision + 1);
                Some(Ready {
                    path: path.to_path_buf(),
                    file_name: file_name_of(path),
                    bytes,
                    sha,
                    stat: st,
                    revision,
                    existing,
                    shape,
                })
            }
        }
    }

    /// Schedules a retry with backoff, or parks the file and reports it once the give-up time has passed.
    fn retry_or_park(
        &mut self,
        path: &Path,
        now: Instant,
        reason: String,
        st: Option<StatKey>,
    ) -> Option<Ready> {
        let give_up = self.give_up();
        let attempts = self.files.get(path)?.pending.as_ref()?.attempts;
        let delay = self.backoff(attempts);
        let f = self.files.get_mut(path)?;
        let p = f.pending.as_mut()?;
        if now.duration_since(p.first_seen) >= give_up {
            let stat = st.or(f.last_seen);
            let notified = p.notified;
            f.pending = None;
            f.parked = Some(Parked {
                stat,
                retry_at: now + give_up,
            });
            if notified {
                // Already reported during this wait (it kept changing): once is enough.
                f.reported = Some(stat);
            } else if f.reported != Some(stat) {
                f.reported = Some(stat);
                (self.sink)(WatchEvent::Incomplete {
                    path: path.to_path_buf(),
                    file_name: file_name_of(path),
                    reason: format!("{reason}; will retry when the file changes"),
                });
            }
            return None;
        }
        p.next_check = now + delay;
        None
    }

    fn deliver(&mut self, r: Ready) {
        self.seq += 1;
        let same_content_as = match self.by_sha.get(&r.sha) {
            Some(p) if *p != r.path => Some(p.clone()),
            Some(_) => None,
            None => {
                self.by_sha.insert(r.sha, r.path.clone());
                None
            }
        };
        let verify_at = self
            .cfg
            .timing
            .verify_after_delivery
            .then(|| Instant::now() + (self.settle() * 2).max(Duration::from_millis(500)));
        if let Some(f) = self.files.get_mut(&r.path) {
            f.reported = None;
            f.delivered = Some(Delivered {
                sha: r.sha,
                stat: r.stat,
                revision: r.revision,
                at: SystemTime::now(),
            });
            f.verify_at = verify_at;
        }
        let (recognised, kind_hint, detail) = match r.shape {
            Shape::Complete { version, kind } => (true, Some(kind), version),
            Shape::Unrecognised(why) => (false, None, why),
            Shape::Incomplete(why) => (false, None, why.to_string()),
        };
        let size = r.bytes.len() as u64;
        let arrival = Arrival {
            seq: self.seq,
            path: r.path,
            file_name: r.file_name,
            bytes: Arc::from(r.bytes),
            sha256: crate::hex(&r.sha),
            size,
            modified_ms: r.stat.mtime.map(crate::unix_ms),
            revision: r.revision,
            existing: r.existing,
            same_content_as,
            recognised,
            kind_hint,
            detail,
        };
        (self.sink)(WatchEvent::Arrived(arrival));
    }

    /// Quiet periodic retry of parked files (a lock released without a size or mtime change, a share that
    /// came back). Not reported again unless the stat changed.
    fn retry_parked(&mut self, now: Instant) {
        let due: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(_, f)| f.pending.is_none() && f.parked.is_some_and(|p| p.retry_at <= now))
            .map(|(p, _)| p.clone())
            .collect();
        for path in due {
            self.touch(path, now, false);
        }
    }

    /// The one-time check after delivery (module docs, step 5).
    fn process_verify(&mut self, now: Instant) {
        let due: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(_, f)| f.verify_at.is_some_and(|t| t <= now) && f.pending.is_none())
            .map(|(p, _)| p.clone())
            .collect();
        for path in due {
            let Some(f) = self.files.get_mut(&path) else {
                continue;
            };
            f.verify_at = None;
            let Some(d) = &f.delivered else { continue };
            let (d_sha, d_stat, d_at) = (d.sha, d.stat, d.at);
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let st = key_of(&meta);
            if st != d_stat {
                self.touch(path, now, false);
                continue;
            }
            // Same stat. Re-read only when the mtime was fresh at delivery (a coarse-tick rewrite is possible).
            let fresh = d_stat.mtime.is_none_or(|m| {
                d_at.duration_since(m)
                    .map_or(true, |age| age < Duration::from_secs(3))
            });
            if !fresh {
                continue;
            }
            if let Ok(bytes) = read_snapshot(&path, st) {
                let sha: [u8; 32] = Sha256::digest(&bytes).into();
                if sha != d_sha {
                    self.touch(path, now, false);
                }
            }
        }
    }

    fn pending_count(&self) -> usize {
        self.files.values().filter(|f| f.pending.is_some()).count()
    }

    fn status_value(&self, state: WatchState) -> WatchStatus {
        WatchStatus {
            folder: self.cfg.folder.to_string_lossy().into_owned(),
            state,
            mode: self.mode,
            volume: self.volume.kind,
            volume_detail: self.volume.detail.clone(),
            mode_reason: self.mode_reason.clone(),
            pending: self.pending_count(),
            delivered: self.seq,
            interval_ms: self.rescan_interval().as_millis() as u64,
            note: self.note.clone(),
        }
    }

    fn update_shared(&self, state: WatchState) {
        let v = self.status_value(state);
        match self.shared_status.lock() {
            Ok(mut s) => *s = v,
            Err(p) => *p.into_inner() = v,
        }
    }

    fn publish_status(&mut self) {
        let st = self.status_value(self.state());
        self.update_shared(st.state);
        self.last_status_sent =
            Some((st.state, st.mode, st.pending, st.delivered, st.note.clone()));
        self.last_status_at = Instant::now();
        (self.sink)(WatchEvent::Status(st));
    }

    /// Status for pending/delivered counts: at most every 300 ms, and only on change.
    fn publish_status_throttled(&mut self) {
        let key = (
            self.state(),
            self.mode,
            self.pending_count(),
            self.seq,
            self.note.clone(),
        );
        if self.last_status_sent.as_ref() == Some(&key) {
            return;
        }
        let structural = self
            .last_status_sent
            .as_ref()
            .is_none_or(|k| k.0 != key.0 || k.1 != key.1 || k.4 != key.4);
        let quiet_enough = self.last_status_at.elapsed() >= Duration::from_millis(300);
        let settled = key.2 == 0;
        if structural || quiet_enough || settled {
            self.publish_status();
        } else {
            self.update_shared(key.0);
        }
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reads the whole file and confirms that neither the handle nor the path changed while reading.
fn read_snapshot(path: &Path, before: StatKey) -> Result<Vec<u8>, ReadErr> {
    // std opens with FILE_SHARE_READ | WRITE | DELETE on Windows: the writer is never blocked by us.
    let mut f = fs::File::open(path).map_err(ReadErr::Io)?;
    let mut buf = Vec::with_capacity(before.size as usize);
    (&mut f)
        .take(MAX_SCAN_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(ReadErr::Io)?;
    let by_handle = f.metadata().map_err(ReadErr::Io)?;
    drop(f);
    let by_path = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Err(ReadErr::Changed),
    };
    if buf.len() as u64 != before.size || key_of(&by_handle) != before || key_of(&by_path) != before
    {
        return Err(ReadErr::Changed);
    }
    Ok(buf)
}

fn is_sharing_violation(e: &io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32) and ERROR_LOCK_VIOLATION (33) on Windows.
    cfg!(windows) && matches!(e.raw_os_error(), Some(32) | Some(33))
}

/// Identity of the folder itself, to notice a folder deleted and recreated between two rescans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FolderIdentity {
    #[cfg(unix)]
    Inode(u64, u64),
    #[cfg(not(unix))]
    Created(SystemTime),
}

fn folder_identity(p: &Path) -> Option<FolderIdentity> {
    let m = fs::metadata(p).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(FolderIdentity::Inode(m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        m.created().ok().map(FolderIdentity::Created)
    }
}
