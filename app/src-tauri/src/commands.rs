//! IPC commands for scoring real scans: the session (open files / folder, the Standard / High-res switch, the
//! results for one analysis type), the display arrays (binary float32) and the CSV export. Thin wrappers
//! around `scoring`; heavy work runs off the main thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use spyder_watch::settings::InstrumentClass;
use tauri::State;

use crate::binary::f32_le_bytes;
use crate::display::{DisplayInfo, RefMeta};
use crate::engine::Core;
use crate::live::Live;
use crate::mapping::ScanResult;
use crate::scoring::{self, SessionInfo};
use crate::session::{core_class, Session};

/// The analysis core and the session on screen, shared by the commands and the folder watcher.
pub struct Shared {
    pub core: Arc<Core>,
    pub session: Mutex<Session>,
    gen: AtomicU64,
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn remembered_class(c: Option<InstrumentClass>) -> Option<&'static str> {
    c.map(|c| match c {
        InstrumentClass::Standard => spyder_core::plugins::CLASS_STD,
        InstrumentClass::Hires => spyder_core::plugins::CLASS_HIRES,
    })
}

impl Shared {
    pub fn new(core: Core) -> Shared {
        Shared {
            core: Arc::new(core),
            session: Mutex::new(Session::new(0, None, false, None)),
            gen: AtomicU64::new(0),
        }
    }

    /// Replace the session with a new, empty one at once (a watch: its watcher scores into it); returns its
    /// generation. The generation is taken under the session lock, so sessions are installed in generation order.
    pub fn new_session(
        &self,
        folder: Option<String>,
        live: bool,
        remembered: Option<&'static str>,
        log: Option<spyder_watch::session::SessionLog>,
    ) -> u64 {
        let mut cur = lock(&self.session);
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        *cur = Session::new(g, folder, live, remembered).with_log(log);
        g
    }

