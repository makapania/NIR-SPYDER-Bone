//! PRIVATE parity tests for the other-instrument reader (DECISIONS 87-88): every FieldSpec 3 legacy file (`.NNN`,
//! `.sco`) and Trek-type `as7` file of the private set. Run only when SPYDER_PRIVATE_DATA points at the private
//! `data/` folder; otherwise each test prints a visible SKIPPED line and passes. No private file name or value
//! appears in this file.
//!
//! Goldens come from `planning/work/other_instruments/gen_private_goldens_instruments.py` (git-ignored), found at
//! $SPYDER_PRIVATE_INSTRUMENTS or `<SPYDER_PRIVATE_DATA>/../planning/work/other_instruments` (as the other private
//! suites' SPYDER_PRIVATE_GOLDENS / _PREDICTIONS / _ORACLE, so a branch can run against its own fixtures).
//!
//!   SPYDER_PRIVATE_DATA="<private data folder>" cargo test -p spyder-core --test private_instruments -- --nocapture

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::Value;
use spyder_core::pipeline::val::{flatten, matches_expected, V};
use spyder_core::pipeline::Engine;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};
use spyder_core::read::read_file;
use spyder_core::scan::FileFormat;

fn private(test: &str) -> Option<(PathBuf, Value)> {
    let Some(data) = std::env::var_os("SPYDER_PRIVATE_DATA").map(PathBuf::from) else {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED {test}: SPYDER_PRIVATE_DATA is not set (private parity tests need the private data)"
        );
        return None;
    };
    let g = std::env::var_os("SPYDER_PRIVATE_INSTRUMENTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("..").join("planning/work/other_instruments"))
        .join("private_goldens_instruments.json");
    let text = std::fs::read_to_string(&g).unwrap_or_else(|e| {
        panic!(
            "SPYDER_PRIVATE_DATA is set but {} is missing ({e}); run gen_private_goldens_instruments.py",
            g.display()
        )
    });
    Some((
        data.join("other_instruments"),
        serde_json::from_str(&text).unwrap(),
    ))
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_private_instrument_file_matches_the_oracle() {
    let Some((dir, g)) = private("every_private_instrument_file_matches_the_oracle") else {
        return;
    };
    let reg = Registry::load(
        &[Location {
            dir: repo().join("plugins"),
            origin: Origin::Bundled,
        }],
        &Pins::new(),
    );
    let eng = Engine::new(&reg).expect("engine");
    let (abs, rel) = (1e-9, 1e-9);
    let (mut n, mut bad) = (0usize, Vec::new());
    let cases = g["cases"].as_array().unwrap();
    assert!(cases.len() >= 90, "{} files", cases.len());
    for (k, c) in cases.iter().enumerate() {
        let path = dir.join(c["file"].as_str().unwrap());
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let rec = eng.analyse_bytes(&bytes, &name, "asd.labspec4.std", "user");
        let got: BTreeMap<String, V> = flatten(&V::Map(rec))
            .into_iter()
            .filter(|(k, _)| !k.starts_with("dependencies.") && !k.starts_with("input.file"))
            .collect();
        let want = c["expected"].as_object().unwrap();
        for (key, w) in want {
            n += 1;
            match got.get(key) {
                Some(v) if matches_expected(v, w, abs, rel) => {}
                v => bad.push(format!("file #{k}: {key}: got {v:?}, want {w}")),
            }
        }
        for key in got.keys() {
            if !want.contains_key(key) {
                bad.push(format!("file #{k}: unexpected output {key}"));
            }
        }
        // the reader's reflectance, bit for bit as the oracle computes it
        let scan = read_file(&path).unwrap();
        for (i, w) in c["reflectance"].as_array().unwrap().iter().enumerate() {
            if scan.reflectance[i].to_bits() != w.as_f64().unwrap().to_bits() {
                bad.push(format!("file #{k}: reflectance channel {i}"));
                break;
            }
        }
    }
    eprintln!(
        "private instrument goldens: {} files, {n} comparisons, {} failed",
        cases.len(),
        bad.len()
    );
    assert!(bad.is_empty(), "{}", bad[..bad.len().min(40)].join("\n"));
}

#[test]
fn reflectance_matches_the_vendor_exports_and_joins_are_found() {
    let Some((dir, g)) = private("reflectance_matches_the_vendor_exports_and_joins_are_found")
    else {
        return;
    };
    let mut checked = 0;
    for c in g["cases"].as_array().unwrap() {
        let path = dir.join(c["file"].as_str().unwrap());
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let scan = read_file(&path).unwrap();
        let export = match scan.format {
            FileFormat::As7 => {
                assert_eq!(scan.splices_nm, vec![1000.0, 1785.0], "Trek joins");
                assert_eq!(scan.joins_source, "reference_dn");
                &g["exports"]["trek"][&name]
            }
            FileFormat::Legacy => {
                assert_eq!(scan.splices_nm, vec![1000.0, 1800.0], "FieldSpec 3 joins");
                assert!(scan.sample_dn.is_none());
                &g["exports"]["fieldspec3_sco"][&name]
            }
            FileFormat::As8 => panic!("no as8 files in this set"),
        };
        // the FieldSpec 3 export holds the .sco files only; the Trek export every as7 file
        let Some(want) = export.as_array() else {
            continue;
        };
        for (i, w) in want.iter().enumerate() {
            let w = w.as_f64().unwrap();
            assert!(
                (scan.reflectance[i] - w).abs() <= 1e-12 + 1e-12 * w.abs(),
                "channel {i}: {} vs export {w}",
                scan.reflectance[i]
            );
        }
        checked += 1;
    }
    eprintln!("vendor-export reflectance: {checked} files checked");
    assert!(checked >= 60, "{checked}");
}
