//! PLAN Phase 3 gate: the Rust pipeline reproduces every end-to-end oracle golden
//! (`reference/oracle/goldens/oracle_public_v1.json`): every flattened output, numbers within
//! 1e-9 + 1e-9 |want|, labels, booleans and null exactly, and no output the oracle does not produce
//! (keys starting with `dependencies.` or `input.file` are not compared, as in `make_goldens.compare`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;
use spyder_core::pipeline::val::{flatten, matches_expected, V};
use spyder_core::pipeline::{Context, Engine};
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn b64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

fn load() -> Registry {
    Registry::load(
        &[Location {
            dir: repo().join("plugins"),
            origin: Origin::Bundled,
        }],
        &Pins::new(),
    )
}

#[test]
fn rust_pipeline_reproduces_the_oracle_goldens() {
    let reg = load();
    assert!(reg.startup_errors.is_empty(), "{:?}", reg.startup_errors);
    let eng = Engine::new(&reg).expect("engine");
    let gdir = repo().join("reference/oracle/goldens");
    let e: Value =
        serde_json::from_slice(&std::fs::read(gdir.join("oracle_public_v1.json")).unwrap())
            .unwrap();
    let sp: Value = serde_json::from_slice(
        &std::fs::read(gdir.join(e["spectra_file"].as_str().unwrap())).unwrap(),
    )
    .unwrap();
    let spectra: BTreeMap<String, Vec<f64>> = sp["spectra"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["sha256"].as_str().unwrap().to_string(),
                s["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect(),
            )
        })
        .collect();
    let wl: Vec<f64> = (0..2151).map(|i| 350.0 + i as f64).collect();
    let (abs, rel) = (1e-9, 1e-9);
    let (mut n, mut cases, mut bad) = (0usize, 0usize, Vec::new());
    for c in e["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let ctx = &c["context"];
        let class = ctx["instrument_class"].as_str().unwrap();
        let source = ctx["class_source"].as_str().unwrap_or("user");
        let rec = if let Some(b) = c["asd_b64"].as_str() {
            eng.analyse_bytes(&b64(b), &format!("{name}.asd"), class, source)
        } else {
            let x = &spectra[c["spectrum_sha256"].as_str().unwrap()];
            let c = Context {
                instrument_class: class.to_string(),
                class_source: source.to_string(),
                serial: ctx["serial"].as_u64(),
                splices_nm: ctx["splices_nm"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_f64().unwrap()).collect())
                    .unwrap_or(vec![1000.0, 1800.0]),
            };
            eng.analyse_spectrum(&wl, x, &c)
        };
        let got: BTreeMap<String, V> = flatten(&V::Map(rec))
            .into_iter()
            .filter(|(k, _)| !k.starts_with("dependencies.") && !k.starts_with("input.file"))
            .collect();
        let want = c["expected"].as_object().unwrap();
        for (k, w) in want {
            n += 1;
            match got.get(k) {
                Some(g) if matches_expected(g, w, abs, rel) => {}
                g => bad.push(format!("{name}: {k}: got {g:?}, want {w}")),
            }
        }
        for k in got.keys() {
            if !want.contains_key(k) {
                bad.push(format!("{name}: unexpected output {k}"));
            }
        }
        cases += 1;
    }
    eprintln!(
        "oracle goldens: {cases} cases, {n} comparisons, {} failed",
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad[..bad.len().min(40)].join("\n")
    );
    assert_eq!(cases, 54);
}

/// Numbers (Python repr) within 1e-9 + 1e-9 |want|; JSON-valued cells element-wise; everything else exactly.
fn cell_close(g: &str, w: &str) -> bool {
    fn jclose(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
                (x - y).abs() <= 1e-9 + 1e-9 * y.abs()
            }
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(p, q)| jclose(p, q))
            }
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len()
                    && x.iter()
                        .zip(y)
                        .all(|((k1, p), (k2, q))| k1 == k2 && jclose(p, q))
            }
            (p, q) => p == q,
        }
    }
    if g == w {
        return true;
    }
    if w.starts_with(['[', '{']) {
        return match (
            serde_json::from_str::<Value>(g),
            serde_json::from_str::<Value>(w),
        ) {
            (Ok(a), Ok(b)) => jclose(&a, &b),
            _ => false,
        };
    }
    let fl = |s: &str| s.contains(['.', 'e']) && s.parse::<f64>().is_ok();
    fl(g) && fl(w) && {
        let (x, y): (f64, f64) = (g.parse().unwrap(), w.parse().unwrap());
        (x - y).abs() <= 1e-9 + 1e-9 * y.abs()
    }
}

