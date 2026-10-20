//! Options shared by `validate`, `predict` and `analyse`: plug-in folders, pins and the instrument class.

use std::ffi::OsString;
use std::path::PathBuf;

use spyder_core::plugins::registry::{Pins, Registry};
use spyder_core::plugins::{Version, CLASS_HIRES, CLASS_STD};

/// A `--class` value as given on the command line: a switch alias, or a registry class id (`maker.model...`); checked
/// against the loaded registry later by [`resolve_class`].
pub fn class_arg(arg: Option<&OsString>, usage: &str) -> Result<String, (u8, String)> {
    match arg.and_then(|c| c.to_str()) {
        Some(c @ ("std" | "standard" | "hires" | "high-res")) => Ok(c.to_string()),
        Some(c) if c.contains('.') && !c.starts_with('-') => Ok(c.to_string()),
        _ => Err((
            2,
            format!(
                "--class must be std, hires or an instrument class id

{usage}"
            ),
        )),
    }
}

/// The instrument class a `--class` value names: `std` / `standard` or `hires` / `high-res` (the LabSpec 4 switch),
/// or any class the loaded instrument registry lists (DECISIONS 137). A file another instrument wrote is scored as
/// that instrument's class whatever this says (the registry names it by file format and detector joins).
pub fn resolve_class(reg: &Registry, raw: &str, usage: &str) -> Result<String, (u8, String)> {
    let c = match raw {
        "std" | "standard" => CLASS_STD,
        "hires" | "high-res" => CLASS_HIRES,
        other => other,
    };
    let known =
        c == CLASS_STD || c == CLASS_HIRES || reg.instruments().is_some_and(|i| i.has_class(c));
    if known {
        Ok(c.to_string())
    } else {
        let listed: Vec<String> = reg
            .instruments()
            .map(|i| i.classes.keys().cloned().collect())
            .unwrap_or_default();
        Err((
            2,
            format!(
                "--class {raw:?}: use std, hires or a class of the instrument registry ({})\n\n{usage}",
                listed.join(", ")
            ),
        ))
    }
}

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
