//! DECISIONS 137 Phase 2 (part 1): the engine works for ANY correction file. Noise and band-readability gains are
//! computed from the correction applied and the scan's own segments; unknown band noise is "too noisy" (fail safe),
//! never "readable"; a correction the engine cannot propagate noise through says so; a band or model whose kernel or
//! noise window does not sit in one detector segment is "not available on this instrument". The same cases run against
//! the Python oracle in reference/tests/test_general_transfer.py. Public spectra only; the test corrections are built
//! here from the shipped v0.2 file and the public golden spectra.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use spyder_core::pipeline::checks::{evidence, zooms};
use spyder_core::pipeline::kernels::derivative;
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::{Context, Engine};
use spyder_core::plugins::checks::CheckParams;
use spyder_core::plugins::gains::{band_gain, window_covered};
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry, State};
use spyder_core::plugins::{CLASS_HIRES, CLASS_STD};
use spyder_core::preprocess::{parse_chain, run_chain, Spectrum};

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

/// A public golden spectrum by name (plugins/golden_spectra_public_v2.json).
fn golden(name: &str) -> Vec<f64> {
    let g = read(&plugins().join("golden_spectra_public_v2.json"));
    g["spectra"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("{name}"))["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn ctx(class: &str, joins: &[f64]) -> Context {
    Context {
        instrument_class: class.into(),
        class_source: "user".into(),
        serial: None,
        splices_nm: joins.to_vec(),
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

fn opt<'a>(o: &'a Obj, path: &[&str]) -> Option<&'a V> {
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

/// The key of a test correction: the shipped id at version 1.0.0, so it replaces v0.2 for high-res scans (within one
/// id the highest version wins; a class-wide user file with another id never replaces the bundled one).
const TEST_KEY: &str = "transfer.labspec4.hires_to_std@1.0.0";

/// A test correction: the shipped v0.2 file at version 1.0.0 (`id` names the test in its title), these operators and
/// provenance.instrument_noise, and golden cases computed by the engine on three public spectra.
fn test_transfer(id: &str, ops: Value, noise: Option<Value>) -> Value {
    let mut t = read(&plugins().join("hires_to_std_v0_2.spyder-transfer.json"));
    t["version"] = json!("1.0.0");
    t["title"] = json!(format!("test correction {id}"));
    t["operator"] = ops.clone();
    if let Some(n) = noise {
        t["provenance"]["instrument_noise"] = n;
    }
    let chain = parse_chain(&ops).unwrap();
    let mut cases = Vec::new();
    for name in ["pair10_hires", "pair22_hires", "synthetic_hireslike_RS79"] {
        let g = read(&plugins().join("golden_spectra_public_v2.json"));
        let s = g["spectra"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap()
            .clone();
        let out = run_chain(
            &chain,
            &Spectrum::new(wl(), golden(name)),
            &[1000.0, 1800.0],
        )
        .unwrap();
        let mut at = serde_json::Map::new();
        for (w, v) in out.wl.iter().zip(&out.x).step_by(97) {
            at.insert(format!("{}", *w as i64), json!(v));
        }
        cases.push(json!({"name": name, "spectrum_sha256": s["sha256"],
            "scan_context": {"splices_nm": [1000.0, 1800.0], "instrument_class": CLASS_HIRES},
            "expected": {"transferred_at": at}}));
    }
    t["golden"]["cases"] = json!(cases);
    t
}

/// The shipped plug-ins (bundled) plus a user folder holding one test correction.
fn with_correction(tag: &str, t: &Value) -> Registry {
    let d = std::env::temp_dir().join(format!("spyder-gt-{tag}-{}", std::process::id()));
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
    assert_eq!(e.state, State::Loaded, "{:?}", e.error);
    assert!(reg.startup_errors.is_empty(), "{:?}", reg.startup_errors);
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

const READABLE: [&str; 7] = [
    "AM2175", "CH1689", "CH1728", "CH2262", "CH2284", "NH1545c", "NH2044",
];

/// A correction the engine cannot propagate noise through (here a smoothing step on reflectance): every band SD is
/// unknown (+infinity: unreadable unless strong, with the note), and B6 says why for every transferred model.
#[test]
fn a_correction_without_noise_propagation_makes_band_noise_unknown_and_b6_not_assessed() {
    let ops = json!([
        {"op": "gaussian_blur", "sigma_nm": [0.0, 3.3973, 3.3973], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"},
        {"op": "absorbance_affine", "gain": [0.6343, 0.705, 0.6258], "offset": [0.2312, 0.2073, 0.2453], "clip_min": 1e-6},
        {"op": "savgol", "window": 5, "polyorder": 2, "deriv": 0, "delta": 1.0, "mode": "interp"}
    ]);
    let reg = with_correction("smooth", &test_transfer("transfer.test.smooth", ops, None));
    let eng = Engine::new(&reg).unwrap();
    let why = "not assessed: no noise propagation through the correction's savgol step";
    for name in ["pair22_hires", "pair10_hires", "synthetic_hireslike_RS79"] {
        let rec = eng.analyse_spectrum(&wl(), &golden(name), &ctx(CLASS_HIRES, &[1000.0, 1800.0]));
        assert_eq!(at(&rec, &["stream", "transfer"]).as_str(), Some(TEST_KEY));
        let unk = at(&rec, &["band_noise", "noise_unknown"]).as_obj().unwrap();
        assert_eq!(unk.0.len(), READABLE.len(), "{name}");
        for b in READABLE {
            assert_eq!(unk.get(b).and_then(V::as_str), Some(why), "{name} {b}");
            assert!(matches!(at(&rec, &["band_noise", "sd_E", b]), V::Null));
        }
        let notes = at(&rec, &["instrument", "notes"]);
        let V::List(l) = notes else { panic!() };
        let n = l
            .iter()
            .filter_map(V::as_obj)
            .find(|n| n.get("key").and_then(V::as_str) == Some("band_noise_unknown"))
            .expect("the band-noise note");
        let V::List(bl) = n.get("bands").unwrap() else {
            panic!()
        };
        assert_eq!(bl.len(), READABLE.len());
        // every band: unreadable ("can't tell") unless strong (u >= 1.5); evidence is never "none"
        let ev = at(&rec, &["evidence"]).as_obj().unwrap();
        for (b, d) in &at(ev, &["bands"]).as_obj().unwrap().0 {
            let d = d.as_obj().unwrap();
            let u = d.get("u").and_then(V::as_f64).unwrap();
            let st = d.get("state").and_then(V::as_str).unwrap();
            assert!(matches!(d.get("sd_u"), Some(V::Null)), "{b}");
            if u < 1.5 {
                assert_eq!(st, "can't tell", "{name} {b} u {u}");
            } else {
                assert_ne!(st, "can't tell", "{name} {b} u {u}");
            }
        }
        assert_ne!(at(ev, &["level"]).as_str(), Some("none"), "{name}");
        // the ZooMS band check: a band is readable only when it is strong (E >= 1.5 w)
        for (b, d) in &at(&rec, &["zooms_pattern", "bands"]).as_obj().unwrap().0 {
            let d = d.as_obj().unwrap();
            assert!(matches!(d.get("sd_E"), Some(V::Null)), "{b}");
        }
        // B6: not assessed, with the reason, for every model read through the correction; S1_R2 (read as measured)
        // keeps its recorded gain
        for k in ["cons3", "ryder2045"] {
            assert_eq!(
                at(&rec, &["models", k, "B6", "status"]).as_str(),
                Some(why),
                "{k}"
            );
            assert!(
                matches!(at(&rec, &["models", k, "implied_sd"]), V::Null),
                "{k}"
            );
            assert!(
                matches!(at(&rec, &["models", k, "B6", "outcome"]), V::Null),
                "{k}"
            );
        }
        assert_eq!(
            at(&rec, &["models", "s1_r2", "B6", "status"]).as_str(),
            Some("assessed")
        );
    }
}

/// The fail safe at the rule level: flat bands with unknown noise never read "none" (no flat-band Unlikely); the same
/// spectrum with its noise known reads "none" (the shipped evidence golden case rc_flat_none_low_m).
#[test]
fn flat_bands_with_unknown_noise_never_read_as_flat() {
    let reg = shipped();
    let bands = reg.bands().unwrap();
    let ev = reg
        .checks()
        .into_iter()
        .find(|c| c.check == "evidence_levels")
        .unwrap();
    let zp = reg
        .checks()
        .into_iter()
        .find(|c| c.check == "zooms_patterns")
        .unwrap();
    let doc = read(&plugins().join("checks/check.evidence_levels.json"));
    let case = doc["golden"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "rc_flat_none_low_m")
        .unwrap()
        .clone();
    let gs = read(&plugins().join("checks/golden_spectra_checks_v1.json"));
    let x: Vec<f64> = gs["spectra"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["sha256"] == case["spectrum_sha256"])
        .unwrap()["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let CheckParams::EvidenceLevels { kernel, .. } = &ev.params else {
        panic!()
    };
    let e = derivative(&x, kernel);
    let known: std::collections::BTreeMap<String, f64> = case["inputs"]["sd_E"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
        .collect();
    let (_, level, _) = evidence(&wl(), &e, &ev.params, bands, &known);
    assert_eq!(level, "none");
    let (_, zv, _, _) = zooms(&wl(), &e, &zp.params, bands, &known);
    assert_eq!(zv, "Unlikely");
    // unknown noise: missing SDs and NaN SDs alike
    let nan: std::collections::BTreeMap<String, f64> =
        known.keys().map(|k| (k.clone(), f64::NAN)).collect();
    for sd in [std::collections::BTreeMap::new(), nan] {
        let (rec, level, s) = evidence(&wl(), &e, &ev.params, bands, &sd);
        assert_eq!(level, "can't tell");
        assert_eq!(s, None);
        for (b, d) in &at(&rec, &["bands"]).as_obj().unwrap().0 {
            assert_eq!(
                d.as_obj().unwrap().get("state").and_then(V::as_str),
                Some("can't tell"),
                "{b}"
            );
        }
        let (zrec, zv, _, _) = zooms(&wl(), &e, &zp.params, bands, &sd);
        assert_eq!(zv, "Can't tell");
        for (b, d) in &at(&zrec, &["bands"]).as_obj().unwrap().0 {
            assert_eq!(
                d.as_obj().unwrap().get("readable").and_then(V::as_bool),
                Some(false),
                "{b}"
            );
        }
    }
}

/// A scan whose join sits inside a band's kernel: that band's noise is "not available on this instrument" (unknown,
/// +infinity, with the note); the other bands are untouched.
#[test]
fn a_band_across_a_join_is_not_available_on_this_instrument() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let x = golden("mmc2_Reference_Sample_79");
    let rec = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, &[1000.0, 1740.0]));
    let unk = at(&rec, &["band_noise", "noise_unknown"]).as_obj().unwrap();
    assert_eq!(unk.0.len(), 1, "{unk:?}");
    let st = unk.get("CH1728").and_then(V::as_str).unwrap();
    assert!(
        st.starts_with("not assessed: not available on this instrument (band CH1728: 1711-1741 nm crosses the detector join at 1740 nm"),
        "{st}"
    );
    for b in READABLE.iter().filter(|b| **b != "CH1728") {
        assert!(
            at(&rec, &["band_noise", "sd_E", b]).as_f64().is_some(),
            "{b}"
        );
    }
    // the same scan at the LabSpec joins: no note, no unknown noise
    let rec = eng.analyse_spectrum(&wl(), &x, &ctx(CLASS_STD, &[1000.0, 1800.0]));
    assert!(opt(&rec, &["band_noise", "noise_unknown"]).is_none());
    let V::List(l) = at(&rec, &["instrument", "notes"]) else {
        panic!()
    };
    assert!(l.is_empty());
}

/// The three panel readers are linear: they now get implied SDs (exported), but B6 is not applied to them, so they
/// raise no new check (DECISIONS 137: no user-facing change because of them).
#[test]
fn panel_readers_report_an_implied_sd_without_b6() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    for (name, class) in [("pair22_std", CLASS_STD), ("pair22_hires", CLASS_HIRES)] {
        let rec = eng.analyse_spectrum(&wl(), &golden(name), &ctx(class, &[1000.0, 1800.0]));
        for k in ["lowrange", "ch1728", "nb1545"] {
            let sd = at(&rec, &["models", k, "implied_sd"]).as_f64().unwrap();
            assert!(sd > 0.0 && sd.is_finite(), "{name} {k}");
            assert_eq!(
                at(&rec, &["models", k, "B6", "status"]).as_str(),
                Some("not assessed: B6 not applied to this reader")
            );
            assert!(matches!(at(&rec, &["models", k, "B6", "outcome"]), V::Null));
        }
        for k in ["cons3", "ryder2045"] {
            assert_eq!(
                at(&rec, &["models", k, "B6", "status"]).as_str(),
                Some("assessed")
            );
        }
    }
}

/// A lab-built linear correction (other blur widths and gains, and its instrument's own noise correlation and C-H
/// envelope) gets computed gains with no table update: B6 is assessed and every band SD is known; the SDs follow the
/// file (a direct gain computation agrees) and its instrument noise.
#[test]
fn a_lab_built_linear_correction_gets_computed_gains() {
    let ops = json!([
        {"op": "gaussian_blur", "sigma_nm": [0.0, 2.6, 3.9], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"},
        {"op": "absorbance_affine", "gain": [0.65, 0.74, 0.58], "offset": [0.2, 0.2, 0.25], "clip_min": 1e-6}
    ]);
    let noise = json!({"correlation_nm": 1.4, "ch_envelope_factors": {"CH1728": 2.0}});
    let t = test_transfer("transfer.test.lab", ops.clone(), Some(noise.clone()));
    let reg = with_correction("lab", &t);
    let plain = with_correction("lab-plain", &test_transfer("transfer.test.lab", ops, None));
    let x = golden("pair29_hires");
    let c = ctx(CLASS_HIRES, &[1000.0, 1800.0]);
    let rec = Engine::new(&reg).unwrap().analyse_spectrum(&wl(), &x, &c);
    let rec_plain = Engine::new(&plain).unwrap().analyse_spectrum(&wl(), &x, &c);
    assert_eq!(at(&rec, &["stream", "transfer"]).as_str(), Some(TEST_KEY));
    assert!(opt(&rec, &["band_noise", "noise_unknown"]).is_none());
    for b in READABLE {
        assert!(
            at(&rec, &["band_noise", "sd_E", b]).as_f64().is_some(),
            "{b}"
        );
    }
    for k in ["cons3", "ryder2045"] {
        assert_eq!(
            at(&rec, &["models", k, "B6", "status"]).as_str(),
            Some("assessed")
        );
    }
    // the SDs equal the INDEPENDENT research path (reference/spyder_ref/golden_gen.py: scipy SG coefficients, an
    // explicit segment blur matrix, the explicit noise covariance, a line-by-line N2), not the engine's own routine:
    // research_model_sd / research_band_sd on pair29_hires through this correction and noise, joins 1000/1800
    let tr = reg
        .transfers()
        .into_iter()
        .find(|t| t.key() == TEST_KEY)
        .unwrap();
    for (path, want) in [
        (
            &["models", "ryder2045", "implied_sd"][..],
            0.9675378668166736,
        ),
        (&["band_noise", "sd_E", "CH1689"][..], 0.5519059834173786),
        (&["band_noise", "sd_E", "CH1728"][..], 0.7261920834439192),
        (&["band_noise", "sd_E", "NH2044"][..], 2.3771313043158995),
    ] {
        let got = at(&rec, path).as_f64().unwrap();
        assert!(
            (got - want).abs() <= 1e-9 * want,
            "{path:?}: {got} want {want}"
        );
    }
    // the file's own instrument noise: its correlation length (1.4 nm) moves every band SD; its C-H 1728 envelope
    // factor 2.0 replaces the default 1.72 (CH1689, same detector and kernel, keeps its default 1.52)
    let sd = |r: &Obj, b: &str| at(r, &["band_noise", "sd_E", b]).as_f64().unwrap();
    let r1689 = sd(&rec, "CH1689") / sd(&rec_plain, "CH1689");
    let r1728 = sd(&rec, "CH1728") / sd(&rec_plain, "CH1728");
    assert!(
        (r1689 - 1.0).abs() > 1e-3,
        "the correlation length moved the SD: {r1689}"
    );
    assert!((r1728 / r1689 - 2.0 / 1.72).abs() < 1e-9, "{r1728} {r1689}");
    // the transfer file's bytes are its identity; nothing in the tables names it
    let bytes = std::fs::read(plugins().join("noise_gains.json")).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(&tr.file_sha256));
}

/// A block-SNV correction (non-linear; its output keeps only the block's points): B6 and every band say "non-linear
/// correction", and the scan is scored without a panic.
#[test]
fn a_block_snv_correction_is_non_linear() {
    let ops = json!([
        {"op": "gaussian_blur", "sigma_nm": [0.0, 3.3973, 3.3973], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"},
        {"op": "snv", "ddof": 0, "blocks_nm": [[1100.0, 1700.0]]}
    ]);
    let reg = with_correction("snv", &test_transfer("transfer.test.snv", ops, None));
    let eng = Engine::new(&reg).unwrap();
    let rec = eng.analyse_spectrum(
        &wl(),
        &golden("pair22_hires"),
        &ctx(CLASS_HIRES, &[1000.0, 1800.0]),
    );
    let why = "not assessed: non-linear correction";
    for k in ["cons3", "ryder2045"] {
        assert_eq!(
            at(&rec, &["models", k, "B6", "status"]).as_str(),
            Some(why),
            "{k}"
        );
    }
    let unk = at(&rec, &["band_noise", "noise_unknown"]).as_obj().unwrap();
    assert_eq!(unk.0.len(), READABLE.len());
    assert!(unk.0.iter().all(|(_, v)| v.as_str() == Some(why)));
}

/// Astra's part-1 review: on a 1598-1750 nm grid the C-H 1689 kernel fits but its 1500-1600 nm noise window has one
/// centre (a zero MAD). The whole window, with the neighbours its second difference reads, must be on the grid; else
/// the band is "not available on this instrument" (+infinity in the pipeline), never a quiet SD.
#[test]
fn a_noise_window_the_grid_only_partly_covers_is_not_available() {
    let reg = shipped();
    let bands = reg.bands().unwrap();
    let wl: Vec<f64> = (1598..=1750).map(f64::from).collect();
    let err = band_gain(bands, "CH1689", &wl, &[], None).unwrap_err();
    assert_eq!(
        err.0,
        "not assessed: not available on this instrument (band CH1689: the N2 window 1500-1600 nm is not covered by the scan)"
    );
    let g = |a: i32, b: i32| (a..=b).map(f64::from).collect::<Vec<f64>>();
    assert!(window_covered(&g(1499, 1600), 1500.0, 1600.0));
    assert!(!window_covered(&g(1500, 1600), 1500.0, 1600.0));
    assert!(!window_covered(&g(1499, 1599), 1500.0, 1600.0));
}

/// Astra's part-1 review: an absorbance_affine gain of 0 erases the spectrum. Refused at load (both engines alike);
/// the bundled v0.3 stays the high-res correction.
#[test]
fn a_zero_gain_correction_is_refused_at_load() {
    for gain in [json!(0.0), json!([0.6343, 0.0, 0.6258])] {
        let mut t = read(&plugins().join("hires_to_std_v0_2.spyder-transfer.json"));
        t["id"] = json!("transfer.test.zero");
        t["version"] = json!("1.0.0");
        t["operator"][1]["gain"] = gain;
        let err = spyder_core::transfer::parse_transfer(&spyder_core::plugins::Node::root(&t), "x")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("absorbance_affine gain must be a finite non-zero number"),
            "{err}"
        );
    }
    let d = std::env::temp_dir().join(format!("spyder-gt-zero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut t = read(&plugins().join("hires_to_std_v0_2.spyder-transfer.json"));
    t["id"] = json!("transfer.test.zero");
    t["version"] = json!("1.0.0");
    t["operator"][1]["gain"] = json!(0.0);
    t["operator"][1]["offset"] = json!(0.2);
    std::fs::write(
        d.join("zero.spyder-transfer.json"),
        serde_json::to_vec_pretty(&t).unwrap(),
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
        .find(|e| e.path.ends_with("zero.spyder-transfer.json"))
        .unwrap();
    assert_ne!(e.state, State::Loaded);
    let rec = Engine::new(&reg).unwrap().analyse_spectrum(
        &wl(),
        &golden("pair22_hires"),
        &ctx(CLASS_HIRES, &[1000.0, 1800.0]),
    );
    assert_eq!(
        at(&rec, &["stream", "transfer"]).as_str(),
        Some("transfer.labspec4.hires_to_std@0.3.0")
    );
}

fn ctx_serial(class: &str, serial: Option<u64>) -> Context {
    Context {
        instrument_class: class.into(),
        class_source: "user".into(),
        serial,
        splices_nm: vec![1000.0, 1800.0],
    }
}

/// Registry (DECISIONS 137): versions are compared only within one id. A user class-wide correction for high-res
/// under another id, even at version 9.0.0, does not replace the bundled v0.3 (bundled first between ids); the
/// registry says so. Within the shipped id, a higher user version does replace it (tested above via TEST_KEY).
#[test]
fn versions_are_compared_within_an_id_only() {
    let mut t = test_transfer(
        "transfer.test.other",
        json!([
            {"op": "gaussian_blur", "sigma_nm": [0.0, 3.0, 3.0], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"},
            {"op": "absorbance_affine", "gain": [0.6, 0.7, 0.6], "offset": [0.2, 0.2, 0.25], "clip_min": 1e-6}
        ]),
        None,
    );
    t["id"] = json!("transfer.lab.other");
    t["version"] = json!("9.0.0");
    let reg = with_correction("other-id", &t);
    let rec = Engine::new(&reg).unwrap().analyse_spectrum(
        &wl(),
        &golden("pair22_hires"),
        &ctx(CLASS_HIRES, &[1000.0, 1800.0]),
    );
    assert_eq!(
        at(&rec, &["stream", "transfer"]).as_str(),
        Some("transfer.labspec4.hires_to_std@0.3.0")
    );
    assert!(
        reg.notes
            .iter()
            .any(|n| n.starts_with("user correction transfer.lab.other@9.0.0 is not used")),
        "{:?}",
        reg.notes
    );
}

/// A lab's own file for a standard-resolution unit (std -> std, source_serial given as text) is allowed and applies
/// to that unit only; the serial is compared as text, so a JSON string matches the file's numeric serial.
#[test]
fn a_standard_unit_file_applies_to_its_own_serial() {
    let mut t = test_transfer(
        "transfer.test.unit",
        json!([
            {"op": "absorbance_affine", "gain": [1.0, 1.05, 0.95], "offset": 0.0, "clip_min": 1e-6}
        ]),
        None,
    );
    t["id"] = json!("transfer.lab.std_unit");
    t["source_class"] = json!(CLASS_STD);
    t["target_class"] = json!(CLASS_STD);
    t["source_serial"] = json!("424242");
    let reg = with_correction("std-unit", &t);
    let eng = Engine::new(&reg).unwrap();
    let x = golden("pair22_std");
    let rec = eng.analyse_spectrum(&wl(), &x, &ctx_serial(CLASS_STD, Some(424242)));
    assert_eq!(
        at(&rec, &["stream", "transfer"]).as_str(),
        Some("transfer.lab.std_unit@1.0.0")
    );
    // its gate statistics' noise stays within 15% of the uncorrected noise: today's standard-resolution gates
    let plain = eng.analyse_spectrum(&wl(), &x, &ctx_serial(CLASS_STD, Some(1)));
    assert_eq!(at(&plain, &["stream", "transfer"]).as_str(), Some("none"));
    assert_eq!(at(&rec, &["checks", "B6b"]), at(&plain, &["checks", "B6b"]));
    // without the field source_serial, a file mapping a class to itself is refused
    let mut bad = t.clone();
    bad.as_object_mut().unwrap().remove("source_serial");
    let err = spyder_core::transfer::parse_transfer(&spyder_core::plugins::Node::root(&bad), "x")
        .unwrap_err();
    assert!(err.to_string().contains("must differ"), "{err}");
}
