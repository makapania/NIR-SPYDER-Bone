//! `spyder analyse`: the full per-scan pipeline (PLAN Steps 1-10): acquisition checks, the stream transfer,
//! models and CONS3, evidence, ZooMS pattern, signs, C1, B9, long-wave policy, B6/B6b and the verdict of one
//! analysis profile, with the CSV row (UTF-8 with BOM) and the analysis manifest. The records equal the frozen
//! Python oracle's (`reference/oracle`).
//!
//! A bundled startup error (a failing shipped file, no verdict model) stops scoring: exit 1 with the reasons.
//! Exit codes: 0 done (rejected files are a result); 1 path, plug-in or output problem; 2 usage.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Serialize;
use spyder_core::pipeline::export::{csv_text, manifest};
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::{Engine, ANALYSES};
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};
use spyder_core::plugins::{CLASS_HIRES, CLASS_STD};

use crate::plugin_opts::{add_pin, bundled_dir, user_dir};
use crate::read_cmd::{collect_inputs, write_stdout};

pub const USAGE: &str = "usage: spyder analyse <file.asd | folder> --class std|hires
                      [--profile radiocarbon|isotopes|zooms] [--csv out.csv] [--json]
                      [--sort] [--recursive] [--plugins DIR] [--user-plugins DIR] [--pin id@version]...

  Runs the full pipeline on each scan and prints its verdict for one analysis
  type: the verdict and the rule step that set it, the CONS3 reading, the
  organic-evidence level, the ZooMS band pattern, the notes shown (with the
  ZooMS line where the ZooMS band check calls the scan better) and the flags
  (contaminant / burnt flags never change the verdict).

  --class std|hires   the instrument class (the Standard / High-res switch)
  --profile NAME      radiocarbon (default), isotopes or zooms
  --csv FILE          write one CSV row per scan (UTF-8 with BOM; every number unrounded)
  --json              one JSON document: every scan's full record and its analysis manifest
  --sort              most promising scans first (the profile's sort key)
  --recursive         include sub-folders (links and junctions are not followed)
  --plugins DIR       bundled plug-in folder (default: $SPYDER_PLUGINS_DIR or the shipped plugins/)
  --user-plugins DIR  user plug-in folder (default: $SPYDER_USER_PLUGINS_DIR)
  --pin id@ver        pin a plug-in version (repeatable)
";

#[derive(Serialize)]
struct Item {
    path: String,
    record: Obj,
    manifest: Obj,
}

#[derive(Serialize)]
struct Doc<'a> {
    schema: &'static str,
    schema_version: u32,
    spyder_core_version: &'static str,
    instrument_class: &'a str,
    class_source: &'static str,
    profile: &'a str,
    plugin_notes: &'a [String],
    files: Vec<Item>,
}

fn s<'a>(o: &'a Obj, path: &[&str]) -> Option<&'a V> {
    let mut cur = o;
    for (i, k) in path.iter().enumerate() {
        let v = cur.get(k)?;
        if i + 1 == path.len() {
            return Some(v);
        }
        cur = v.as_obj()?;
    }
    None
}

