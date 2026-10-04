//! The settings file (03_architecture section 6.3): a typed, versioned JSON struct, migrated forward on load and
//! written atomically (temp file, fsync, rename).
//!
//! Version 1 holds the watched folders: per folder the Standard / High-res switch (DECISIONS 50: the user's,
//! remembered per folder), the "include files already there" choice, the watch mode (auto / poll / native) and
//! when it was last used; plus the folder that was being watched when the app closed, so watching resumes there.
//!
//! Failure handling never loses the user's file: unreadable JSON is kept as `settings.json.corrupt-<ms>` and
//! defaults are used; a file written by a NEWER app version is left untouched and this app does not save over it.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::watcher::ModeChoice;

pub const SETTINGS_VERSION: u32 = 1;
/// Folders remembered (most recent first); older ones are dropped.
pub const MAX_FOLDERS: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstrumentClass {
    Standard,
    Hires,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSettings {
    pub path: String,
    /// The Standard / High-res switch for this folder; `None` until the user sets it.
    #[serde(default)]
    pub instrument_class: Option<InstrumentClass>,
    /// Whether scans already in the folder are included when watching starts; `None` until asked.
    #[serde(default)]
    pub include_existing: Option<bool>,
    #[serde(default)]
    pub watch_mode: ModeChoice,
    #[serde(default)]
    pub last_used_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub settings_version: u32,
    #[serde(default)]
    pub folders: Vec<FolderSettings>,
    /// The folder being watched when the app last closed (watching resumes there on launch).
    #[serde(default)]
    pub resume_folder: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            settings_version: SETTINGS_VERSION,
            folders: Vec::new(),
            resume_folder: None,
        }
    }
}

#[derive(Debug)]
pub enum MigrateError {
    Newer(u64),
    Invalid(String),
}

/// Bring any known older layout up to [`SETTINGS_VERSION`].
pub fn migrate(mut v: serde_json::Value) -> Result<Settings, MigrateError> {
    let ver = v
        .get("settingsVersion")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    if ver > SETTINGS_VERSION as u64 {
        return Err(MigrateError::Newer(ver));
    }
    if ver == 0 {
        // Pre-versioned files (none were ever shipped): same field names; stamp the version.
        if let Some(o) = v.as_object_mut() {
            o.insert("settingsVersion".into(), SETTINGS_VERSION.into());
        } else {
            return Err(MigrateError::Invalid("not a JSON object".into()));
        }
    }
    // Future: `if ver < 2 { ...rewrite v to version 2... }` chains go here, oldest first.
    serde_json::from_value(v).map_err(|e| MigrateError::Invalid(e.to_string()))
}

/// Folder identity for settings lookups: separators unified and trailing separators dropped; case-folded on
/// Windows and macOS, whose default file systems ignore case.
pub fn folder_key(p: &str) -> String {
    let mut s = if cfg!(windows) {
        p.replace('\\', "/")
    } else {
        p.to_string()
    };
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") {
        s.pop();
    }
    if cfg!(any(windows, target_os = "macos")) {
        s = s.to_lowercase();
    }
    s
}

pub struct SettingsStore {
    path: PathBuf,
    pub settings: Settings,
    writable: bool,
    /// A one-line explanation when the file could not be used as-is (shown in the status line / log).
    pub notice: Option<String>,
}

