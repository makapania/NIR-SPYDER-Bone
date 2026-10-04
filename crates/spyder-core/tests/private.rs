//! PRIVATE parity tests (PLAN section 8 Phase 1 gate). Run only when SPYDER_PRIVATE_DATA points at the
//! private `data/` folder; otherwise each test prints a visible SKIPPED line and passes.
//!
//! Goldens come from `planning/work/phase1/gen_private_goldens.py` (git-ignored), found at
//! $SPYDER_PRIVATE_GOLDENS or `<SPYDER_PRIVATE_DATA>/../planning/work/phase1`. No private file name or
//! value appears in this file.
//!
//!   SPYDER_PRIVATE_DATA="<private data folder>" cargo test -p spyder-core --test private -- --nocapture

#![allow(clippy::field_reassign_with_default, clippy::needless_range_loop)] // test fixtures read better this way

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::Value;
use spyder_core::n2::n2;
use spyder_core::qc::{acquisition_checks, Outcome};
use spyder_core::read::read_file;

struct Private {
    data: PathBuf,
    goldens_dir: PathBuf,
    goldens: Value,
}

fn private(test: &str) -> Option<Private> {
    let Some(data) = std::env::var_os("SPYDER_PRIVATE_DATA").map(PathBuf::from) else {
        // written to stderr directly (not via eprintln!), so libtest's capture cannot hide the skip
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED {test}: SPYDER_PRIVATE_DATA is not set (private parity tests need the private data)"
        );
        return None;
    };
    let goldens_dir = std::env::var_os("SPYDER_PRIVATE_GOLDENS")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("..").join("planning").join("work").join("phase1"));
    let text = std::fs::read_to_string(goldens_dir.join("private_goldens.json")).unwrap_or_else(|e| {
        panic!(
            "SPYDER_PRIVATE_DATA is set but the private goldens are missing in {} ({e}); run gen_private_goldens.py",
            goldens_dir.display()
        )
    });
    Some(Private {
        data,
        goldens_dir,
        goldens: serde_json::from_str(&text).expect("private goldens JSON"),
    })
}

fn files(p: &Private) -> &Vec<Value> {
    p.goldens["files"].as_array().unwrap()
}

#[test]
fn private_reflectance_bitwise_equal_to_python_reader() {
    let Some(p) = private("private_reflectance_bitwise_equal_to_python_reader") else {
        return;
    };
    let mut n = 0;
    for f in files(&p) {
        let rel = f["path"].as_str().unwrap();
        let scan = read_file(&p.data.join(rel)).unwrap_or_else(|e| panic!("file #{n}: {e}"));
        let want = std::fs::read(p.goldens_dir.join(f["r_file"].as_str().unwrap())).unwrap();
        let got: Vec<u8> = scan
            .reflectance
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        assert!(
            got == want,
            "file #{n}: reflectance differs from the Python reader"
        );
        let h = &f["header"];
        let sh = &scan.header;
        let pairs: [(&str, f64); 16] = [
            ("instrument_num", sh.serial.into()),
            ("it_ms", sh.integration_time_ms.into()),
            ("swir1_gain", sh.swir1_gain.into()),
            ("swir2_gain", sh.swir2_gain.into()),
            ("swir1_offset", sh.swir1_offset.into()),
            ("swir2_offset", sh.swir2_offset.into()),
            ("dc_count", sh.dark_averages.into()),
            ("ref_count", sh.reference_averages.into()),
            ("sample_count", sh.sample_averages.into()),
            ("channels", sh.channels.into()),
            ("calibration_series", sh.calibration_series.into()),
            ("program_version", sh.program_version.into()),
            ("file_version", sh.file_version.into()),
            ("splice1", sh.splice1_nm.into()),
            ("splice2", sh.splice2_nm.into()),
            ("ch1_wavel", sh.first_wavelength_nm.into()),
        ];
        for (k, v) in pairs {
            assert_eq!(h[k].as_f64().unwrap(), v, "file #{n}: header {k}");
        }
        n += 1;
    }
    let expected = p.goldens["n_files"].as_u64().unwrap() as usize;
    assert_eq!(n, expected);
    // every .asd on disk is in the goldens (none silently skipped)
    let mut on_disk = 0;
    let mut stack = vec![p.data.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case("asd"))
            {
                on_disk += 1;
            }
        }
    }
    assert_eq!(on_disk, n, "files on disk vs goldens");
    println!("private: reflectance bitwise equal and headers equal on {n} files");
}

