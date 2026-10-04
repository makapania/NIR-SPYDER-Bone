//! PRIVATE Phase 2 gate (PLAN section 8): Rust predictions on the real .asd files (read by the Rust reader)
//! equal the frozen Python reference's within 1e-9 + 1e-9 |python|, for every active model and both instrument
//! classes. Runs only when SPYDER_PRIVATE_DATA points at the private `data/` folder; otherwise it prints a
//! visible SKIPPED line and passes. The Python values come from `planning/work/phase2/gen_private_predictions.py`
//! (git-ignored), found at $SPYDER_PRIVATE_PREDICTIONS or `<SPYDER_PRIVATE_DATA>/../planning/work/phase2`.
//! No private file name or value appears in this file or its output.
//!
//!   SPYDER_PRIVATE_DATA="<private data folder>" cargo test -p spyder-core --test private_predict -- --nocapture

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::Value;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};
use spyder_core::plugins::ScanContext;
use spyder_core::predict::predict_scan;
use spyder_core::read::read_file;

#[test]
fn private_predictions_match_python_on_the_real_spectra() {
    let Some(data) = std::env::var_os("SPYDER_PRIVATE_DATA").map(PathBuf::from) else {
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED private_predictions_match_python_on_the_real_spectra: SPYDER_PRIVATE_DATA is not set"
        );
        return;
    };
    let dir = std::env::var_os("SPYDER_PRIVATE_PREDICTIONS")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("..").join("planning").join("work").join("phase2"));
    let text = std::fs::read(dir.join("private_predictions.json")).unwrap_or_else(|e| {
        panic!(
            "SPYDER_PRIVATE_DATA is set but private_predictions.json is missing in {} ({e}); run gen_private_predictions.py",
            dir.display()
        )
    });
    let fx: Value = serde_json::from_slice(&text).unwrap();
    let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let reg = Registry::load(
        &[Location {
            dir: plugins.clone(),
            origin: Origin::Bundled,
        }],
        &Pins::new(),
    );
    let on_disk: BTreeMap<String, String> = reg
        .entries
        .iter()
        .filter(|e| e.path.parent() == Some(plugins.as_path()))
        .map(|e| {
            (
                e.path.file_name().unwrap().to_string_lossy().to_string(),
                e.sha256.clone(),
            )
        })
        .collect();
    for (f, sha) in fx["plugin_files"].as_object().unwrap() {
        assert_eq!(
            on_disk.get(f).map(String::as_str),
            sha.as_str(),
            "a plug-in file changed since the private values were generated: re-run gen_private_predictions.py"
        );
    }
    let (mut n, mut worst, mut files) = (0usize, 0.0f64, 0usize);
    let mut bad = Vec::new();
    for (i, f) in fx["files"].as_array().unwrap().iter().enumerate() {
        let scan = read_file(&data.join(f["path"].as_str().unwrap()))
            .unwrap_or_else(|e| panic!("file #{i}: {e}"));
        files += 1;
        for (cls, want) in f["classes"].as_object().unwrap() {
            let ctx = ScanContext::new(
                scan.splices_nm.clone(),
                cls,
                Some(scan.header.serial.to_string()),
            );
            let got = predict_scan(&reg, &scan.wavelengths_nm, &scan.reflectance, &ctx);
            for r in &got {
                let w = &want[&r.id];
                if w.get("error").is_some() {
                    n += 1;
                    if r.prediction.is_some() {
                        bad.push(format!(
                            "file #{i} {cls} {}: Python failed, Rust predicted",
                            r.id
                        ));
                    }
                    continue;
                }
                if r.transfer.as_ref().map(|t| t.sha256.as_str()) != w["transfer"].as_str() {
                    bad.push(format!("file #{i} {cls} {}: transfer differs", r.id));
                }
                let Some(p) = &r.prediction else {
                    bad.push(format!(
                        "file #{i} {cls} {}: {}",
                        r.id,
                        r.assessment.describe()
                    ));
                    continue;
                };
                for (key, wv) in w.as_object().unwrap() {
                    if key == "transfer" {
                        continue;
                    }
                    let wv = wv.as_f64().unwrap();
                    n += 1;
                    match p.output(key) {
                        Some(g) if g.is_finite() && (g - wv).abs() <= 1e-9 + 1e-9 * wv.abs() => {
                            worst = worst.max((g - wv).abs() / (1e-9 + 1e-9 * wv.abs()))
                        }
                        // no values in the message: they derive from private spectra
                        _ => bad.push(format!("file #{i} {cls} {} {key}: outside 1e-9", r.id)),
                    }
                }
            }
        }
    }
    let _ = writeln!(
        std::io::stderr(),
        "private prediction parity: {files} files x 2 classes, {n} comparisons, worst |diff| / tolerance {worst:.3e}"
    );
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad[..bad.len().min(20)].join("\n")
    );
    assert!(files > 0);
}
