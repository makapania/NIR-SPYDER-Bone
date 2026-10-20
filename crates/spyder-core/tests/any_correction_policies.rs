//! DECISIONS 137 Phase 2 (part 2): the noise policies follow the correction applied, never a transfer file's SHA or
//! a class name. Per-sign B6b gates, the DECISIONS 107 fallbacks, long-wave and soft-tier eligibility through a
//! FieldSpec 3-like class file and a lab's own high-res unit file; v0.2 reproduces its shipped cuts; the uncalibrated
//! note for another instrument's scan with no correction; heat on the scan as measured when a correction did not fit
//! its visible levels; per-reader offsets; a correction's own second-opinion gap. The same cases run against the
//! oracle in reference/tests/test_any_correction_policies.py. Public spectra only.
//!
//! Independent expectations: the corrections here have no blur, so a statistic term's noise gain through them is the
//! detector's absorbance gain x its gain as measured, and the as-measured gains are g2's research values (planning
//! g2_gates/g01_gains.py: kernels by finite differences through the oracle kernels), not the engine's own routine.
//! The cuts are read from the shipped check files.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use spyder_core::pipeline::checks::heat;
use spyder_core::pipeline::export::csv_row;
use spyder_core::pipeline::kernels::derivative;
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::{Context, Engine};
use spyder_core::plugins::checks::CheckParams;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry, State};
use spyder_core::plugins::{CLASS_HIRES, CLASS_STD};

const FS3: &str = "asd.fieldspec3";
/// The bundled high-res correction (the tests that vary it compare with the shipped plug-ins).
const SHIPPED: &str = "hires_to_std_v0_3.spyder-transfer.json";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn plugins() -> PathBuf {
    repo().join("plugins")
}

fn wl() -> Vec<f64> {
    (0..2151).map(|i| 350.0 + i as f64).collect()
}

