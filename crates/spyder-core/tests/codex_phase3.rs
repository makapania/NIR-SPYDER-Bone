//! Regressions for the Codex review of the Phase 3 commit (d39e88f). The same cases run against the Python
//! oracle in `reference/tests/test_oracle_review3.py`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use spyder_core::pipeline::checks::{evidence, signs, zooms};
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::{Context, Engine};
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(dir: &Path) -> Registry {
    Registry::load(
        &[Location {
            dir: dir.to_path_buf(),
            origin: Origin::User,
        }],
        &Pins::new(),
    )
}

fn wl() -> Vec<f64> {
    (0..2151).map(|i| 350.0 + i as f64).collect()
}

/// A public golden spectrum (an mmc2 reference bone) by name.
fn golden(name: &str) -> Vec<f64> {
    let g: Value = serde_json::from_slice(
        &std::fs::read(repo().join("plugins/golden_spectra_public_v2.json")).unwrap(),
    )
    .unwrap();
    g["spectra"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap()["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn ctx(class: &str) -> Context {
    Context {
        instrument_class: class.into(),
        class_source: "user".into(),
        serial: None,
        splices_nm: vec![1000.0, 1800.0],
    }
}

fn at<'a>(o: &'a Obj, path: &[&str]) -> &'a V {
    let mut cur = o;
    for (i, k) in path.iter().enumerate() {
        let v = cur.get(k).unwrap_or_else(|| panic!("no {k} in {path:?}"));
        if i + 1 == path.len() {
            return v;
        }
        cur = v.as_obj().unwrap();
    }
    unreachable!()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.path().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// HIGH 1: each profile reads its OWN verdict-input model (here: isotopes switched to Ryder 2045).
#[test]
fn each_profile_uses_its_own_verdict_model() {
    let d = std::env::temp_dir().join(format!("spyder-codex3-prof-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    copy_dir(&repo().join("plugins"), &d);
    let f = d.join("profiles/profile.isotopes.json");
    let mut p: Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
    p["parameters"]["models"] = json!([{"key": "ryder2045", "model_id": "collagen.ryder2026.2045",
        "role": "verdict_input", "components": [],
        "shown_for_classes": ["asd.labspec4.std", "asd.labspec4.hires"]}]);
    std::fs::write(&f, serde_json::to_vec_pretty(&p).unwrap()).unwrap();
    let reg = load(&d);
    let eng = Engine::new(&reg).expect("engine");
    let rec = eng.analyse_spectrum(
        &wl(),
        &golden("mmc2_Reference_Sample_79"),
        &ctx("asd.labspec4.std"),
    );
    let cons3 = at(&rec, &["models", "cons3", "value"]).as_f64().unwrap();
    let ryder = at(&rec, &["models", "ryder2045", "value"])
        .as_f64()
        .unwrap();
    assert!(
        (cons3 - ryder).abs() > 1e-6,
        "the test needs two different readings"
    );
    assert_eq!(
        at(&rec, &["radiocarbon", "sort_value"]).as_f64(),
        Some(cons3)
    );
    assert_eq!(at(&rec, &["isotopes", "sort_value"]).as_f64(), Some(ryder));
    let _ = std::fs::remove_dir_all(&d);
}

fn shipped() -> Registry {
    load(&repo().join("plugins"))
}

/// MEDIUM 2: an empty or short reflectance array is unsupported input, never a panic.
#[test]
fn empty_or_short_spectra_are_unsupported_not_a_panic() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let w = wl();
    let x = golden("mmc2_Reference_Sample_79");
    for (wl_in, r_in) in [
        (vec![], vec![]),
        (w.clone(), x[..100].to_vec()),
        (w[..100].to_vec(), x[..100].to_vec()),
        (w.clone(), vec![]),
    ] {
        let rec = eng.analyse_spectrum(&wl_in, &r_in, &ctx("asd.labspec4.std"));
        assert_eq!(
            at(&rec, &["radiocarbon", "verdict"]).as_str(),
            Some("Unsupported")
        );
        assert_eq!(
            at(&rec, &["checks", "B1", "reason"]).as_str(),
            Some("unsupported: wavelength grid")
        );
    }
}

/// MEDIUM 3: a non-finite heat reading (NaN reflectance near 979 nm, outside B3's range) is "not assessed",
/// never "not burnt"; plaster, wax and ester likewise.
#[test]
fn non_finite_heat_reading_is_not_assessed() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let mut x = golden("mmc2_Reference_Sample_79");
    for v in &mut x[620..640] {
        *v = f64::NAN; // 970-989 nm
    }
    let rec = eng.analyse_spectrum(&wl(), &x, &ctx("asd.labspec4.std"));
    assert_eq!(at(&rec, &["checks", "B3", "outcome"]).as_str(), Some("ok"));
    assert_eq!(
        at(&rec, &["signs", "burnt", "status"]).as_str(),
        Some("not assessed: non-finite reading")
    );
    assert!(at(&rec, &["signs", "burnt", "fired"]).is_null());
    assert!(at(&rec, &["signs", "burnt", "calcined"]).is_null());
    // the signs evaluator on an all-NaN stream: every sign not assessed
    let checks: BTreeMap<String, _> = reg
        .checks()
        .into_iter()
        .map(|c| (c.check.clone(), &c.params))
        .collect();
    let s = signs(
        &wl(),
        &vec![f64::NAN; 2151],
        checks["signs"],
        checks["heat"],
        &Default::default(),
    );
    for n in ["plaster", "wax", "ester", "burnt"] {
        let o = s.get(n).and_then(V::as_obj).unwrap();
        assert!(
            o.get("status")
                .and_then(V::as_str)
                .unwrap()
                .starts_with("not assessed"),
            "{n}"
        );
        assert!(o.get("fired").unwrap().is_null(), "{n}");
    }
    // evidence and ZooMS on non-finite readings: not assessed (never "none" / "Unlikely")
    let e = vec![f64::NAN; 2151];
    let (ev, level, s_val) = evidence(
        &wl(),
        &e,
        checks["evidence_levels"],
        reg.bands().unwrap(),
        &BTreeMap::new(),
    );
    assert!(ev
        .get("status")
        .and_then(V::as_str)
        .unwrap()
        .starts_with("not assessed"));
    assert!(level.is_empty() && s_val.is_none());
    let (zo, zv, _, _) = zooms(
        &wl(),
        &e,
        checks["zooms_patterns"],
        reg.bands().unwrap(),
        &BTreeMap::new(),
    );
    assert!(zo
        .get("status")
        .and_then(V::as_str)
        .unwrap()
        .starts_with("not assessed"));
    assert_eq!(zv, "Can't tell");
}

/// MEDIUM 4: a clear but noisy N-H 2044 band (u = 2, sd_u = 0.33) is readable (Step 7: can't tell only if
/// SD > 0.25 AND u < 1.5), so it blocks "none".
#[test]
fn clear_noisy_nh2044_is_readable() {
    let reg = shipped();
    let checks: BTreeMap<String, _> = reg
        .checks()
        .into_iter()
        .map(|c| (c.check.clone(), &c.params))
        .collect();
    let bands = reg.bands().unwrap();
    let w = wl();
    let mut e = vec![0.0; 2151];
    for v in &mut e[(2044 - 2 - 350)..=(2044 + 2 - 350)] {
        *v = 3.2; // u = 3.2 / 1.6 = 2
    }
    let mut sd = BTreeMap::new();
    for b in ["CH1728", "CH1689", "CH2262", "AM2175", "CH2284"] {
        sd.insert(b.to_string(), 0.01);
    }
    sd.insert("NH2044".to_string(), 0.528); // sd_u = 0.33 > 0.25
    let (ev, level, _) = evidence(&w, &e, checks["evidence_levels"], bands, &sd);
    let st = ev
        .get("bands")
        .and_then(V::as_obj)
        .and_then(|b| b.get("NH2044"))
        .and_then(V::as_obj)
        .and_then(|b| b.get("state"))
        .and_then(V::as_str)
        .unwrap();
    assert_eq!(st, "clear");
    assert_eq!(level, "trace", "a resolved N-H 2044 band rules out 'none'");
}

/// LOW 5: acquisition failures carry the profile's sort key (Rescan before "Doesn't look like bone"?
/// whatever the profile says: its own position), and rejected files sort last.
#[test]
fn rescan_and_rejected_records_have_sort_keys() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let order = &eng.profile("radiocarbon").unwrap().sort.verdict_order;
    let panel: Vec<f64> = (0..2151)
        .map(|i| 1.0 + 0.002 * ((i as f64) * 0.37).sin())
        .collect();
    let rec = eng.analyse_spectrum(&wl(), &panel, &ctx("asd.labspec4.std"));
    assert_eq!(
        at(&rec, &["radiocarbon", "verdict"]).as_str(),
        Some("Rescan")
    );
    let g = at(&rec, &["radiocarbon", "sort_group"]).as_f64().unwrap() as usize;
    assert_eq!(order[g], "Rescan");
    assert!(at(&rec, &["radiocarbon", "sort_value"]).is_null());
    let mut c = ctx("asd.labspec4.std");
    c.splices_nm = vec![1000.0, 1830.0];
    let rec = eng.analyse_spectrum(&wl(), &golden("mmc2_Reference_Sample_79"), &c);
    assert_eq!(
        at(&rec, &["zooms", "verdict"]).as_str(),
        Some("Unsupported")
    );
    assert_eq!(
        at(&rec, &["zooms", "sort_group"]).as_f64(),
        Some(eng.profile("zooms").unwrap().sort.verdict_order.len() as f64)
    );
}
