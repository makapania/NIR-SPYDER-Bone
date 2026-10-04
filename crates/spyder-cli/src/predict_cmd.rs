//! `spyder predict`: read .asd files, apply the transfer for high-res (Step 4 rule), run every selected active
//! model and CONS3, and print the predictions with id@version and file SHA-256 (and the transfer's).
//!
//! The instrument class is the user's switch (`--class std|hires`, DECISIONS 50); a serial listed under the
//! other class only adds a gentle note. Exit codes: 0 done (rejected files are a result, not an error);
//! 1 path or plug-in folder problem, or the startup-error state (no scan is scored); 2 usage.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};
use spyder_core::plugins::{short, ScanContext, CLASS_HIRES, CLASS_STD};
use spyder_core::predict::{predict_scan, ModelResult};
use spyder_core::read;

use crate::plugin_opts::{add_pin, bundled_dir, user_dir};
use crate::read_cmd::{collect_inputs, write_stdout};

pub const USAGE: &str =
    "usage: spyder predict <file.asd | folder> --class std|hires [--json] [--recursive]
                      [--plugins DIR] [--user-plugins DIR] [--pin id@version]...

  Reads ASD LabSpec 4 .asd files, applies the high-res -> standard-res transfer
  where a model needs it (provisional), and runs every active model and CONS3.
  Prints each prediction (signed, unrounded) with the model's id@version and
  file SHA-256, the transfer used, and the reading's noise SD.

  --class std|hires   the instrument class (the Standard / High-res switch)
  --json              one JSON document (schema spyder-bone/predict v1)
  --recursive         include sub-folders (links and junctions are not followed)
  --plugins DIR       bundled plug-in folder (default: $SPYDER_PLUGINS_DIR or the shipped plugins/)
  --user-plugins DIR  user plug-in folder (default: $SPYDER_USER_PLUGINS_DIR, none if unset)
  --pin id@ver        pin a plug-in version (repeatable)
";

#[derive(Serialize)]
struct FileOut {
    path: String,
    file_name: String,
    /// "accepted" | "not_scored" | "rejected"
    status: &'static str,
    reason: Option<String>,
    serial: Option<u16>,
    splices_nm: Option<Vec<f64>>,
    /// Gentle note when the serial is listed under the other class.
    class_note: Option<String>,
    models: Vec<ModelResult>,
}

#[derive(Serialize)]
struct PluginsOut<'a> {
    bundled: String,
    user: Option<String>,
    notes: &'a [String],
    startup_errors: &'a [String],
}

#[derive(Serialize)]
struct Doc<'a> {
    schema: &'static str,
    schema_version: u32,
    spyder_core_version: &'static str,
    engine: String,
    instrument_class: &'a str,
    class_source: &'static str,
    plugins: PluginsOut<'a>,
    files: Vec<FileOut>,
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn one(reg: &Registry, path: &Path, class: &str) -> FileOut {
    let mut f = FileOut {
        path: path.display().to_string(),
        file_name: file_name(path),
        status: "accepted",
        reason: None,
        serial: None,
        splices_nm: None,
        class_note: None,
        models: Vec::new(),
    };
    let scan = match read::read_file(path) {
        Ok(s) => s,
        Err(e) => {
            f.status = "rejected";
            f.reason = Some(e.to_string());
            return f;
        }
    };
    f.serial = Some(scan.header.serial);
    f.splices_nm = Some(scan.splices_nm.clone());
    if !scan.kind.is_scored() {
        f.status = "not_scored";
        f.reason = Some(scan.kind.label().to_string());
        return f;
    }
    f.class_note = reg
        .instruments()
        .and_then(|i| i.mismatch(u64::from(scan.header.serial), class));
    let ctx = ScanContext::new(
        scan.splices_nm.clone(),
        class,
        Some(scan.header.serial.to_string()),
    );
    f.models = predict_scan(reg, &scan.wavelengths_nm, &scan.reflectance, &ctx);
    f
}