fn read(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn golden(name: &str) -> Vec<f64> {
    let g = read(&plugins().join("golden_spectra_public_v2.json"));
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

/// White noise in absorbance: SD `sd` above 1800 nm, `ratio` x `sd` below; the same LCG as the Python test.
fn noisy(x: &[f64], sd: f64, seed: u32, ratio: f64) -> Vec<f64> {
    let mut s = seed;
    x.iter()
        .enumerate()
        .map(|(i, v)| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let u = f64::from(s) / 4294967296.0 - 0.5;
            let a = if 350.0 + i as f64 > 1800.0 {
                sd
            } else {
                ratio * sd
            };
            v * 10f64.powf(-a * 3.4641016151377544 * u)
        })
        .collect()
}

fn ctx(class: &str, serial: Option<u64>) -> Context {
    Context {
        instrument_class: class.into(),
        class_source: "user".into(),
        serial,
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

fn f(o: &Obj, path: &[&str]) -> f64 {
    at(o, path).as_f64().unwrap()
}

fn s<'a>(o: &'a Obj, path: &[&str]) -> &'a str {
    at(o, path).as_str().unwrap()
}

fn list(o: &Obj, path: &[&str]) -> Vec<String> {
    match at(o, path) {
        V::List(l) => l.iter().filter_map(V::as_str).map(str::to_string).collect(),
        _ => panic!(),
    }
}

fn note_keys(rec: &Obj) -> Vec<String> {
    match at(rec, &["instrument", "notes"]) {
        V::List(l) => l
            .iter()
            .filter_map(|n| n.as_obj()?.get("key")?.as_str().map(str::to_string))
            .collect(),
        _ => panic!(),
    }
}

/// A test correction: the shipped v0.2 file with this id, version 1.0.0, classes, operators and extra fields, its
/// golden cases computed by the engine (as the tests of part 1 do).
fn correction(id: &str, source: &str, ops: Value, serial: Option<&str>, extra: Value) -> Value {
    let mut t = read(&plugins().join("hires_to_std_v0_2.spyder-transfer.json"));
    t["id"] = json!(id);
    t["version"] = json!("1.0.0");
    t["source_class"] = json!(source);
    t["target_class"] = json!(CLASS_STD);
    t["operator"] = ops.clone();
    if let Some(sr) = serial {
        t["source_serial"] = json!(sr);
    }
    for (k, v) in extra.as_object().unwrap() {
        if k == "provenance" {
            for (pk, pv) in v.as_object().unwrap() {
                t["provenance"][pk] = pv.clone();
            }
        } else {
            t[k] = v.clone();
        }
    }
    let chain = spyder_core::preprocess::parse_chain(&ops).unwrap();
    let g = read(&plugins().join("golden_spectra_public_v2.json"));
    let mut cases = Vec::new();
    for name in ["pair10_hires", "pair22_hires", "synthetic_hireslike_RS79"] {
        let sp = g["spectra"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == name)
            .unwrap();
        let out = spyder_core::preprocess::run_chain(
            &chain,
            &spyder_core::preprocess::Spectrum::new(wl(), golden(name)),
            &[1000.0, 1800.0],
        )
        .unwrap();
        let mut at = serde_json::Map::new();
        for (w, v) in out.wl.iter().zip(&out.x).step_by(97) {
            at.insert(format!("{}", *w as i64), json!(v));
        }
        cases.push(json!({"name": name, "spectrum_sha256": sp["sha256"],
            "scan_context": {"splices_nm": [1000.0, 1800.0], "instrument_class": CLASS_HIRES},
            "expected": {"transferred_at": at}}));
    }
    t["golden"]["cases"] = json!(cases);
    t
}

/// The shipped plug-ins plus a user folder holding `t`; `loads`: whether the file must load.
fn with_file(tag: &str, t: &Value, loads: bool) -> Registry {
    let d = std::env::temp_dir().join(format!("spyder-acp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(
        plugins().join("golden_spectra_public_v2.json"),
        d.join("golden_spectra_public_v2.json"),
    )
    .unwrap();
    std::fs::write(
        d.join("test.spyder-transfer.json"),
        serde_json::to_vec_pretty(t).unwrap(),
    )
    .unwrap();
    let reg = Registry::load(
        &[
            Location {
                dir: plugins(),
                origin: Origin::Bundled,
            },
            Location {
                dir: d,
                origin: Origin::User,
            },
        ],
        &Pins::new(),
    );
    let e = reg
        .entries
        .iter()
        .find(|e| e.path.ends_with("test.spyder-transfer.json"))
        .unwrap();
    assert_eq!(e.state == State::Loaded, loads, "{:?}", e.error);
    reg
}

fn shipped() -> Registry {
    Registry::load(
        &[Location {
            dir: plugins(),
            origin: Origin::Bundled,
        }],
        &Pins::new(),
    )
}

/// g2's research gains as measured (joins 1000/1800).
const PL1945: f64 = 0.00046797729678216103;
const WAX1766: f64 = 0.07928105063409477;
const ESTER: f64 = 0.07928105063409477;
const C1_SWIR2: f64 = 0.030949583251220675;
const PL1446: f64 = 0.0007515802359379831;
const C1_SWIR1: f64 = 0.04906751646104421;
const TAIL: f64 = 0.017728969621838987;

struct Cuts {
    sd_above: [(String, f64); 4],
    fb_plaster: f64,
    fb_c1: f64,
    b9: f64,
    c1_tail: f64,
    soft: f64,
}

fn cuts() -> Cuts {
    let a = read(&plugins().join("checks/check.acquisition.json"));
    let g = &a["parameters"]["B6b_signs_gate"];
    let c = |s: &str| g["corrected"][s]["sd_above"].as_f64().unwrap();
    let lw = &read(&plugins().join("checks/check.longwave.json"))["parameters"];
    let soft =
        &read(&plugins().join("checks/check.signs.json"))["parameters"]["soft_tier"]["eligibility"];
    Cuts {
        sd_above: [
            ("plaster".into(), c("plaster")),
            ("wax".into(), c("wax")),
            ("ester".into(), c("ester")),
            ("C1".into(), c("C1")),
        ],
        fb_plaster: g["fallbacks"]["plaster"]["sd_at_most"].as_f64().unwrap(),
        fb_c1: g["fallbacks"]["C1"]["sd_at_most"].as_f64().unwrap(),
        b9: lw["b9_sd_above"].as_f64().unwrap(),
        c1_tail: lw["c1_sd_above"].as_f64().unwrap(),
        soft: soft["otherwise_sd_at_most"].as_f64().unwrap(),
    }
}

/// Checks one record against the SD rule for a blur-free correction with these per-detector gains. Returns (gated,
/// reduced, tail, soft off) counts.
fn check_record(rec: &Obj, gains: [f64; 3], tag: &str) -> (usize, usize, usize, usize) {
    let c = cuts();
    let n2 = |w: &str| f(rec, &["n2", w]);
    let (g1, g2) = (gains[1].abs(), gains[2].abs());
    let sd = |sign: &str| match sign {
        "plaster" => g2 * PL1945 * n2("2000-2100"),
        "wax" => g1 * WAX1766 * n2("1500-1600"),
        "ester" => g2 * ESTER * n2("2220-2245"),
        _ => g2 * C1_SWIR2 * n2("2000-2100"),
    };
    let gated_list = list(rec, &["checks", "B6b", "signs_gated"]);
    let reduced_list = list(rec, &["checks", "B6b", "signs_reduced"]);
    let (mut ng, mut nr) = (0, 0);
    for (sign, cut) in &c.sd_above {
        let gated = sd(sign) > *cut;
        let red = gated
            && match sign.as_str() {
                "plaster" => g1 * PL1446 * n2("1500-1600") <= c.fb_plaster,
                "C1" => g1 * C1_SWIR1 * n2("1500-1600") <= c.fb_c1,
                _ => false,
            };
        let st = if sign == "C1" {
            s(rec, &["C1", "status"])
        } else {
            s(rec, &["signs", sign, "status"])
        };
        if red {
            nr += 1;
            assert!(reduced_list.contains(sign), "{tag} {sign}");
            assert!(
                st.starts_with("assessed: reduced") || st.starts_with("skipped"),
                "{tag} {sign} {st}"
            );
        } else if gated {
            assert!(gated_list.contains(sign), "{tag} {sign}");
            assert!(st.starts_with("gated: noise in"), "{tag} {sign} {st}");
        } else {
            assert!(
                !gated_list.contains(sign) && !reduced_list.contains(sign),
                "{tag} {sign}"
            );
        }
        ng += usize::from(gated);
    }
    assert!(!s(rec, &["signs", "burnt", "status"]).starts_with("gated"));
    let tail = g2 * TAIL * n2("2300-2400");
    assert_eq!(s(rec, &["checks", "longwave", "status"]), "assessed");
    assert_eq!(
        at(rec, &["checks", "longwave", "tail_unreliable"]).as_bool(),
        Some(tail > c.b9),
        "{tag}"
    );
    assert_eq!(
        at(rec, &["checks", "longwave", "tail_unreliable_c1"]).as_bool(),
        Some(tail > c.c1_tail),
        "{tag}"
    );
    let soft = g2 * C1_SWIR2 * n2("2000-2100") <= c.soft;
    assert_eq!(
        at(rec, &["signs", "soft", "eligible"]).as_bool(),
        Some(soft),
        "{tag}"
    );
    (ng, nr, usize::from(tail > c.b9), usize::from(!soft))
}

const NOISE: [f64; 10] = [
    0.0, 1e-4, 2e-4, 3e-4, 4.5e-4, 6e-4, 1e-3, 1.6e-3, 2.5e-3, 4e-3,
];

#[test]
fn a_fieldspec3_like_class_file_gets_the_sd_rule_gates() {
    let gains = [1.0, 1.25, 1.8];
    let t = correction(
        "transfer.test.fs3like",
        FS3,
        json!([{"op": "absorbance_affine", "gain": gains, "offset": 0.0, "clip_min": 1e-6}]),
        None,
        json!({}),
    );
    let reg = with_file("fs3", &t, true);
    let eng = Engine::new(&reg).unwrap();
    let x0 = golden("mmc2_Reference_Sample_79");
    let mut seen = [0usize; 5];
    for (k, sd) in NOISE.iter().enumerate() {
        let rec =
            eng.analyse_spectrum(&wl(), &noisy(&x0, *sd, 7 + k as u32, 0.15), &ctx(FS3, None));
        assert_eq!(
            s(&rec, &["stream", "transfer"]),
            "transfer.test.fs3like@1.0.0"
        );
        assert!(!note_keys(&rec).contains(&"instrument_uncalibrated".to_string()));
        let (g, r, tl, so) = check_record(&rec, gains, &format!("fs3like {sd}"));
        seen[0] += g;
        seen[1] += r;
        seen[2] += tl;
        seen[3] += so;
        seen[4] += usize::from(g == 0);
        // S1_R2 reads the scan as measured for any class, never through the FieldSpec 3 file
        assert_eq!(s(&rec, &["models", "s1_r2", "transfer"]), "none");
    }
    assert!(seen.iter().all(|n| *n > 0), "{seen:?}");
}

#[test]
fn a_lab_unit_file_gets_the_sd_rule_gates_for_its_serial_only() {
    let gains = [0.7, 0.6, 0.5];
    let t = correction(
        "transfer.lab.unit42",
        CLASS_HIRES,
        json!([{"op": "absorbance_affine", "gain": gains, "offset": 0.0, "clip_min": 1e-6}]),
        Some("4242"),
        json!({}),
    );
    let reg = with_file("unit42", &t, true);
    let eng = Engine::new(&reg).unwrap();
    let x0 = golden("mmc2_Reference_Sample_79");
    let mut seen = [0usize; 3];
    for (k, sd) in NOISE.iter().enumerate() {
        let x = noisy(&x0, *sd, 11 + k as u32, 0.08);
        let rec = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, Some(4242)));
        assert_eq!(
            s(&rec, &["stream", "transfer"]),
            "transfer.lab.unit42@1.0.0"
        );
        let (g, r, _, _) = check_record(&rec, gains, &format!("unit42 {sd}"));
        seen[0] += g;
        seen[1] += r;
        seen[2] += usize::from(g == 0);
        let other = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, Some(4243)));
        assert_eq!(
            s(&other, &["stream", "transfer"]),
            "transfer.labspec4.hires_to_std@0.3.0"
        );
    }
    assert!(seen.iter().all(|n| *n > 0), "{seen:?}");
}

