//! PRIVATE Phase 3 gate (PLAN section 8): the Rust pipeline equals the frozen Python oracle on every real
//! .asd file, for both instrument classes and all three analysis profiles: every flattened record field
//! (numbers within 1e-9 + 1e-9 |want|, labels exactly) and every CSV cell in the same column order.
//! Runs only when SPYDER_PRIVATE_DATA is set (else a visible SKIPPED line). Python values:
//! `planning/work/phase3/gen_private_oracle.py` (git-ignored), at $SPYDER_PRIVATE_ORACLE or
//! `<SPYDER_PRIVATE_DATA>/../planning/work/phase3`. No private name or value appears here or in the output.
//!
//!   SPYDER_PRIVATE_DATA="<private data folder>" cargo test -p spyder-core --test private_pipeline -- --nocapture

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::Value;
use spyder_core::pipeline::export::{cell, csv_row};
use spyder_core::pipeline::val::{flatten, matches_expected, V};
use spyder_core::pipeline::{Engine, ANALYSES};
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};

const TOL: f64 = 1e-9;

fn close(g: f64, w: f64) -> bool {
    (g.is_nan() && w.is_nan()) || (g.is_finite() && (g - w).abs() <= TOL + TOL * w.abs())
}

/// Compare JSON values with the float tolerance (CSV cells holding JSON lists or dicts).
fn json_close(g: &Value, w: &Value) -> bool {
    match (g, w) {
        (Value::Number(a), Value::Number(b)) => close(
            a.as_f64().unwrap_or(f64::NAN),
            b.as_f64().unwrap_or(f64::NAN),
        ),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| json_close(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|((ka, x), (kb, y))| ka == kb && json_close(x, y))
        }
        (a, b) => a == b,
    }
}

/// A CSV cell: floats (repr) within the tolerance, JSON cells element-wise, everything else exactly.
fn cell_close(g: &str, w: &str) -> bool {
    if g == w {
        return true;
    }
    if (w.starts_with('[') || w.starts_with('{')) && (g.starts_with('[') || g.starts_with('{')) {
        return match (
            serde_json::from_str::<Value>(g),
            serde_json::from_str::<Value>(w),
        ) {
            (Ok(a), Ok(b)) => json_close(&a, &b),
            _ => false,
        };
    }
    let is_float = |s: &str| s.contains(['.', 'e']) && s.parse::<f64>().is_ok();
    if is_float(w) && is_float(g) {
        return close(g.parse().unwrap(), w.parse().unwrap());
    }
    false
}

#[test]
fn private_pipeline_matches_the_oracle_on_the_real_spectra() {
    let Some(data) = std::env::var_os("SPYDER_PRIVATE_DATA").map(PathBuf::from) else {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED private_pipeline_matches_the_oracle_on_the_real_spectra: SPYDER_PRIVATE_DATA is not set"
        );
        return;
    };
    let dir = std::env::var_os("SPYDER_PRIVATE_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("..").join("planning").join("work").join("phase3"));
    let text = std::fs::read(dir.join("private_oracle.json")).unwrap_or_else(|e| {
        panic!(
            "private_oracle.json missing in {} ({e}); run gen_private_oracle.py",
            dir.display()
        )
    });
    let fx: Value = serde_json::from_slice(&text).unwrap();
    let reg = Registry::load(
        &[Location {
            dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
            origin: Origin::Bundled,
        }],
        &Pins::new(),
    );
    let eng = Engine::new(&reg).expect("engine");
    let (mut n_fields, mut n_cells, mut files) = (0usize, 0usize, 0usize);
    let mut bad: Vec<String> = Vec::new();
    for (i, f) in fx["files"].as_array().unwrap().iter().enumerate() {
        let p = data.join(f["path"].as_str().unwrap());
        let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("file #{i}: {e}"));
        files += 1;
        for (cls, want) in f["classes"].as_object().unwrap() {
            let rec = eng.analyse_bytes(&bytes, "scan.asd", cls, "user");
            let got: BTreeMap<String, V> = flatten(&V::Map(rec.clone()))
                .into_iter()
                .filter(|(k, _)| !k.starts_with("dependencies.") && !k.starts_with("input.file"))
                .collect();
            let wf = want["flat"].as_object().unwrap();
            for (k, w) in wf {
                n_fields += 1;
                if !got.get(k).is_some_and(|g| matches_expected(g, w, TOL, TOL)) {
                    // field names only: values derive from private spectra
                    bad.push(format!("file #{i} {cls}: field {k} differs"));
                }
            }
            for k in got.keys() {
                if !wf.contains_key(k) {
                    bad.push(format!("file #{i} {cls}: unexpected field {k}"));
                }
            }
            for a in ANALYSES {
                let row = csv_row(&rec, a);
                let cells: Vec<(String, String)> = row
                    .0
                    .iter()
                    .filter(|(k, _)| k != "file" && k != "dependencies")
                    .map(|(k, v)| (k.clone(), cell(v)))
                    .collect();
                let wrow = want["csv"][a].as_object().unwrap();
                let wcols: Vec<&String> = wrow.keys().collect();
                let gcols: Vec<&String> = cells.iter().map(|(k, _)| k).collect();
                let mut ws: Vec<&String> = wcols.clone();
                let mut gs: Vec<&String> = gcols.clone();
                ws.sort();
                gs.sort();
                if ws != gs {
                    bad.push(format!("file #{i} {cls} {a}: CSV columns differ"));
                    continue;
                }
                for (k, g) in &cells {
                    n_cells += 1;
                    if !cell_close(g, wrow[k].as_str().unwrap_or("")) {
                        bad.push(format!("file #{i} {cls} {a}: CSV cell {k} differs"));
                    }
                }
            }
        }
    }
    let _ = writeln!(
        std::io::stderr(),
        "private pipeline parity: {files} files x 2 classes x 3 profiles, {n_fields} record fields, {n_cells} CSV cells, {} mismatches",
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad[..bad.len().min(30)].join("\n")
    );
}