#[test]
fn private_reflectance_within_1e6_of_indico_csv() {
    let Some(p) = private("private_reflectance_within_1e6_of_indico_csv") else {
        return;
    };
    let rel = p.goldens["indico_csv"].as_str().unwrap();
    let csv_path = p.data.join(rel);
    let text = std::fs::read_to_string(&csv_path).unwrap();
    let dir = csv_path.parent().unwrap();
    let mut lines = text.lines();
    let header: Vec<f64> = lines
        .next()
        .unwrap()
        .trim_start_matches('\u{feff}')
        .split(',')
        .skip(2)
        .map(|s| s.trim().parse().unwrap())
        .collect();
    assert_eq!(header.len(), 2151);
    let (mut rows, mut worst_rel, mut worst_abs) = (0, 0.0f64, 0.0f64);
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let mut cells = line.split(',');
        let name = cells.next().unwrap().trim();
        let _ = cells.next();
        let vals: Vec<f64> = cells.map(|s| s.trim().parse().unwrap()).collect();
        let scan = read_file(&dir.join(name)).unwrap_or_else(|e| panic!("csv row {rows}: {e}"));
        assert_eq!(vals.len(), scan.reflectance.len());
        assert_eq!(header, scan.wavelengths_nm);
        for (a, b) in scan.reflectance.iter().zip(&vals) {
            let d = (a - b).abs();
            worst_abs = worst_abs.max(d);
            if *b != 0.0 {
                worst_rel = worst_rel.max(d / b.abs());
            } else {
                assert!(d <= 1e-6);
            }
        }
        rows += 1;
    }
    println!("private: Indico CSV {rows} rows, max rel {worst_rel:e}, max abs {worst_abs:e}");
    assert!(rows > 100, "{rows} rows");
    assert!(
        worst_rel <= 1e-6,
        "max relative difference {worst_rel:e} > 1e-6"
    );
}

#[test]
fn private_n2_matches_n2_py_and_goldens() {
    let Some(p) = private("private_n2_matches_n2_py_and_goldens") else {
        return;
    };
    let (mut n, mut bitwise, mut worst) = (0, 0, 0.0f64);
    for f in files(&p) {
        let scan = read_file(&p.data.join(f["path"].as_str().unwrap())).unwrap();
        for (key, want) in f["n2"].as_object().unwrap() {
            let (lo, hi) = key.split_once('_').unwrap();
            let got = n2(
                &scan.reflectance,
                &scan.wavelengths_nm,
                lo.parse().unwrap(),
                hi.parse().unwrap(),
            );
            match want.as_f64() {
                None => assert_eq!(got, None),
                Some(w) => {
                    let g = got.unwrap();
                    let rel = (g - w).abs() / w.abs().max(1e-12);
                    worst = worst.max(rel);
                    assert!(rel <= 1e-9, "file #{n} window {key}: {g} vs {w}");
                    if g.to_bits() == w.to_bits() {
                        bitwise += 1;
                    }
                }
            }
        }
        n += 1;
    }
    for g in p.goldens["n2_goldens"].as_array().unwrap() {
        let scan = read_file(&p.data.join(g["path"].as_str().unwrap())).unwrap();
        let w = &g["window"];
        let v = n2(
            &scan.reflectance,
            &scan.wavelengths_nm,
            w[0].as_f64().unwrap(),
            w[1].as_f64().unwrap(),
        )
        .unwrap();
        let want = g["value"].as_f64().unwrap();
        assert!(
            (v - want).abs() <= g["tol"].as_f64().unwrap(),
            "N2 golden: {v} vs {want}"
        );
        println!("private: N2 golden {want} -> {v:.4}");
    }
    println!("private: N2 on {n} files x 4 windows, {bitwise} bitwise equal, worst rel {worst:e}");
}

#[test]
fn private_acquisition_checks_run_on_every_file() {
    let Some(p) = private("private_acquisition_checks_run_on_every_file") else {
        return;
    };
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut warnings = 0;
    for f in files(&p) {
        let read = read_file(&p.data.join(f["path"].as_str().unwrap()));
        let scan = read.as_ref().unwrap();
        *kinds.entry(format!("{:?}", scan.kind)).or_insert(0) += 1;
        warnings += scan.warnings.len();
        let n2 = spyder_core::n2::N2Set::compute(&scan.reflectance, &scan.wavelengths_nm);
        for c in acquisition_checks(&read, Some(&n2)) {
            let key = match c.outcome {
                Some(o) => format!("{} {:?}", c.id, o),
                None => format!("{} {}", c.id, c.assessment.describe()),
            };
            *tally.entry(key).or_insert(0) += 1;
            if c.id == "B1" {
                assert_eq!(c.outcome, Some(Outcome::Pass));
            }
        }
    }
    println!("private: scan kinds {kinds:?}; reader warnings {warnings}");
    for (k, v) in &tally {
        println!("private: {k}: {v}");
    }
}