/// Through v0.2 (pinned: superseded by 0.3.0, kept as history) the SD rule is its shipped N2 table exactly.
#[test]
fn v02_reproduces_its_shipped_cuts() {
    let reg = Registry::load(
        &[Location {
            dir: plugins(),
            origin: Origin::Bundled,
        }],
        &Pins::from([(
            "transfer.labspec4.hires_to_std".to_string(),
            spyder_core::plugins::Version(0, 2, 0),
        )]),
    );
    assert!(
        reg.notes.iter().all(|n| !n.starts_with("pin:")),
        "{:?}",
        reg.notes
    );
    let eng = Engine::new(&reg).unwrap();
    let x0 = golden("pair22_hires");
    for (k, sd) in NOISE.iter().enumerate() {
        let rec = eng.analyse_spectrum(
            &wl(),
            &noisy(&x0, *sd, 3 + k as u32, 0.15),
            &ctx(CLASS_HIRES, None),
        );
        let n2 = |w: &str| f(&rec, &["n2", w]);
        let gl = list(&rec, &["checks", "B6b", "signs_gated"]);
        let rl = list(&rec, &["checks", "B6b", "signs_reduced"]);
        let g = |sg: &str| gl.iter().chain(&rl).any(|x| x == sg);
        assert_eq!(g("plaster"), n2("2000-2100") > 150.0);
        assert_eq!(g("C1"), n2("2000-2100") > 125.0);
        assert_eq!(g("wax"), n2("1500-1600") > 60.0);
        assert_eq!(g("ester"), n2("2220-2245") > 250.0);
        if !rl.is_empty() {
            assert!(n2("1500-1600") <= 30.0);
        }
        assert_eq!(
            at(&rec, &["checks", "longwave", "tail_unreliable"]).as_bool(),
            Some(n2("2300-2400") > 150.0)
        );
        assert_eq!(
            at(&rec, &["checks", "longwave", "tail_unreliable_c1"]).as_bool(),
            Some(n2("2300-2400") > 250.0)
        );
        assert_eq!(
            at(&rec, &["signs", "soft", "eligible"]).as_bool(),
            Some(n2("2000-2100") <= 60.0)
        );
    }
}

fn models_of(rec: &Obj) -> &Obj {
    rec.get("models").and_then(V::as_obj).unwrap()
}

fn second_opinion_fired(rec: &Obj) -> bool {
    ["radiocarbon", "isotopes", "zooms"]
        .iter()
        .any(|a| match at(rec, &[a, "notes_all"]) {
            V::List(l) => l.iter().any(|n| {
                n.as_obj().and_then(|o| o.get("key")).and_then(V::as_str)
                    == Some("second_opinion_differs")
            }),
            _ => false,
        })
}

const NEAR: &str =
    "not assessed: near-uncorrected correction (read as a native standard-resolution scan)";