/// Minimal RFC 4180 reader (the oracle writes QUOTE_MINIMAL, CRLF).
fn parse_csv(t: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cur) = (Vec::new(), Vec::new(), String::new());
    let (mut q, mut chars) = (false, t.chars().peekable());
    while let Some(c) = chars.next() {
        match (q, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            (true, '"') => q = false,
            (true, c) => cur.push(c),
            (false, '"') => q = true,
            (false, ',') => row.push(std::mem::take(&mut cur)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cur));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => cur.push(c),
        }
    }
    rows
}

#[test]
fn csv_rows_and_file_match_the_oracle() {
    use spyder_core::pipeline::export::{cell, csv_row, csv_text};
    let reg = load();
    let eng = Engine::new(&reg).expect("engine");
    let gdir = repo().join("reference/oracle/goldens");
    let e: Value =
        serde_json::from_slice(&std::fs::read(gdir.join("oracle_public_v1.json")).unwrap())
            .unwrap();
    let sp: Value = serde_json::from_slice(
        &std::fs::read(gdir.join(e["spectra_file"].as_str().unwrap())).unwrap(),
    )
    .unwrap();
    let spectra: BTreeMap<String, Vec<f64>> = sp["spectra"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["sha256"].as_str().unwrap().to_string(),
                s["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect(),
            )
        })
        .collect();
    let fx: Value = serde_json::from_slice(
        &std::fs::read(repo().join("crates/spyder-core/tests/goldens/oracle_csv_v1.json")).unwrap(),
    )
    .unwrap();
    let wl: Vec<f64> = (0..2151).map(|i| 350.0 + i as f64).collect();
    let mut bad = Vec::new();
    let mut rc_rows = Vec::new();
    let mut n = 0;
    for (c, want) in e["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(fx["cases"].as_array().unwrap())
    {
        let name = c["name"].as_str().unwrap();
        assert_eq!(want["name"], name);
        let ctx = &c["context"];
        let class = ctx["instrument_class"].as_str().unwrap();
        let rec = if let Some(b) = c["asd_b64"].as_str() {
            eng.analyse_bytes(&b64(b), &format!("{name}.asd"), class, "user")
        } else {
            let cx = Context {
                instrument_class: class.to_string(),
                class_source: "user".into(),
                serial: ctx["serial"].as_u64(),
                splices_nm: ctx["splices_nm"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_f64().unwrap()).collect())
                    .unwrap_or(vec![1000.0, 1800.0]),
            };
            eng.analyse_spectrum(&wl, &spectra[c["spectrum_sha256"].as_str().unwrap()], &cx)
        };
        for a in spyder_core::pipeline::ANALYSES {
            let mut row = csv_row(&rec, a);
            row.set("file", name);
            row.set("dependencies", "");
            let got: Vec<(String, String)> =
                row.0.iter().map(|(k, v)| (k.clone(), cell(v))).collect();
            let w: Vec<(String, String)> = want["rows"][a]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    (
                        p[0].as_str().unwrap().to_string(),
                        p[1].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            let gk: Vec<&String> = got.iter().map(|(k, _)| k).collect();
            let wk: Vec<&String> = w.iter().map(|(k, _)| k).collect();
            if gk != wk {
                bad.push(format!(
                    "{name} {a}: column order differs:\n got {gk:?}\nwant {wk:?}"
                ));
                continue;
            }
            for ((k, g), (_, wv)) in got.iter().zip(&w) {
                n += 1;
                if !cell_close(g, wv) {
                    bad.push(format!("{name} {a} {k}: got {g:?} want {wv:?}"));
                }
            }
            if a == "radiocarbon" {
                rc_rows.push(row);
            }
        }
    }
    // the whole file: BOM, CRLF, header, quoting
    let text = csv_text(&rc_rows);
    let wtext = fx["radiocarbon_csv"].as_str().unwrap();
    assert!(text.starts_with('\u{feff}') && wtext.starts_with('\u{feff}'));
    assert!(text.ends_with("\r\n"));
    let (g, w) = (parse_csv(&text[3..]), parse_csv(&wtext[3..]));
    assert_eq!(g.len(), w.len(), "row count");
    assert_eq!(g[0], w[0], "header");
    assert_eq!(text.lines().next(), wtext.lines().next(), "header bytes");
    for (i, (gr, wr)) in g.iter().zip(&w).enumerate().skip(1) {
        assert_eq!(gr.len(), wr.len(), "row {i} length");
        for (j, (a, b)) in gr.iter().zip(wr).enumerate() {
            if !cell_close(a, b) {
                bad.push(format!("csv row {i} col {}: got {a:?} want {b:?}", w[0][j]));
            }
        }
    }
    eprintln!("oracle CSV: {n} cells compared, {} mismatches", bad.len());
    assert!(bad.is_empty(), "{}", bad[..bad.len().min(30)].join("\n"));
}