    /// Reserve a generation for an operation that builds its own session off to the side (Open files or a
    /// folder). Any later Open or Watch supersedes it.
    pub fn reserve(&self) -> u64 {
        self.gen.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The generation of the newest Open or Watch.
    pub fn current_gen(&self) -> u64 {
        self.gen.load(Ordering::SeqCst)
    }

    /// Install an operation's finished session, only if no later Open or Watch started meanwhile. Returns
    /// false (and drops the session) when it was superseded.
    pub fn publish(&self, s: Session) -> bool {
        let mut cur = lock(&self.session);
        if self.gen.load(Ordering::SeqCst) != s.gen {
            return false;
        }
        *cur = s;
        true
    }

    /// Open files into a new session owned by this operation (generation `gen`, from [`Shared::reserve`]):
    /// every file is read and analysed under THIS folder's class, with no lock held, and the session is
    /// published only if it is still the newest operation. Returns the number of files, or None when a later
    /// Open or Watch superseded it (nothing of it reaches the screen).
    pub fn open_files(
        &self,
        gen: u64,
        folder: Option<String>,
        remembered: Option<&'static str>,
        files: &[PathBuf],
    ) -> Option<usize> {
        let mut s = Session::new(gen, folder, false, remembered);
        let mut n = 0;
        for p in files {
            if self.current_gen() != gen {
                return None; // superseded: stop reading
            }
            s.add(&self.core, scoring::read_scan(p));
            n += 1;
        }
        self.publish(s).then_some(n)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDto {
    pub session: SessionInfo,
    pub count: usize,
    pub message: String,
    /// A later Open or Watch replaced this one before it finished: the list shows that one instead.
    pub superseded: bool,
}

fn is_watching(live: &Live) -> bool {
    live.is_watching()
}

#[tauri::command]
pub fn session_get(shared: State<'_, Arc<Shared>>, live: State<'_, Live>) -> SessionInfo {
    let s = lock(&shared.session);
    scoring::session_info(&shared.core, &s, s.live && is_watching(&live))
}

/// The user's Standard / High-res switch: re-analyses the session and remembers it for the folder.
#[tauri::command]
pub async fn session_set_class(
    shared: State<'_, Arc<Shared>>,
    live: State<'_, Live>,
    class: String,
) -> Result<SessionInfo, String> {
    let c = core_class(&class).ok_or_else(|| format!("unknown instrument class {class:?}"))?;
    let sh = Arc::clone(&shared);
    let folder = tauri::async_runtime::spawn_blocking(move || {
        let mut s = lock(&sh.session);
        s.set_class(&sh.core, c);
        s.folder.clone()
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Some(f) = folder {
        live.remember_class(&f, c);
    }
    let s = lock(&shared.session);
    Ok(scoring::session_info(
        &shared.core,
        &s,
        s.live && is_watching(&live),
    ))
}

#[tauri::command]
pub fn session_scans(shared: State<'_, Arc<Shared>>, analysis: String) -> Vec<ScanResult> {
    let s = lock(&shared.session);
    scoring::results(&shared.core, &s, &analysis)
}

/// Open files (`folder: false`) or one folder (`folder: true`, its .asd files, not sub-folders) as a new
/// session. A running watch stops (its scans leave the list with it).
#[tauri::command]
pub async fn session_open(
    shared: State<'_, Arc<Shared>>,
    live: State<'_, Live>,
    paths: Vec<String>,
    folder: bool,
) -> Result<OpenDto, String> {
    // Resolve what to open BEFORE stopping a running watch: a failed open (e.g. a dropped file that is not a
    // folder) must leave the watch running, since nothing else would tell the UI it had stopped.
    let (files, folder_path) = if folder {
        let f = paths.first().cloned().ok_or("no folder given")?;
        let p = PathBuf::from(&f);
        if !p.is_dir() {
            return Err(format!("{f} is not a folder or an .asd file"));
        }
        (scoring::folder_files(&p)?, f)
    } else {
        let files: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let parent = files
            .first()
            .and_then(|p| p.parent())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        (files, parent)
    };
    let stopped = live.stop_current();
    let remembered = live.remembered_class(&folder_path);
    let gen = shared.reserve();
    let sh = Arc::clone(&shared);
    let fp = folder_path.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || {
        sh.open_files(gen, Some(fp), remembered, &files)
    })
    .await
    .map_err(|e| e.to_string())?;
    let s = lock(&shared.session);
    let info = scoring::session_info(&shared.core, &s, s.live && is_watching(&live));
    let Some(n) = opened else {
        return Ok(OpenDto {
            session: info,
            count: 0,
            message: String::new(),
            superseded: true,
        });
    };
    let what = if folder {
        format!("{n} scan{} in {folder_path}", if n == 1 { "" } else { "s" })
    } else {
        format!("{n} file{}", if n == 1 { "" } else { "s" })
    };
    let mut message = format!("Opened {what}.");
    if n == 0 && folder {
        message = format!("No .asd scans in {folder_path} (sub-folders are not read).");
    }
    if let Some(f) = stopped {
        message.push_str(&format!(" Stopped watching {f}."));
    }
    if let Some(e) = &shared.core.error {
        message = format!("No verdict model available: {e}");
    }
    Ok(OpenDto {
        session: info,
        count: n,
        message,
        superseded: false,
    })
}

/// The display arrays of one scan: `display::VIEW_ORDER` views of n float32 values, little-endian.
#[tauri::command]
pub fn scan_views(
    shared: State<'_, Arc<Shared>>,
    id: String,
    smoothing: u32,
) -> Result<tauri::ipc::Response, String> {
    let s = lock(&shared.session);
    let v = scoring::scan_views(&shared.core, &s, &id, smoothing)?;
    Ok(tauri::ipc::Response::new(f32_le_bytes(&v)))
}

#[tauri::command]
pub fn display_info(shared: State<'_, Arc<Shared>>) -> DisplayInfo {
    shared.core.display.info()
}

#[tauri::command]
pub fn reference_meta(shared: State<'_, Arc<Shared>>) -> Vec<RefMeta> {
    scoring::reference_views(&shared.core, 31).0
}

#[tauri::command]
pub fn reference_views(shared: State<'_, Arc<Shared>>, smoothing: u32) -> tauri::ipc::Response {
    tauri::ipc::Response::new(f32_le_bytes(
        &scoring::reference_views(&shared.core, smoothing).1,
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDto {
    pub path: String,
    pub rows: usize,
    /// The technical CSV written beside the readable one, if any.
    pub technical_path: Option<String>,
}

/// Export. With `readable` (the UI's plain-language CSV, worded as on screen) it goes to `path` and the session's
/// technical CSV (the core's oracle-identical rows) goes beside it as "<name> (technical).csv"; without it, the
/// technical CSV goes to `path`.
#[tauri::command]
pub async fn export_csv(
    shared: State<'_, Arc<Shared>>,
    path: String,
    analysis: String,
    readable: Option<String>,
) -> Result<ExportDto, String> {
    let sh = Arc::clone(&shared);
    let p = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (text, rows) = scoring::export_csv(&lock(&sh.session), &analysis);
        let dest = std::path::Path::new(&p);
        let Some(readable) = readable else {
            write_replacing(dest, text.as_bytes()).map_err(|e| format!("cannot write {p}: {e}"))?;
            return Ok(ExportDto {
                path: p,
                rows,
                technical_path: None,
            });
        };
        let tech = technical_path(dest);
        let t = tech.to_string_lossy().into_owned();
        write_replacing(&tech, text.as_bytes()).map_err(|e| format!("cannot write {t}: {e}"))?;
        write_replacing(dest, readable.as_bytes()).map_err(|e| format!("cannot write {p}: {e}"))?;
        Ok(ExportDto {
            path: p,
            rows,
            technical_path: Some(t),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "C:/x/name.csv" -> "C:/x/name (technical).csv": the technical CSV saved beside the readable one.
pub fn technical_path(readable: &std::path::Path) -> std::path::PathBuf {
    let stem = readable
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export".into());
    readable.with_file_name(format!("{stem} (technical).csv"))
}

/// Writes `bytes` to `dest` without ever leaving it truncated: the whole file goes to a temporary sibling first and
/// then replaces `dest` in one step, so a full disk or a dropped network share keeps the previous file intact.
pub fn write_replacing(dest: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dest.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, dest)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

#[cfg(test)]
mod write_tests {
    #[test]
    fn the_technical_file_sits_beside_the_readable_one() {
        let p = super::technical_path(std::path::Path::new("C:/data/site_spyder_bone.csv"));
        assert_eq!(
            p,
            std::path::Path::new("C:/data/site_spyder_bone (technical).csv")
        );
    }

    #[test]
    fn replaces_an_existing_file_whole_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("spyder_wr_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("out.csv");
        std::fs::write(&p, "old contents that are longer than the new ones").unwrap();
        super::write_replacing(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(left.len(), 1, "{left:?}");
    }
}