/// Standard-res never changes (Matt, 2026-10-19): a correction whose every noise-policy gain (the B6b gate and fallback
/// statistics, the long-wave tail reading, the soft-tier statistic) is within check.acquisition's std_like_within (15%)
/// of uncorrected, here a standard-resolution unit's own file, is read exactly like a native standard-resolution scan by
/// every noise policy: the uncorrected B6b table (no fallbacks), long-wave not assessed, the soft tier always eligible,
/// no S1_R2 and no second opinion. The noise levels reach the SD rules' cuts, so the test shows a difference.
#[test]
fn a_near_uncorrected_correction_reads_as_a_native_standard_scan() {
    let gains = [1.0, 1.05, 0.95];
    let t = correction(
        "transfer.lab.std_unit",
        CLASS_STD,
        json!([{"op": "absorbance_affine", "gain": gains, "offset": 0.0, "clip_min": 1e-6}]),
        Some("424242"),
        json!({}),
    );
    let reg = with_file("stdunit-near", &t, true);
    let eng = Engine::new(&reg).unwrap();
    let c = cuts();
    let x0 = golden("pair22_std");
    let mut sd_rule = [0usize; 3];
    for (k, sd) in NOISE.iter().enumerate() {
        let x = noisy(&x0, *sd, 21 + k as u32, 0.15);
        let rec = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, Some(424242)));
        let plain = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, Some(1)));
        assert_eq!(
            s(&rec, &["stream", "transfer"]),
            "transfer.lab.std_unit@1.0.0"
        );
        assert_eq!(s(&plain, &["stream", "transfer"]), "none");
        // the uncorrected table on N2 as measured: the native scan's gates, sign by sign
        assert_eq!(at(&rec, &["checks", "B6b"]), at(&plain, &["checks", "B6b"]));
        for sign in ["plaster", "wax", "ester", "burnt"] {
            assert_eq!(
                at(&rec, &["signs", sign, "gated"]),
                at(&plain, &["signs", sign, "gated"]),
                "{sd} {sign}"
            );
        }
        assert_eq!(at(&rec, &["C1", "gated"]), at(&plain, &["C1", "gated"]));
        assert_eq!(s(&rec, &["checks", "longwave", "status"]), NEAR);
        for k in ["tail_unreliable", "tail_unreliable_c1"] {
            assert_eq!(at(&rec, &["checks", "longwave", k]).as_bool(), Some(false));
            assert_eq!(
                at(&plain, &["checks", "longwave", k]).as_bool(),
                Some(false)
            );
        }
        assert_eq!(
            at(&rec, &["signs", "soft", "eligible"]).as_bool(),
            Some(true)
        );
        assert_eq!(s(&rec, &["signs", "soft", "status"]), "assessed");
        assert!(models_of(&rec).get("s1_r2").is_none());
        assert!(!second_opinion_fired(&rec));
        // what the SD rules would have said through this file
        let n2 = |w: &str| f(&rec, &["n2", w]);
        let tail = gains[2] * TAIL * n2("2300-2400");
        sd_rule[0] += usize::from(tail > c.b9);
        sd_rule[1] += usize::from(tail > c.c1_tail);
        sd_rule[2] += usize::from(gains[2] * C1_SWIR2 * n2("2000-2100") > c.soft);
    }
    assert!(sd_rule.iter().all(|n| *n > 0), "{sd_rule:?}");
}

/// An identity correction (gain 1, no blur) gives the native record itself: only the stream, the transfer the models
/// name and the long-wave status's reason differ.
#[test]
fn an_identity_unit_file_gives_the_native_record() {
    let t = correction(
        "transfer.lab.std_unit",
        CLASS_STD,
        json!([{"op": "absorbance_affine", "gain": 1.0, "offset": 0.0, "clip_min": 1e-6}]),
        Some("424242"),
        json!({}),
    );
    let reg = with_file("stdunit-identity", &t, true);
    let eng = Engine::new(&reg).unwrap();
    let x0 = golden("pair22_std");
    for (k, sd) in NOISE.iter().enumerate() {
        let x = noisy(&x0, *sd, 41 + k as u32, 0.15);
        let rec = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, Some(424242)));
        let plain = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, Some(424242 + 1)));
        let skip = |key: &str| {
            key.starts_with("stream.")
                || key.starts_with("dependencies.")
                || key.starts_with("instrument.serial")
                || key == "band_noise.transfer_key"
                || key == "checks.longwave.status"
                || (key.starts_with("models.") && key.ends_with(".transfer"))
        };
        let a: Vec<(String, V)> = spyder_core::pipeline::val::flatten(&V::Map(rec.clone()))
            .into_iter()
            .filter(|(k, _)| !skip(k))
            .collect();
        let b: Vec<(String, V)> = spyder_core::pipeline::val::flatten(&V::Map(plain.clone()))
            .into_iter()
            .filter(|(k, _)| !skip(k))
            .collect();
        // numbers to 1e-9 (the identity runs through absorbance and back)
        let same = |x: &(String, V), y: &(String, V)| {
            x.0 == y.0
                && match (&x.1, &y.1) {
                    (V::Num(p), V::Num(q)) => (p - q).abs() <= 1e-9 * (1.0 + q.abs()),
                    (p, q) => p == q,
                }
        };
        let diff: Vec<_> = a
            .iter()
            .zip(&b)
            .filter(|(x, y)| !same(x, y))
            .take(5)
            .collect();
        assert!(
            a.len() == b.len() && diff.is_empty(),
            "noise {sd}: {diff:?}"
        );
        assert_eq!(s(&rec, &["checks", "longwave", "status"]), NEAR);
    }
}

/// A unit file beyond 15% on any noise-policy statistic is a correction like any other: the SD rules.
#[test]
fn a_unit_file_beyond_the_near_band_gets_the_sd_rules() {
    let gains = [1.0, 1.25, 1.25];
    let t = correction(
        "transfer.lab.std_unit",
        CLASS_STD,
        json!([{"op": "absorbance_affine", "gain": gains, "offset": 0.0, "clip_min": 1e-6}]),
        Some("424242"),
        json!({}),
    );
    let reg = with_file("stdunit-far", &t, true);
    let eng = Engine::new(&reg).unwrap();
    let x0 = golden("pair22_std");
    let mut seen = [0usize; 2];
    for (k, sd) in NOISE.iter().enumerate() {
        let x = noisy(&x0, *sd, 61 + k as u32, 0.15);
        let rec = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, Some(424242)));
        assert_eq!(
            s(&rec, &["stream", "transfer"]),
            "transfer.lab.std_unit@1.0.0"
        );
        let (g, _, tl, _) = check_record(&rec, gains, &format!("std unit far {sd}"));
        seen[0] += g;
        seen[1] += tl;
        assert!(models_of(&rec).get("s1_r2").is_some());
    }
    assert!(seen.iter().all(|n| *n > 0), "{seen:?}");
}

