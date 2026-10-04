//! Options shared by `validate` and `predict`: plug-in folders and pins.

use std::ffi::OsString;
use std::path::PathBuf;

use spyder_core::plugins::registry::Pins;
use spyder_core::plugins::Version;

/// Parse `id@MAJOR.MINOR.PATCH` into the pin map.
pub fn add_pin(pins: &mut Pins, arg: Option<&OsString>) -> Result<(), (u8, String)> {
    let s = arg
        .and_then(|a| a.to_str())
        .ok_or_else(|| (2, "--pin needs id@version".to_string()))?;
    let (id, v) = s
        .rsplit_once('@')
        .ok_or_else(|| (2, format!("--pin {s:?}: expected id@version")))?;
    let v = Version::parse(v)
        .ok_or_else(|| (2, format!("--pin {s:?}: version must be MAJOR.MINOR.PATCH")))?;
    pins.insert(id.to_string(), v);
    Ok(())
}

/// The bundled plug-in folder: `--plugins`, else `$SPYDER_PLUGINS_DIR`, else `plugins/` next to the executable
/// (or two and three levels up, for `target/<profile>/spyder`), else the repository's `plugins/`.
pub fn bundled_dir(given: Option<PathBuf>) -> PathBuf {
    if let Some(p) = given {
        return p;
    }
    if let Some(p) = std::env::var_os("SPYDER_PLUGINS_DIR") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut d = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..4 {
            let Some(dir) = d else { break };
            let cand = dir.join("plugins");
            if cand.join("catalog.json").is_file() {
                return cand;
            }
            d = dir.parent().map(|p| p.to_path_buf());
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins"))
}

/// The user plug-in folder: `--user-plugins`, else `$SPYDER_USER_PLUGINS_DIR`, else none.
pub fn user_dir(given: Option<PathBuf>) -> Option<PathBuf> {
    given.or_else(|| std::env::var_os("SPYDER_USER_PLUGINS_DIR").map(PathBuf::from))
}
