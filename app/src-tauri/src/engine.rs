//! The analysis core inside the shell: the plug-in registry, loaded once at startup from the bundled
//! `plugins/` folder (shipped as a Tauri resource) plus the user's plug-in folder, and the display kit the
//! charts use. No Tauri types here, so the integration tests drive it directly.
//!
//! A bundled startup error (a failing shipped file, no verdict model) is a state, not a crash: nothing is
//! scored and the UI says "No verdict model available: <reason>" (PLAN section 4).

use std::path::{Path, PathBuf};

use spyder_core::pipeline::val::Obj;
use spyder_core::pipeline::Engine;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};

use crate::display::DisplayKit;

pub struct Core {
    reg: Option<Registry>,
    /// Why nothing can be scored (the startup-error state), in plain words.
    pub error: Option<String>,
    /// Registry notes (a user file disabled, a pin fallen back, ...): shown in About, never alarming.
    pub notes: Vec<String>,
    pub bundled_dir: PathBuf,
    pub user_dir: Option<PathBuf>,
    pub display: DisplayKit,
}

/// The bundled plug-in folder for this run: `$SPYDER_PLUGINS_DIR`, else the given resource folder (the
/// installed app's `plugins/`). Debug builds fall back to the repository's `plugins/`; release builds never do,
/// so a packaging fault shows as "No verdict model available" instead of working only on the build machine.
pub fn bundled_dir(resource_plugins: Option<PathBuf>) -> PathBuf {
    if let Some(p) = std::env::var_os("SPYDER_PLUGINS_DIR") {
        return PathBuf::from(p);
    }
    let dev = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins"));
    match resource_plugins {
        Some(p) if p.join("catalog.json").is_file() => p,
        Some(p) if !cfg!(debug_assertions) => p,
        None if !cfg!(debug_assertions) => PathBuf::from("plugins"),
        _ => dev,
    }
}

impl Core {
    /// Load the registry from the bundled folder and (if given) the user folder.
    pub fn load(bundled: &Path, user: Option<&Path>) -> Core {
        let mut locs = vec![Location {
            dir: bundled.to_path_buf(),
            origin: Origin::Bundled,
        }];
        if let Some(u) = user {
            if u.is_dir() {
                locs.push(Location {
                    dir: u.to_path_buf(),
                    origin: Origin::User,
                });
            }
        }
        let reg = Registry::load(&locs, &Pins::new());
        let notes = reg.notes.clone();
        let error = if reg.startup_error() {
            Some(reg.startup_errors.join("; "))
        } else {
            Engine::new(&reg).err()
        };
        let display = if error.is_none() {
            DisplayKit::new(&reg)
        } else {
            DisplayKit::unavailable("no verdict model is loaded")
        };
        Core {
            reg: if error.is_none() { Some(reg) } else { None },
            error,
            notes,
            bundled_dir: bundled.to_path_buf(),
            user_dir: user.map(Path::to_path_buf),
            display,
        }
    }

    /// The loaded registry (None in the startup-error state).
    pub fn registry(&self) -> Option<&Registry> {
        self.reg.as_ref()
    }

    /// A pipeline engine over the loaded registry (cheap: it resolves a dozen references).
    pub fn engine(&self) -> Option<Engine<'_>> {
        self.reg.as_ref().and_then(|r| Engine::new(r).ok())
    }

    /// `spyder analyse` on one file's bytes: the record of `Engine::analyse_bytes`, unchanged.
    pub fn analyse_bytes(
        &self,
        bytes: &[u8],
        name: &str,
        class: &str,
        source: &str,
    ) -> Option<Obj> {
        Some(self.engine()?.analyse_bytes(bytes, name, class, source))
    }

    /// The class a known serial says (registry `instruments.json`), if any.
    pub fn class_of_serial(&self, serial: u64) -> Option<&str> {
        self.reg.as_ref()?.instruments()?.class_of_serial(serial)
    }

    /// The class a file's header SWIR gains suggest (registry `header_hint`), if any: a heuristic for unlisted
    /// serials.
    pub fn class_of_swir_gains(&self, swir1: u64, swir2: u64) -> Option<&str> {
        self.reg
            .as_ref()?
            .instruments()?
            .class_of_swir_gains(swir1, swir2)
    }
}