#[test]
fn another_instruments_scan_with_no_correction_is_uncalibrated() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let x = golden("mmc2_Reference_Sample_79");
    let rec = eng.analyse_spectrum(&wl(), &x, &ctx(FS3, None));
    let std = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, None));
    let V::List(notes) = at(&rec, &["instrument", "notes"]) else {
        panic!()
    };
    let n = notes
        .iter()
        .filter_map(V::as_obj)
        .find(|n| n.get("key").and_then(V::as_str) == Some("instrument_uncalibrated"))
        .expect("the uncalibrated note");
    assert_eq!(n.get("class").and_then(V::as_str), Some(FS3));
    assert_eq!(s(&rec, &["stream", "transfer"]), "none");
    assert_eq!(at(&rec, &["checks", "B6b"]), at(&std, &["checks", "B6b"]));
    assert_eq!(s(&rec, &["checks", "longwave", "status"]), "assessed");
    assert_eq!(
        s(&std, &["checks", "longwave", "status"]),
        "not assessed: native standard-resolution scan"
    );
    assert!(rec
        .get("models")
        .and_then(V::as_obj)
        .unwrap()
        .get("s1_r2")
        .is_some());
    assert!(std
        .get("models")
        .and_then(V::as_obj)
        .unwrap()
        .get("s1_r2")
        .is_none());
    assert_eq!(
        f(&rec, &["models", "cons3", "value"]),
        f(&std, &["models", "cons3", "value"])
    );
}

#[test]
fn heat_reads_the_scan_as_measured_when_visible_levels_are_not_fitted() {
    let bundled = read(&plugins().join(SHIPPED));
    let t = correction(
        "transfer.labspec4.hires_to_std",
        CLASS_HIRES,
        bundled["operator"].clone(),
        None,
        json!({"provenance": {"visible_levels": "not_fitted"}}),
    );
    let reg = with_file("heat", &t, true);
    let x = golden("pair22_hires");
    let rec = Engine::new(&reg)
        .unwrap()
        .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
    let base = shipped();
    let fitted = Engine::new(&base)
        .unwrap()
        .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
    assert_eq!(
        s(&rec, &["stream", "transfer"]),
        "transfer.labspec4.hires_to_std@1.0.0"
    );
    assert!(note_keys(&rec).contains(&"visible_levels_not_fitted".to_string()));
    assert!(!note_keys(&fitted).contains(&"visible_levels_not_fitted".to_string()));
    // the heat check on the scan as measured
    let checks = base.checks();
    let p = |n: &str| &checks.iter().find(|c| c.check == n).unwrap().params;
    let CheckParams::Signs { kernel, .. } = p("signs") else {
        panic!()
    };
    let h = heat(&wl(), &x, &derivative(&x, kernel), p("heat"));
    for k in [
        "edge50_nm",
        "R_vis",
        "OH1433",
        "OH979",
        "charred",
        "calcined",
    ] {
        assert_eq!(at(&rec, &["signs", "burnt", k]), h.get(k).unwrap(), "{k}");
    }
    assert_ne!(
        at(&fitted, &["signs", "burnt", "R_vis"]),
        at(&rec, &["signs", "burnt", "R_vis"])
    );
    // only "fitted" or "not_fitted" (absent = fitted); null is refused too, with the reference's message, so the two
    // engines never read heat on different streams (Astra's whole-branch review: null used to mean fitted here and not
    // fitted in Python); a refused file is never selected
    for (k, v) in [
        ("partly", json!("partly")),
        ("null", Value::Null),
        ("number", json!(1)),
    ] {
        let mut bad = t.clone();
        bad["provenance"]["visible_levels"] = v;
        let err =
            spyder_core::transfer::parse_transfer(&spyder_core::plugins::Node::root(&bad), "x")
                .unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"provenance.visible_levels must be "fitted" or "not_fitted""#,
            "{k}"
        );
        let reg = with_file(&format!("heat-{k}"), &bad, false);
        let r2 = Engine::new(&reg)
            .unwrap()
            .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
        assert_eq!(
            s(&r2, &["stream", "transfer"]),
            "transfer.labspec4.hires_to_std@0.3.0",
            "{k}"
        );
        assert_eq!(
            at(&r2, &["signs", "burnt"]),
            at(&fitted, &["signs", "burnt"]),
            "{k}"
        );
    }
    let mut absent = t.clone();
    absent["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("visible_levels");
    assert!(
        spyder_core::transfer::parse_transfer(&spyder_core::plugins::Node::root(&absent), "x")
            .unwrap()
            .visible_levels_fitted
    );
}

#[test]
fn heat_outside_its_range_is_not_available() {
    let reg = shipped();
    let checks = reg.checks();
    let p = |n: &str| &checks.iter().find(|c| c.check == n).unwrap().params;
    let CheckParams::Signs { kernel, .. } = p("signs") else {
        panic!()
    };
    let w: Vec<f64> = (900..=2500).map(f64::from).collect();
    let x = golden("mmc2_Reference_Sample_79")[550..].to_vec();
    let h = heat(&w, &x, &derivative(&x, kernel), p("heat"));
    assert_eq!(
        h.get("status").and_then(V::as_str),
        Some("not assessed: not available on this instrument (the heat sign reads 450-1446 nm)")
    );
    assert!(matches!(h.get("fired"), Some(V::Null)));
}

