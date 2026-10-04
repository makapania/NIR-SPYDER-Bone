//! PLAN Phase 2 gate: Cal-100 reproduces Ryder et al. 2026 Table 5 (2045 nm model, Calibration 100 /
//! Validation 40) through the app engine: the test-only Calibration-100 model file loads (its goldens pass),
//! then predicts the 140 public reference bones (mmc2) and the Table 5 statistics match the published values to
//! the published decimals. The shipped n = 140 model reproduces its own statistics too (R2 0.8862, RMSEC 1.5952;
//! LESSONS 13: these belong to n = 140, not to Table 5).
//!
//! Fixture: tests/fixtures/cal100 (public; tests/gen/gen_cal100_fixture.py).

use std::path::{Path, PathBuf};

use serde_json::Value;
use spyder_core::model::Model;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry, State};
use spyder_core::plugins::{ScanContext, CLASS_STD};
use spyder_core::preprocess::Spectrum;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Bone {
    set: String,
    y: f64,
    wl: Vec<f64>,
    r: Vec<f64>,
}

fn bones() -> (Vec<Bone>, Value) {
    let v: Value = serde_json::from_slice(
        &std::fs::read(manifest().join("tests/fixtures/cal100/mmc2_2045_window.json")).unwrap(),
    )
    .unwrap();
    let start = v["wl_start_nm"].as_f64().unwrap();
    let step = v["wl_step_nm"].as_f64().unwrap();
    let out = v["bones"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            let r: Vec<f64> = b["reflectance"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            Bone {
                set: b["set"].as_str().unwrap().to_string(),
                y: b["collagen_pct"].as_f64().unwrap(),
                wl: (0..r.len()).map(|i| start + step * i as f64).collect(),
                r,
            }
        })
        .collect();
    (out, v["table5_published"].clone())
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Squared Pearson correlation (Unscrambler R-square, as Ryder et al.).
fn r2(a: &[f64], b: &[f64]) -> f64 {
    let (ma, mb) = (mean(a), mean(b));
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    sab * sab / (saa * sbb)
}

fn rmse(a: &[f64], b: &[f64]) -> f64 {
    (a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>() / a.len() as f64).sqrt()
}

fn round(x: f64, d: i32) -> f64 {
    let s = 10f64.powi(d);
    (x * s).round() / s
}

fn predict(m: &Model, b: &Bone) -> f64 {
    let ctx = ScanContext::new(vec![1000.0, 1800.0], CLASS_STD, None);
    m.predict(&Spectrum::new(b.wl.clone(), b.r.clone()), &ctx)
        .expect("prediction")
        .value
}

fn load_model(dir: &Path, file: &str, tag: &str) -> Registry {
    let d = std::env::temp_dir().join(format!("spyder-cal100-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(dir.join(file), d.join(file)).unwrap();
    let plugins = manifest().join("../../plugins");
    std::fs::copy(
        plugins.join("golden_spectra_public_v2.json"),
        d.join("golden_spectra_public_v2.json"),
    )
    .unwrap();
    let reg = Registry::load(
        &[Location {
            dir: d,
            origin: Origin::User,
        }],
        &Pins::new(),
    );
    let e = reg
        .entries
        .iter()
        .find(|e| e.path.file_name().unwrap() == file)
        .unwrap();
    assert_eq!(e.state, State::Loaded, "{:?}", e.error);
    reg
}

#[test]
fn cal100_reproduces_table5() {
    let reg = load_model(
        &manifest().join("tests/fixtures/cal100"),
        "test_ryder2045_cal100.spyder-model.json",
        "t5",
    );
    // a demo-status file loads but is never selected automatically; take it from its entry
    let m = reg
        .entries
        .iter()
        .find_map(|e| match &e.item {
            Some(spyder_core::plugins::registry::Item::Model(m)) => Some(m.as_ref().clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        reg.model(&m.header.id).is_none(),
        "a demo file must not be auto-selected"
    );
    let (bones, t5) = bones();
    let (mut yc, mut pc, mut yv, mut pv) = (vec![], vec![], vec![], vec![]);
    for b in &bones {
        let p = predict(&m, b);
        match b.set.as_str() {
            "Calibration" => {
                yc.push(b.y);
                pc.push(p)
            }
            "Validation" => {
                yv.push(b.y);
                pv.push(p)
            }
            s => panic!("unexpected set {s}"),
        }
    }
    assert_eq!((yc.len(), yv.len()), (100, 40));
    let dec = &t5["decimals"];
    let check = |name: &str, got: f64| {
        let d = dec[name].as_i64().unwrap() as i32;
        let want = t5[name].as_f64().unwrap();
        assert!(
            (round(got, d) - want).abs() < 1e-12,
            "{name}: {got} rounds to {} (published {want})",
            round(got, d)
        );
    };
    check("R2C", r2(&yc, &pc));
    check("RMSEC", rmse(&yc, &pc));
    check("VR2_val40", r2(&yv, &pv));
    check("RMSEV_val40", rmse(&yv, &pv));
    let below = yv
        .iter()
        .zip(&pv)
        .filter(|(y, p)| **y < 3.0 && **p < 3.0)
        .count();
    let n_below = yv.iter().filter(|y| **y < 3.0).count();
    let above = yv
        .iter()
        .zip(&pv)
        .filter(|(y, p)| **y >= 3.0 && **p >= 3.0)
        .count();
    let n_above = yv.iter().filter(|y| **y >= 3.0).count();
    assert_eq!(
        format!("{below}/{n_below}"),
        t5["val40_below3_correct"].as_str().unwrap()
    );
    assert_eq!(
        format!("{above}/{n_above}"),
        t5["val40_above3_correct"].as_str().unwrap()
    );
}

#[test]
fn shipped_n140_model_reproduces_its_own_statistics() {
    let plugins = manifest().join("../../plugins");
    let reg = load_model(
        &plugins,
        "collagen_2045_ryder2026_n140.spyder-model.json",
        "n140",
    );
    let m = reg
        .model("collagen.ryder2026.2045")
        .expect("Ryder 2045 selected");
    let (bones, _) = bones();
    let y: Vec<f64> = bones.iter().map(|b| b.y).collect();
    let p: Vec<f64> = bones.iter().map(|b| predict(m, b)).collect();
    assert_eq!(y.len(), 140);
    // LESSONS 13 / the file's statistics block: R2C 0.8862, RMSEC 1.5952
    assert!(
        (round(r2(&y, &p), 4) - 0.8862).abs() < 1e-12,
        "{}",
        r2(&y, &p)
    );
    assert!(
        (round(rmse(&y, &p), 4) - 1.5952).abs() < 1e-12,
        "{}",
        rmse(&y, &p)
    );
}
