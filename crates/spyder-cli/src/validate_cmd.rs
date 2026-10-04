//! `spyder validate`: load plug-in files exactly as the app does (schema-level checks, sidecars, goldens on
//! load, cross-file rules, precedence) and report, per file, loaded or disabled with the reason.
//!
//! A folder is validated as a bundled plug-in folder unless `--user` is given (then a failing file is only
//! disabled, never a startup error). A single file is validated in the context of its own folder (its golden
//! spectra file, consensus components and transfers live there) and only that file is reported.
//!
//! Exit codes: 0 every reported file loads (and, for a bundled folder, no startup error); 1 otherwise; 2 usage.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use spyder_core::plugins::registry::{Entry, Location, Origin, Pins, Registry, State};

use crate::plugin_opts::add_pin;
use crate::read_cmd::write_stdout;

pub const USAGE: &str =
    "usage: spyder validate <plug-in file | folder> [--json] [--user] [--pin id@version]...

  Loads plug-in files as the app does: schema-level validation, sidecars (.npy,
  SHA-256, 16 MiB cap), goldens run on load (>= 3 cases, tolerance <= 1e-9),
  cross-file rules (consensus components, transfer keys, catalog), precedence
  and pins. Prints each file as loaded or disabled, with the reason.

  --json        one JSON document (schema spyder-bone/validate v1)
  --user        treat a folder as a user plug-in folder (no startup-error state)
  --pin id@ver  pin a version (repeatable)
";

#[derive(Serialize)]
struct FileOut<'a> {
    path: String,
    sha256: &'a str,
    format: Option<&'a str>,
    id: Option<&'a str>,
    version: Option<String>,
    status: Option<&'static str>,
    state: State,
    selected: bool,
    error_kind: Option<spyder_core::plugins::ErrorKind>,
    error: Option<&'a str>,
    golden_checks: Option<usize>,
    golden_failed: Option<usize>,
    /// Largest |got - want| / tolerance over the numeric comparisons (<= 1 passes).
    golden_worst_ratio: Option<f64>,
    /// Golden cases checked for content, run by the Phase 3 engine (engine checks, profiles).
    golden_cases_deferred: Option<usize>,
    notes: &'a [String],
}

#[derive(Serialize)]
struct Doc<'a> {
    schema: &'static str,
    schema_version: u32,
    spyder_core_version: &'static str,
    engine: String,
    target: String,
    origin: Origin,
    files: Vec<FileOut<'a>>,
    notes: &'a [String],
    startup_errors: Vec<&'a str>,
    ok: bool,
}

fn file_out<'a>(reg: &Registry, i: usize, e: &'a Entry) -> FileOut<'a> {
    FileOut {
        path: e.path.display().to_string(),
        sha256: &e.sha256,
        format: e.format.as_deref(),
        id: e.id.as_deref(),
        version: e.version.map(|v| v.to_string()),
        status: e.status.map(|s| s.as_str()),
        state: e.state,
        selected: reg.selected.values().any(|&k| k == i),
        error_kind: e.error.as_ref().map(|x| x.kind),
        error: e.error.as_ref().map(|x| x.message.as_str()),
        golden_checks: e.golden.as_ref().map(|g| g.checks),
        golden_failed: e.golden.as_ref().map(|g| g.failures.len()),
        golden_worst_ratio: e.golden.as_ref().map(|g| g.worst_ratio),
        golden_cases_deferred: e.golden_deferred,
        notes: &e.notes,
    }
}

pub fn run(args: &[OsString]) -> Result<(), (u8, String)> {
    let mut json = false;
    let mut user = false;
    let mut pins = Pins::new();
    let mut target: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str().unwrap_or("") {
            "--json" => json = true,
            "--user" => user = true,
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
    let Some(target) = target else {
        return Err((2, USAGE.to_string()));
    };
    if !target.exists() {
        return Err((1, format!("no such file or folder: {}", target.display())));
    }
    let single = target.is_file();
    let (dir, origin) = if single {
        (
            target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
            Origin::User,
        )
    } else {
        (
            target.clone(),
            if user { Origin::User } else { Origin::Bundled },
        )
    };
    let reg = Registry::load(&[Location { dir, origin }], &pins);
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let tcanon = canon(&target);
    let shown: Vec<(usize, &Entry)> = reg
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            if single {
                canon(&e.path) == tcanon
            } else {
                e.state != State::Ignored
            }
        })
        .collect();
    if single && shown.is_empty() {
        return Err((1, format!("{}: not a .json file", target.display())));
    }
    let startup: Vec<&str> = if origin == Origin::Bundled {
        reg.startup_errors.iter().map(String::as_str).collect()
    } else {
        Vec::new()
    };
    let ok = shown.iter().all(|(_, e)| e.state == State::Loaded) && startup.is_empty();
    let out = if json {
        let doc = Doc {
            schema: "spyder-bone/validate",
            schema_version: 1,
            spyder_core_version: env!("CARGO_PKG_VERSION"),
            engine: format!(
                "{}.{}",
                spyder_core::plugins::ENGINE_VERSION.0,
                spyder_core::plugins::ENGINE_VERSION.1
            ),
            target: target.display().to_string(),
            origin,
            files: shown.iter().map(|(i, e)| file_out(&reg, *i, e)).collect(),
            notes: &reg.notes,
            startup_errors: startup.clone(),
            ok,
        };
        let mut s = serde_json::to_string_pretty(&doc).map_err(|e| (1, e.to_string()))?;
        s.push('\n');
        s
    } else {
        let mut o = String::new();
        for (i, e) in &shown {
            let sel = if reg.selected.values().any(|k| k == i) {
                "*"
            } else {
                " "
            };
            let detail = match (&e.golden, e.golden_deferred, &e.error) {
                (_, _, Some(err)) => format!("{:?}: {}", err.kind, err.message),
                (Some(g), _, None) => format!(
                    "goldens {} checks passed (worst {:.2e} of tolerance)",
                    g.checks, g.worst_ratio
                ),
                (None, Some(n), None) => {
                    format!("{n} golden cases present (run by the Phase 3 engine)")
                }
                (None, None, None) if e.state == State::Ignored => e.notes.join("; "),
                _ => "valid (no goldens for this kind)".to_string(),
            };
            let _ = writeln!(
                o,
                "{sel}{:<9} {:<48} sha256 {}  {}  ({})",
                format!("{:?}", e.state).to_uppercase(),
                e.label(),
                spyder_core::plugins::short(&e.sha256, 12),
                detail,
                e.path.display()
            );
        }
        for n in &reg.notes {
            let _ = writeln!(o, "note: {n}");
        }
        for s in &startup {
            let _ = writeln!(o, "STARTUP ERROR: {s}");
        }
        let n_bad = shown
            .iter()
            .filter(|(_, e)| e.state != State::Loaded)
            .count();
        let _ = writeln!(
            o,
            "{}: {} file(s), {} loaded, {} not loaded{}  (* = selected)",
            if ok { "OK" } else { "FAILED" },
            shown.len(),
            shown.len() - n_bad,
            n_bad,
            if startup.is_empty() {
                ""
            } else {
                ", startup-error state"
            }
        );
        o
    };
    write_stdout(&out)?;
    if ok {
        Ok(())
    } else {
        Err((1, String::new()))
    }
}