#[test]
fn reader_offsets_are_added_to_their_readers_and_recorded() {
    let bundled = read(&plugins().join(SHIPPED));
    let offs = json!({"reader_offsets": {"collagen.ryder2026.2045": 0.5, "collagen.spyder.1500_oh_corrected": -0.3}});
    let t = correction(
        "transfer.labspec4.hires_to_std",
        CLASS_HIRES,
        bundled["operator"].clone(),
        None,
        offs,
    );
    let reg = with_file("offsets", &t, true);
    let x = golden("pair22_hires");
    let rec = Engine::new(&reg)
        .unwrap()
        .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
    let base = shipped();
    let plain = Engine::new(&base)
        .unwrap()
        .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
    let ry = |r: &Obj| f(r, &["models", "ryder2045", "value"]);
    assert!((ry(&rec) - (ry(&plain) + 0.5)).abs() < 1e-12);
    assert_eq!(f(&rec, &["models", "ryder2045", "reader_offset"]), 0.5);
    let comp = |r: &Obj, c: &str| f(r, &["models", "cons3", "components", c]);
    let mut c: Vec<f64> = ["wc2045", "wc1500", "F05"]
        .iter()
        .map(|k| comp(&plain, k))
        .collect();
    c[1] -= 0.3;
    assert!((comp(&rec, "wc1500") - c[1]).abs() < 1e-12);
    c.sort_by(f64::total_cmp);
    assert!((f(&rec, &["models", "cons3", "value"]) - c[1]).abs() < 1e-12);
    assert_eq!(
        f(&rec, &["models", "cons3", "component_offsets", "wc1500"]),
        -0.3
    );
    let row = csv_row(&rec, "radiocarbon");
    assert_eq!(
        row.get("ryder2045.reader_offset").and_then(V::as_f64),
        Some(0.5)
    );
    assert_eq!(
        row.get("cons3.wc1500.reader_offset").and_then(V::as_f64),
        Some(-0.3)
    );
    assert!(csv_row(&plain, "radiocarbon")
        .get("ryder2045.reader_offset")
        .is_none());
    // out of range or not finite: refused; a consensus or an unknown model: the file is disabled
    for bad in [json!(6.0), json!("x")] {
        let mut u = t.clone();
        u["reader_offsets"]["collagen.ryder2026.2045"] = bad;
        assert!(
            spyder_core::transfer::parse_transfer(&spyder_core::plugins::Node::root(&u), "x")
                .is_err()
        );
    }
    for (k, mid) in [
        ("cons", "collagen.spyder.consensus3_median"),
        ("unknown", "collagen.unknown"),
    ] {
        let mut u = t.clone();
        u["reader_offsets"] = json!({ mid: 0.2 });
        let reg = with_file(&format!("offsets-{k}"), &u, false);
        let rec = Engine::new(&reg)
            .unwrap()
            .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
        assert_eq!(
            s(&rec, &["stream", "transfer"]),
            "transfer.labspec4.hires_to_std@0.3.0"
        );
        assert!(
            reg.notes.iter().any(|n| n.contains("reader_offsets")),
            "{:?}",
            reg.notes
        );
    }
}

#[test]
fn a_corrections_own_second_opinion_gap() {
    let bundled = read(&plugins().join(SHIPPED));
    let x = golden("pair22_hires");
    let base = shipped();
    let plain = Engine::new(&base)
        .unwrap()
        .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
    let gap =
        (f(&plain, &["models", "s1_r2", "value"]) - f(&plain, &["models", "cons3", "value"])).abs();
    let fired = |r: &Obj| match at(r, &["radiocarbon", "notes_all"]) {
        V::List(l) => l.iter().any(|n| {
            n.as_obj().and_then(|o| o.get("key")).and_then(V::as_str)
                == Some("second_opinion_differs")
        }),
        _ => false,
    };
    assert!(gap < 4.1 && !fired(&plain));
    let t = correction(
        "transfer.labspec4.hires_to_std",
        CLASS_HIRES,
        bundled["operator"].clone(),
        None,
        json!({"provenance": {"second_opinion_gap": {"points": gap / 2.0, "n_scans": 25}}}),
    );
    let reg = with_file("gap", &t, true);
    let rec = Engine::new(&reg)
        .unwrap()
        .analyse_spectrum(&wl(), &x, &ctx(CLASS_HIRES, None));
    assert!(fired(&rec));
    let mut few = t.clone();
    few["provenance"]["second_opinion_gap"]["n_scans"] = json!(12);
    let err = spyder_core::transfer::parse_transfer(&spyder_core::plugins::Node::root(&few), "x")
        .unwrap_err();
    assert!(err.to_string().contains("n_scans"), "{err}");
}

const BAND_NOTES: [&str; 14] = [
    "flat_bands_lead",
    "plus_d",
    "lift_good",
    "lift_borderline",
    "lift_blocked_contaminant",
    "lift_blocked_noisy",
    "zooms_better_good",
    "zooms_better_protein",
    "zooms_better_1545",
    "zooms_better_1545_flat",
    "zooms_faint_protein",
    "positive_signs_all_six",
    "positive_signs_clear",
    "bands_too_noisy",
];

/// The notes of `a` with this key (objects, from notes_all).
fn notes_with(rec: &Obj, a: &str, key: &str) -> Vec<Obj> {
    match at(rec, &[a, "notes_all"]) {
        V::List(l) => l
            .iter()
            .filter_map(|n| n.as_obj().cloned())
            .filter(|n| n.get("key").and_then(V::as_str) == Some(key))
            .collect(),
        _ => panic!(),
    }
}

/// The public cases whose soft-tier sign is just over its line (Astra's whole-branch review reproduced the
/// unqualified soft note on them as FieldSpec 3).
const SOFT_CASES: [&str; 3] = [
    "soft_c1_just_over",
    "soft_ester_just_over",
    "soft_plaster_just_over",
];