impl SettingsStore {
    pub fn load(path: impl Into<PathBuf>) -> SettingsStore {
        let path = path.into();
        let mut store = SettingsStore {
            path: path.clone(),
            settings: Settings::default(),
            writable: true,
            notice: None,
        };
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return store,
            Err(e) => {
                store.writable = false;
                store.notice = Some(format!(
                    "Settings could not be read ({e}); using defaults for now."
                ));
                return store;
            }
        };
        let parsed = serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|e| MigrateError::Invalid(e.to_string()))
            .and_then(migrate);
        match parsed {
            Ok(s) => store.settings = s,
            Err(MigrateError::Newer(v)) => {
                store.writable = false;
                store.notice = Some(format!(
                    "Settings were written by a newer SPYDER Bone (settings version {v}); they are left unchanged."
                ));
            }
            Err(MigrateError::Invalid(e)) => {
                let backup = path.with_extension(format!(
                    "json.corrupt-{}",
                    crate::unix_ms(std::time::SystemTime::now())
                ));
                if let Err(e) = fs::rename(&path, &backup) {
                    // Could not keep the old file aside: never overwrite it.
                    store.writable = false;
                    store.notice = Some(format!(
                        "Settings were unreadable and could not be kept aside ({e}); using defaults without saving."
                    ));
                    return store;
                }
                store.notice = Some(format!(
                    "Settings were unreadable ({e}); kept as {} and started fresh.",
                    backup
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
            }
        }
        store
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    /// Atomic save. A no-op (Ok) when the file belongs to a newer version or could not be read.
    pub fn save(&self) -> io::Result<()> {
        if !self.writable {
            return Ok(());
        }
        let json = serde_json::to_vec_pretty(&self.settings).map_err(io::Error::other)?;
        write_atomic(&self.path, &json)
    }

    pub fn folder(&self, path: &str) -> Option<&FolderSettings> {
        let k = folder_key(path);
        self.settings
            .folders
            .iter()
            .find(|f| folder_key(&f.path) == k)
    }

    /// The folder's entry (created if new), moved to the front and stamped as used now.
    pub fn folder_mut(&mut self, path: &str) -> &mut FolderSettings {
        let k = folder_key(path);
        let now = crate::unix_ms(std::time::SystemTime::now());
        let entry = match self
            .settings
            .folders
            .iter()
            .position(|f| folder_key(&f.path) == k)
        {
            Some(i) => self.settings.folders.remove(i),
            None => FolderSettings {
                path: path.to_string(),
                instrument_class: None,
                include_existing: None,
                watch_mode: ModeChoice::Auto,
                last_used_ms: now,
            },
        };
        self.settings.folders.insert(0, entry);
        self.settings.folders.truncate(MAX_FOLDERS);
        let f = &mut self.settings.folders[0];
        f.last_used_ms = now;
        f
    }
}

/// Write `bytes` to `path` atomically: a sibling temp file, fsync, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("spyder-settings-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    #[test]
    fn round_trip_and_per_folder_switch() {
        let p = tmp("rt");
        let mut s = SettingsStore::load(&p);
        assert!(s.notice.is_none());
        s.folder_mut(r"D:\LabSpec\2026").instrument_class = Some(InstrumentClass::Hires);
        s.folder_mut("/Volumes/lab/drop").include_existing = Some(false);
        s.settings.resume_folder = Some("/Volumes/lab/drop".into());
        s.save().unwrap();
        let t = SettingsStore::load(&p);
        assert_eq!(t.settings, s.settings);
        assert_eq!(t.settings.folders[0].path, "/Volumes/lab/drop");
        let json = fs::read_to_string(&p).unwrap();
        assert!(json.contains("\"settingsVersion\": 1"));
        assert!(json.contains("\"instrumentClass\": \"hires\""));
    }

    #[test]
    fn folder_lookup_ignores_trailing_separators() {
        let p = tmp("key");
        let mut s = SettingsStore::load(&p);
        s.folder_mut("/data/scans/").instrument_class = Some(InstrumentClass::Standard);
        assert!(s.folder("/data/scans").is_some());
        assert_eq!(s.settings.folders.len(), 1);
        s.folder_mut("/data/scans");
        assert_eq!(s.settings.folders.len(), 1);
    }

    #[test]
    fn unversioned_file_migrates() {
        let p = tmp("v0");
        fs::write(
            &p,
            r#"{"folders":[{"path":"C:/x","instrumentClass":"standard"}]}"#,
        )
        .unwrap();
        let s = SettingsStore::load(&p);
        assert!(s.notice.is_none(), "{:?}", s.notice);
        assert_eq!(s.settings.settings_version, SETTINGS_VERSION);
        assert_eq!(
            s.settings.folders[0].instrument_class,
            Some(InstrumentClass::Standard)
        );
        assert_eq!(s.settings.folders[0].watch_mode, ModeChoice::Auto);
    }

    #[test]
    fn newer_file_is_never_overwritten() {
        let p = tmp("newer");
        let body = r#"{"settingsVersion":99,"somethingNew":true}"#;
        fs::write(&p, body).unwrap();
        let mut s = SettingsStore::load(&p);
        assert!(s.notice.as_deref().unwrap().contains("newer"));
        s.folder_mut("/x");
        s.save().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), body);
    }

    #[test]
    fn corrupt_file_is_kept_aside() {
        let p = tmp("corrupt");
        fs::write(&p, "{ not json").unwrap();
        let s = SettingsStore::load(&p);
        assert!(s.notice.as_deref().unwrap().contains("unreadable"));
        assert!(!p.exists());
        let kept = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt-"));
        assert!(kept);
        s.save().unwrap();
        assert!(p.exists());
    }
}