fn text(o: &mut String, f: &FileOut) {
    let _ = writeln!(o, "== {}", f.path);
    if f.status != "accepted" {
        let _ = writeln!(
            o,
            "   {}: {}",
            f.status.replace('_', " "),
            f.reason.as_deref().unwrap_or("")
        );
        return;
    }
    if let Some(n) = &f.class_note {
        let _ = writeln!(o, "   note: {n}");
    }
    for m in &f.models {
        let name = m.short_name.clone().unwrap_or_else(|| m.id.clone());
        let tr = match &m.transfer {
            Some(t) => format!(
                "  via {}@{} [{}]{}",
                t.id,
                t.version,
                short(&t.sha256, 12),
                if t.provisional { " provisional" } else { "" }
            ),
            None => String::new(),
        };
        match &m.prediction {
            Some(p) => {
                let comps = if p.components.is_empty() {
                    String::new()
                } else {
                    format!(
                        "  ({})",
                        p.components
                            .iter()
                            .map(|(n, v)| format!("{n} {v:.4}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let sd = m
                    .noise_sd_pct
                    .map(|s| format!("  noise SD {s:.3}"))
                    .unwrap_or_default();
                let _ = writeln!(
                    o,
                    "   {:<34} {:>9.4}{comps}{sd}  {}@{} [{}]{tr}",
                    name,
                    p.value,
                    m.id,
                    m.version,
                    short(&m.sha256, 12)
                );
            }
            None => {
                let _ = writeln!(
                    o,
                    "   {:<34} {}  {}@{} [{}]{tr}",
                    name,
                    m.assessment.describe(),
                    m.id,
                    m.version,
                    short(&m.sha256, 12)
                );
            }
        }
        if let Some(n) = &m.transfer_note {
            if m.transfer.is_none() {
                let _ = writeln!(o, "      note: {n}");
            }
        }
    }
}

pub fn run(args: &[OsString]) -> Result<(), (u8, String)> {
    let mut json = false;
    let mut recursive = false;
    let mut class: Option<&'static str> = None;
    let mut pins = Pins::new();
    let mut plugins: Option<PathBuf> = None;
    let mut user: Option<PathBuf> = None;
    let mut target: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str().unwrap_or("") {
            "--json" => json = true,
            "--recursive" | "-r" => recursive = true,
            "--class" => {
                class = Some(match it.next().and_then(|c| c.to_str()) {
                    Some("std" | "standard" | CLASS_STD) => CLASS_STD,
                    Some("hires" | "high-res" | CLASS_HIRES) => CLASS_HIRES,
                    _ => return Err((2, format!("--class must be std or hires\n\n{USAGE}"))),
                })
            }
            "--plugins" => {
                plugins = Some(PathBuf::from(
                    it.next()
                        .ok_or((2, "--plugins needs a folder".to_string()))?,
                ))
            }
            "--user-plugins" => {
                user = Some(PathBuf::from(
                    it.next()
                        .ok_or((2, "--user-plugins needs a folder".to_string()))?,
                ))
            }
            "--pin" => add_pin(&mut pins, it.next())?,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            s if s.starts_with('-') => return Err((2, format!("unknown option {s:?}\n\n{USAGE}"))),
            _ => {
                if target.replace(PathBuf::from(a)).is_some() {
                    return Err((2, format!("only one file or folder, please\n\n{USAGE}")));
                }
            }
        }
    }
    let (Some(target), Some(class)) = (target, class) else {
        return Err((2, USAGE.to_string()));
    };
    let files = collect_inputs(&target, recursive)?;
    let bundled = bundled_dir(plugins);
    let user = user_dir(user);
    let mut locs = vec![Location {
        dir: bundled.clone(),
        origin: Origin::Bundled,
    }];
    if let Some(u) = &user {
        locs.push(Location {
            dir: u.clone(),
            origin: Origin::User,
        });
    }
    let reg = Registry::load(&locs, &pins);
    if !bundled.is_dir() {
        return Err((
            1,
            format!("plug-in folder not found: {}", bundled.display()),
        ));
    }
    // Codex Phase 2 HIGH 3: a bundled startup error stops scoring (exit 1, with the reasons)
    if reg.startup_error() {
        return Err((
            1,
            format!(
                "startup error: no scan was scored ({} problem(s)):
  {}",
                reg.startup_errors.len(),
                reg.startup_errors.join(
                    "
  "
                )
            ),
        ));
    }
    for n in &reg.notes {
        eprintln!("note: {n}");
    }
    let outs: Vec<FileOut> = files.iter().map(|p| one(&reg, p, class)).collect();
    let out = if json {
        let doc = Doc {
            schema: "spyder-bone/predict",
            schema_version: 1,
            spyder_core_version: env!("CARGO_PKG_VERSION"),
            engine: format!(
                "{}.{}",
                spyder_core::plugins::ENGINE_VERSION.0,
                spyder_core::plugins::ENGINE_VERSION.1
            ),
            instrument_class: class,
            class_source: "--class",
            plugins: PluginsOut {
                bundled: bundled.display().to_string(),
                user: user.map(|u| u.display().to_string()),
                notes: &reg.notes,
                startup_errors: &reg.startup_errors,
            },
            files: outs,
        };
        let mut s = serde_json::to_string_pretty(&doc).map_err(|e| (1, e.to_string()))?;
        s.push('\n');
        s
    } else {
        let mut o = String::new();
        let label = reg
            .instruments()
            .and_then(|i| i.display_name(class))
            .unwrap_or(class);
        let _ = writeln!(o, "instrument class: {label} ({class}, set by --class)");
        for f in &outs {
            text(&mut o, f);
        }
        let n_ok = outs.iter().filter(|f| f.status == "accepted").count();
        let _ = writeln!(
            o,
            "{} file(s): {} scored, {} not scored or rejected",
            outs.len(),
            n_ok,
            outs.len() - n_ok
        );
        o
    };
    write_stdout(&out)
}