fn text_of(v: Option<&V>) -> String {
    match v {
        None | Some(V::Null) => "-".into(),
        Some(V::Str(x)) => x.clone(),
        Some(V::Num(x)) => format!("{x:.3}"),
        Some(V::Int(i)) => i.to_string(),
        Some(V::Bool(b)) => b.to_string(),
        Some(V::List(l)) => l
            .iter()
            .map(|x| match x {
                V::Str(s) => s.clone(),
                V::Map(m) => {
                    let k = m.get("key").and_then(V::as_str).unwrap_or("");
                    match m.get("signs") {
                        Some(V::List(sg)) => format!(
                            "{k}({})",
                            sg.iter()
                                .filter_map(V::as_str)
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        _ => k.to_string(),
                    }
                }
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => format!("{other:?}"),
    }
}

fn line(o: &mut String, path: &str, rec: &Obj, a: &str) {
    let _ = writeln!(o, "== {path}");
    let v = |p: &[&str]| text_of(s(rec, p));
    let _ = writeln!(
        o,
        "   {}  ({})   CONS3 {}   evidence {}   ZooMS pattern {}",
        v(&[a, "verdict"]),
        v(&[a, "rule_step"]),
        v(&["models", "cons3", "value"]),
        v(&["evidence", "level"]),
        v(&["zooms_pattern", "pattern"]),
    );
    if let Some(r) = s(rec, &["checks", "B1", "reason"]) {
        let _ = writeln!(o, "   {}", text_of(Some(r)));
    }
    let notes = v(&[a, "notes_shown"]);
    if !notes.is_empty() && notes != "-" {
        let _ = writeln!(o, "   notes: {notes}");
    }
    let flags = v(&[a, "flags"]);
    if !flags.is_empty() && flags != "-" {
        let _ = writeln!(o, "   flags: {flags}");
    }
    let inst = v(&["instrument", "notes"]);
    if !inst.is_empty() && inst != "-" {
        let _ = writeln!(o, "   instrument: {inst}");
    }
    if let Some(V::Str(t)) = s(rec, &["stream", "transfer"]) {
        if t != "none" {
            let _ = writeln!(o, "   transfer {t} (provisional)");
        }
    }
}

pub fn run(args: &[OsString]) -> Result<(), (u8, String)> {
    let (mut json, mut recursive, mut sort) = (false, false, false);
    let mut class: Option<&'static str> = None;
    let mut profile = "radiocarbon".to_string();
    let mut csv: Option<PathBuf> = None;
    let mut pins = Pins::new();
    let (mut plugins, mut user, mut target): (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) =
        (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str().unwrap_or("") {
            "--json" => json = true,
            "--sort" => sort = true,
            "--recursive" | "-r" => recursive = true,
            "--class" => {
                class = Some(match it.next().and_then(|c| c.to_str()) {
                    Some("std" | "standard" | CLASS_STD) => CLASS_STD,
                    Some("hires" | "high-res" | CLASS_HIRES) => CLASS_HIRES,
                    _ => return Err((2, format!("--class must be std or hires\n\n{USAGE}"))),
                })
            }
            "--profile" => {
                profile = match it.next().and_then(|c| c.to_str()) {
                    Some(p) if ANALYSES.contains(&p) => p.to_string(),
                    _ => {
                        return Err((
                            2,
                            format!("--profile must be radiocarbon, isotopes or zooms\n\n{USAGE}"),
                        ))
                    }
                }
            }
            "--csv" => {
                csv = Some(PathBuf::from(
                    it.next()
                        .ok_or((2, "--csv needs a file name".to_string()))?,
                ))
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
            x if x.starts_with('-') => return Err((2, format!("unknown option {x:?}\n\n{USAGE}"))),
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
    let mut locs = vec![Location {
        dir: bundled.clone(),
        origin: Origin::Bundled,
    }];
    if let Some(u) = user_dir(user) {
        locs.push(Location {
            dir: u,
            origin: Origin::User,
        });
    }
    let reg = Registry::load(&locs, &pins);
    // Codex Phase 2 HIGH 3: the startup-error state stops scoring
    if reg.startup_error() {
        return Err((
            1,
            format!(
                "startup error: no scan was scored ({} problem(s)):\n  {}",
                reg.startup_errors.len(),
                reg.startup_errors.join("\n  ")
            ),
        ));
    }
    let eng = Engine::new(&reg).map_err(|e| (1, format!("startup error: {e}")))?;
    for n in &reg.notes {
        eprintln!("note: {n}");
    }
    let mut recs: Vec<(String, Obj)> = files
        .iter()
        .map(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let rec = match std::fs::read(p) {
                Ok(b) => eng.analyse_bytes(&b, &name, class, "user"),
                Err(e) => eng.analyse_unreadable(&name, &format!("cannot read file: {e}")),
            };
            (p.display().to_string(), rec)
        })
        .collect();
    if sort {
        let key = |r: &Obj| {
            let a = r.get(&profile).and_then(V::as_obj);
            let g = a
                .and_then(|o| o.get("sort_group"))
                .and_then(V::as_f64)
                .unwrap_or(f64::INFINITY);
            let v = a
                .and_then(|o| o.get("sort_value"))
                .and_then(V::as_f64)
                .unwrap_or(f64::NEG_INFINITY);
            (g, v)
        };
        recs.sort_by(|(_, a), (_, b)| {
            let (ga, va) = key(a);
            let (gb, vb) = key(b);
            ga.total_cmp(&gb).then(vb.total_cmp(&va))
        });
    }
    if let Some(c) = &csv {
        let rows: Vec<Obj> = recs
            .iter()
            .map(|(_, r)| spyder_core::pipeline::export::csv_row(r, &profile))
            .collect();
        std::fs::write(c, csv_text(&rows).as_bytes())
            .map_err(|e| (1, format!("cannot write {}: {e}", c.display())))?;
    }
    let out = if json {
        let doc = Doc {
            schema: "spyder-bone/analyse",
            schema_version: 1,
            spyder_core_version: env!("CARGO_PKG_VERSION"),
            instrument_class: class,
            class_source: "--class",
            profile: &profile,
            plugin_notes: &reg.notes,
            files: recs
                .iter()
                .map(|(p, r)| Item {
                    path: p.clone(),
                    manifest: manifest(r, &profile, 1),
                    record: r.clone(),
                })
                .collect(),
        };
        let mut t = serde_json::to_string_pretty(&doc).map_err(|e| (1, e.to_string()))?;
        t.push('\n');
        t
    } else {
        let mut o = String::new();
        let label = reg
            .instruments()
            .and_then(|i| i.display_name(class))
            .unwrap_or(class);
        let _ = writeln!(
            o,
            "{profile}: instrument class {label} ({class}, set by --class)"
        );
        for (p, r) in &recs {
            line(&mut o, p, r, &profile);
        }
        let _ = writeln!(o, "{} file(s)", recs.len());
        if let Some(c) = &csv {
            let _ = writeln!(o, "CSV written to {}", c.display());
        }
        o
    };
    write_stdout(&out)
}