fn flag_list(rec: &Obj, a: &str) -> Vec<Obj> {
    match at(rec, &[a, "flags"]) {
        V::List(l) => l.iter().filter_map(|f| f.as_obj().cloned()).collect(),
        _ => panic!(),
    }
}

/// DECISIONS 137 (amended 2026-10-19): a class with no built-in correction (FieldSpec 3 here) has the band calls off:
/// the model verdict stands (no flat bands lead, +D, lift or blocked lift), no band ZooMS line, positive-signs line or
/// bands-too-noisy note; evidence and the ZooMS band pattern say "not assessed: band evidence needs this instrument
/// built in" while the band readings stay (the close-ups); the ZooMS profile says Can't tell; flags still show, marked
/// provisional, with the same sort penalty, and so is the soft contaminant note (possible_thin_coating); S1_R2 is shown
/// but its second-opinion note is not added. B9 is the one deliberate exception (an input screen, the D2 profile's
/// shape against bone prototypes, not band evidence; DECISIONS 118, 137): when it passes the readings set the verdict,
/// when it fails its own "Doesn't look like bone" stands (public not_bone_sine). Every public end-to-end golden
/// spectrum, against the same spectrum on the standard-resolution class (unchanged). The same test runs on the oracle
/// (reference/tests/test_any_correction_policies.py).
#[test]
fn an_instrument_not_built_in_reads_its_readings_only() {
    const NB: &str = "not assessed: band evidence needs this instrument built in";
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let sp = read(&repo().join("reference/oracle/goldens/oracle_spectra_public_v1.json"));
    let gap = read(&plugins().join("profiles/profile.radiocarbon.json"))["parameters"]
        ["second_opinion"]["gap_points"]
        .as_f64()
        .unwrap();
    let mut seen = [0usize; 4];
    let mut soft_cases = Vec::new();
    // B9 outcomes on the instrument not built in: (passed or not assessed, failed)
    let mut b9 = [0usize; 2];
    let mut b9_failed = Vec::new();
    for spc in sp["spectra"].as_array().unwrap() {
        let x: Vec<f64> = spc["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let name = spc["name"].as_str().unwrap();
        let u = eng.analyse_spectrum(&wl(), &x, &ctx(FS3, None));
        let std = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, None));
        if u.get("evidence").is_none() {
            continue; // a Rescan
        }
        assert!(note_keys(&u).contains(&"instrument_uncalibrated".to_string()));
        assert_eq!(s(&u, &["evidence", "status"]), NB);
        assert!(matches!(at(&u, &["evidence", "level"]), V::Null));
        assert_eq!(
            at(&u, &["evidence", "bands"]),
            at(&std, &["evidence", "bands"])
        );
        assert_eq!(s(&u, &["zooms_pattern", "status"]), NB);
        for k in ["pattern", "verdict", "pattern_verdict", "vote_1545"] {
            assert!(
                matches!(at(&u, &["zooms_pattern", k]), V::Null),
                "{name} {k}"
            );
        }
        assert_eq!(
            at(&u, &["zooms_pattern", "bands"]),
            at(&std, &["zooms_pattern", "bands"])
        );
        assert_eq!(s(&std, &["evidence", "status"]), "assessed");
        assert_eq!(s(&std, &["zooms_pattern", "status"]), "assessed");
        let notfit = u
            .get("checks")
            .and_then(V::as_obj)
            .and_then(|c| c.get("B9"))
            .and_then(V::as_obj)
            .and_then(|b| b.get("outcome"))
            .and_then(V::as_str)
            == Some("fail");
        for a in ["radiocarbon", "isotopes", "zooms"] {
            let (fu, fs) = (flag_list(&u, a), flag_list(&std, a));
            assert_eq!(fu.len(), fs.len(), "{name} {a}");
            for (p, q) in fu.iter().zip(&fs) {
                assert_eq!(p.get("provisional").and_then(V::as_bool), Some(true));
                assert_eq!(p.get("key"), q.get("key"));
                assert_eq!(p.get("signs"), q.get("signs"));
                assert!(q.get("provisional").is_none());
            }
            assert_eq!(
                at(&u, &[a]).as_obj().unwrap().get("sort_penalty"),
                at(&std, &[a]).as_obj().unwrap().get("sort_penalty")
            );
            // the soft contaminant note is provisional too, as the flags are (never on the standard class)
            let soft = notes_with(&u, a, "possible_thin_coating");
            for n in &soft {
                assert_eq!(
                    n.get("provisional").and_then(V::as_bool),
                    Some(true),
                    "{name} {a}"
                );
            }
            for n in notes_with(&std, a, "possible_thin_coating") {
                assert!(n.get("provisional").is_none(), "{name} {a}");
            }
            if a == "radiocarbon" && !soft.is_empty() {
                seen[3] += 1;
                soft_cases.push(name.to_string());
            }
            let keys: Vec<String> = match at(&u, &[a, "notes_all"]) {
                V::List(l) => l
                    .iter()
                    .filter_map(|n| n.as_obj()?.get("key")?.as_str().map(str::to_string))
                    .collect(),
                _ => panic!(),
            };
            assert!(
                keys.iter()
                    .all(|k| !BAND_NOTES.contains(&k.as_str()) && k != "second_opinion_differs"),
                "{name} {a} {keys:?}"
            );
            if notfit {
                // B9 is a deliberate exception: its own verdict replaces the readings' (flags still provisional)
                assert_eq!(
                    s(&u, &[a, "verdict"]),
                    "Doesn't look like bone",
                    "{name} {a}"
                );
                assert_eq!(s(&u, &[a, "rule_step"]), "B9", "{name} {a}");
                continue;
            }
            // B9 passed (or was not assessed): the readings set the verdict
            if a == "zooms" {
                assert_eq!(s(&u, &[a, "verdict"]), "Can't tell");
                assert_eq!(s(&u, &[a, "rule_step"]), "band_evidence_not_built_in");
                assert!(matches!(at(&u, &[a, "pattern"]), V::Null));
            } else if let Some(mv) = at(&u, &[a, "model_verdict"]).as_str() {
                assert_eq!(s(&u, &[a, "verdict"]), mv, "{name} {a}");
                assert_eq!(s(&u, &[a, "rule_step"]), "model_verdict", "{name} {a}");
            }
        }
        b9[usize::from(notfit)] += 1;
        if notfit {
            b9_failed.push(name.to_string());
        }
        let step = s(&std, &["radiocarbon", "rule_step"]);
        seen[0] += usize::from(matches!(
            step,
            "flat_bands_lead" | "plus_d" | "lift_good" | "lift_borderline" | "lift_blocked"
        ));
        seen[1] += usize::from(!flag_list(&u, "radiocarbon").is_empty());
        let m = models_of(&u);
        assert!(m.get("s1_r2").is_some());
        let val = |k: &str| {
            m.get(k)
                .and_then(V::as_obj)
                .and_then(|o| o.get("value"))
                .and_then(V::as_f64)
        };
        if let (Some(s1), Some(c)) = (val("s1_r2"), val("cons3")) {
            seen[2] += usize::from((s1 - c).abs() > gap);
        }
    }
    assert!(seen.iter().all(|n| *n > 0), "{seen:?}");
    for c in SOFT_CASES {
        assert!(
            soft_cases.iter().any(|n| n == c),
            "{c}: no provisional soft note ({soft_cases:?})"
        );
    }
    // both B9 branches ran; not_bone_sine is caught on the instrument not built in
    assert!(b9.iter().all(|n| *n > 0), "{b9:?}");
    assert!(
        b9_failed.iter().any(|n| n == "not_bone_sine"),
        "{b9_failed:?}"
    );
}

const S1_R2: &str = "collagen.spyder.1500_snv_transfer_free";

/// `spyder predict` shares the pipeline's per-reader stream policy (Astra's whole-branch review): S1_R2 reads the scan
/// as measured even when a unit file for the scan's serial, or another instrument's correction, matches; every other
/// reader goes through that file. Public high_res_23 through a unit file holding the shipped v0.3 operators: S1_R2
/// reads 10.97832074 as measured (10.51328874 had it gone through the file). The same case runs on the reference
/// (reference/tests/test_any_correction_policies.py).
#[test]
fn predict_reads_s1_r2_as_measured_like_the_pipeline() {
    use spyder_core::plugins::ScanContext;
    use spyder_core::predict::predict_scan;
    use spyder_core::preprocess::Spectrum;
    let scan =
        spyder_core::read::read_file(&repo().join("testdata/scans/high_res/high_res_23.asd"))
            .unwrap();
    let serial = scan.header.serial.to_string();
    let (wl, r) = (&scan.wavelengths_nm, &scan.reflectance);
    let ops = read(&plugins().join(SHIPPED))["operator"].clone();
    for (tag, class, t) in [
        (
            "unit",
            CLASS_HIRES,
            correction(
                "transfer.lab.unit_hr23",
                CLASS_HIRES,
                ops.clone(),
                Some(&serial),
                json!({}),
            ),
        ),
        (
            "fs3",
            FS3,
            correction("transfer.test.fs3", FS3, ops.clone(), None, json!({})),
        ),
    ] {
        let reg = with_file(&format!("predict-{tag}"), &t, true);
        let tid = t["id"].as_str().unwrap();
        let pctx = ScanContext::new(scan.splices_nm.clone(), class, Some(serial.clone()));
        let got = predict_scan(&reg, wl, r, &pctx);
        let s1 = got.iter().find(|m| m.id == S1_R2).unwrap();
        assert!(s1.transfer.is_none(), "{tag}: {:?}", s1.transfer);
        let v = s1.prediction.as_ref().unwrap().value;
        // as measured: the model on the scan itself
        let md = reg.model(S1_R2).unwrap();
        let raw = md
            .predict(&Spectrum::new(wl.clone(), r.clone()), &pctx)
            .unwrap()
            .value;
        assert_eq!(v, raw, "{tag}");
        // every other reader goes through the file
        for m in got.iter().filter(|m| m.id != S1_R2) {
            assert_eq!(
                m.transfer.as_ref().map(|x| x.id.as_str()),
                Some(tid),
                "{tag} {}",
                m.id
            );
        }
        // through the file S1_R2 would read differently
        let tr = reg
            .transfers()
            .into_iter()
            .find(|x| x.header.id == tid)
            .unwrap();
        let corrected = md
            .predict(
                &tr.apply(&Spectrum::new(wl.clone(), r.clone()), &pctx)
                    .unwrap(),
                &pctx,
            )
            .unwrap()
            .value;
        assert!((v - corrected).abs() > 0.1, "{tag}: {v} {corrected}");
        if tag == "unit" {
            assert!((v - 10.97832074).abs() < 1e-8, "{v}");
            assert!((corrected - 10.51328874).abs() < 1e-8, "{corrected}");
        }
        // the pipeline record agrees
        let rec = Engine::new(&reg).unwrap().analyse_spectrum(
            wl,
            r,
            &Context {
                instrument_class: class.into(),
                class_source: "user".into(),
                serial: Some(u64::from(scan.header.serial)),
                splices_nm: scan.splices_nm.clone(),
            },
        );
        assert_eq!(s(&rec, &["stream", "transfer"]), format!("{tid}@1.0.0"));
        assert_eq!(s(&rec, &["models", "s1_r2", "transfer"]), "none");
        assert_eq!(f(&rec, &["models", "s1_r2", "value"]), v, "{tag}");
        let c = got
            .iter()
            .find(|m| m.id == "collagen.spyder.consensus3_median")
            .unwrap();
        assert_eq!(
            f(&rec, &["models", "cons3", "value"]),
            c.prediction.as_ref().unwrap().value,
            "{tag}"
        );
    }
}
