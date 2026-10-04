//! Regressions for the Codex review of the Phase 2 commit (1c8f5cc), plus a fuzz over every string field of
//! every shipped plug-in: a malformed file must be disabled, never panic.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use spyder_core::model::parse_model;
use spyder_core::pipeline::checkgold::{run_check_goldens, run_profile_goldens, CheckEnv};
use spyder_core::plugins::checks::parse_engine_check;
use spyder_core::plugins::golden::{
    load_golden_spectra, run_model_goldens, run_transfer_goldens, GoldenSpectra,
};
use spyder_core::plugins::profiles::parse_profile;
use spyder_core::plugins::refset::parse_reference_set;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry, State};
use spyder_core::plugins::tables::{
    parse_bands, parse_instruments, parse_noise_gains, run_table_goldens, TableRef,
};
use spyder_core::plugins::{ErrorKind, Node, ScanContext, Version, CLASS_HIRES};
use spyder_core::transfer::{select_pinned, Selection};

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn read(rel: &str) -> Value {
    serde_json::from_slice(&std::fs::read(plugins().join(rel)).unwrap()).unwrap()
}

fn folder(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spyder-codex2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &Path, name: &str, v: &Value) {
    std::fs::write(dir.join(name), serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

fn load(locs: &[(&Path, Origin)], pins: &Pins) -> Registry {
    Registry::load(
        &locs
            .iter()
            .map(|(d, o)| Location {
                dir: d.to_path_buf(),
                origin: *o,
            })
            .collect::<Vec<_>>(),
        pins,
    )
}

fn entry<'a>(reg: &'a Registry, name: &str) -> &'a spyder_core::plugins::registry::Entry {
    reg.entries
        .iter()
        .find(|e| e.path.file_name().unwrap() == name)
        .unwrap()
}

/// HIGH 1: non-ASCII in a hash or a band name used to panic (byte slicing inside a UTF-8 character).
#[test]
fn non_ascii_strings_disable_the_file_without_panicking() {
    let d = folder("utf8");
    std::fs::copy(
        plugins().join("golden_spectra_public_v2.json"),
        d.join("golden_spectra_public_v2.json"),
    )
    .unwrap();
    let mut m = read("collagen_2045_ryder2026_n140.spyder-model.json");
    m["golden"]["cases"][1]["spectrum_sha256"] = json!("aaaaaaaaaaa\u{e9}");
    write(&d, "m.json", &m);
    let mut c = read("checks/check.evidence_levels.json");
    c["parameters"]["core_bands"][0] = json!("A\u{e9}123");
    write(&d, "c.json", &c);
    let reg = load(&[(&d, Origin::User)], &Pins::new());
    for f in ["m.json", "c.json"] {
        let e = entry(&reg, f);
        assert_eq!(e.state, State::Disabled, "{f}: {:?}", e.error);
    }
}

fn string_paths(v: &Value, path: String, out: &mut Vec<String>) {
    match v {
        Value::String(_) => out.push(path),
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                string_paths(x, format!("{path}/{i}"), out);
            }
        }
        Value::Object(o) => {
            for (k, x) in o {
                string_paths(
                    x,
                    format!("{path}/{}", k.replace('~', "~0").replace('/', "~1")),
                    out,
                );
            }
        }
        _ => {}
    }
}

/// HIGH 1 (broadly): mutate every string field of every shipped plug-in (capped per file) to non-ASCII text;
/// parsing and goldens must return an error or pass, never panic.
#[test]
fn fuzz_every_string_field_of_every_shipped_plugin() {
    let gs: GoldenSpectra =
        load_golden_spectra(&plugins().join("golden_spectra_public_v2.json")).unwrap();
    let gs_checks =
        load_golden_spectra(&plugins().join("checks/golden_spectra_checks_v1.json")).unwrap();
    let bands = parse_bands(&Node::root(&read("bands.json"))).unwrap();
    let check_files = [
        "acquisition",
        "b9",
        "c1",
        "evidence_levels",
        "heat",
        "longwave",
        "signs",
        "zooms_patterns",
    ];
    let checks: Vec<_> = check_files
        .iter()
        .map(|c| {
            parse_engine_check(
                &Node::root(&read(&format!("checks/check.{c}.json"))),
                &plugins().join("checks"),
            )
            .unwrap()
        })
        .collect();
    let env = CheckEnv {
        bands: Some(&bands),
        checks: checks
            .iter()
            .map(|c| (c.check.clone(), &c.params))
            .collect(),
    };
    let transfers: Vec<_> = [
        "hires_to_std_v0_1.spyder-transfer.json",
        "hires_to_std_v0_2.spyder-transfer.json",
    ]
    .iter()
    .map(|f| {
        let bytes = std::fs::read(plugins().join(f)).unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        spyder_core::transfer::parse_transfer(
            &Node::root(&v),
            &spyder_core::plugins::sha256_hex(&bytes),
        )
        .unwrap()
    })
    .collect();
    let tmap: BTreeMap<String, &spyder_core::transfer::Transfer> = transfers
        .iter()
        .map(|t| (t.file_sha256.clone(), t))
        .collect();
    let files = [
        "collagen_2045_ryder2026_n140.spyder-model.json",
        "collagen_consensus3_median.spyder-model.json",
        "collagen_1500_snv_transfer_free.spyder-model.json",
        "hires_to_std_v0_2.spyder-transfer.json",
        "bands.json",
        "noise_gains.json",
        "instruments.json",
        "reference_set_mmc2_v1.json",
        "profiles/profile.radiocarbon.json",
        "profiles/profile.zooms.json",
        "checks/check.acquisition.json",
        "checks/check.b9.json",
        "checks/check.c1.json",
        "checks/check.evidence_levels.json",
        "checks/check.heat.json",
        "checks/check.longwave.json",
        "checks/check.signs.json",
        "checks/check.zooms_patterns.json",
    ];
    let mut n = 0;
    for f in files {
        let base = read(f);
        let mut paths = Vec::new();
        string_paths(&base, String::new(), &mut paths);
        let step = (paths.len() / 40).max(1);
        for p in paths.iter().step_by(step) {
            for variant in 0..2 {
                let mut v = base.clone();
                let s = v.pointer_mut(p).unwrap();
                *s = if variant == 0 {
                    json!("aaaaaaaaaaa\u{e9}")
                } else {
                    json!(format!("\u{e9}{}", s.as_str().unwrap()))
                };
                n += 1;
                let r = catch_unwind(AssertUnwindSafe(|| {
                    let node = Node::root(&v);
                    match v.get("format").and_then(Value::as_str).unwrap_or("") {
                        "spyder-bone/model" => {
                            if let Ok(m) = parse_model(&node) {
                                let _ = run_model_goldens(&m, &node, &gs);
                            }
                        }
                        "spyder-bone/transfer" => {
                            if let Ok(t) = spyder_core::transfer::parse_transfer(&node, "0") {
                                let _ = run_transfer_goldens(&t, &node, &gs);
                            }
                        }
                        "spyder-bone/bands" => {
                            if let Ok(b) = parse_bands(&node) {
                                let _ = run_table_goldens(&node, TableRef::Bands(&b), &gs, &tmap);
                            }
                        }
                        "spyder-bone/noise_gains" => {
                            if let Ok(b) = parse_noise_gains(&node) {
                                let _ = run_table_goldens(&node, TableRef::Noise(&b), &gs, &tmap);
                            }
                        }
                        "spyder-bone/instrument" => {
                            let _ = parse_instruments(&node);
                        }
                        "spyder-bone/reference_set" => {
                            let _ = parse_reference_set(&node);
                        }
                        "spyder-bone/analysis_profile" => {
                            if let Ok(p) = parse_profile(&node) {
                                let _ = run_profile_goldens(&p, &node);
                            }
                        }
                        "spyder-bone/engine_check" => {
                            if let Ok(c) = parse_engine_check(&node, &plugins().join("checks")) {
                                let _ = run_check_goldens(&c, &node, &env, &gs_checks);
                            }
                        }
                        _ => {}
                    }
                }));
                assert!(r.is_ok(), "{f}: mutating {p} panicked");
            }
        }
    }
    eprintln!("string-field fuzz: {n} mutated documents, no panic");
    assert!(n > 400, "{n}");
}

/// HIGH 2: a recursive noise consensus (a component that is itself a consensus) is rejected at parse time.
#[test]
fn recursive_noise_consensus_is_rejected() {
    let mut ng = read("noise_gains.json");
    let cid = "collagen.spyder.consensus3_median";
    ng["consensus"][cid]["components"]["wc2045"] = json!(cid);
    ng["models"][cid] = ng["models"]["collagen.spyder.2045_oh_corrected"].clone();
    let e = parse_noise_gains(&Node::root(&ng)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Schema, "{e}");
    // all components pointing to one model that is a second consensus
    let mut ng = read("noise_gains.json");
    let other = json!({"factor": 1.0, "components": {"a": cid, "b": cid, "c": cid}});
    ng["consensus"]["other"] = other;
    ng["models"][cid] = ng["models"]["collagen.spyder.2045_oh_corrected"].clone();
    assert!(parse_noise_gains(&Node::root(&ng)).is_err());
}

/// MEDIUM 4: an explicitly pinned experimental transfer is used; without the pin the active one is.
#[test]
fn pinned_experimental_transfer_is_selected() {
    let d = folder("pin");
    for f in [
        "golden_spectra_public_v2.json",
        "collagen_2045_ryder2026_n140.spyder-model.json",
        "hires_to_std_v0_2.spyder-transfer.json",
    ] {
        std::fs::copy(plugins().join(f), d.join(f)).unwrap();
    }
    let mut t = read("hires_to_std_v0_2.spyder-transfer.json");
    t["version"] = json!("0.3.0");
    t["status"] = json!("experimental");
    write(&d, "t03.json", &t);
    let ctx = ScanContext::new(vec![1000.0, 1800.0], CLASS_HIRES, None);
    let id = "transfer.labspec4.hires_to_std";
    let pick = |reg: &Registry| -> Version {
        let m = reg.model("collagen.ryder2026.2045").unwrap();
        match select_pinned(m, &ctx, &reg.transfers(), &reg.pinned_transfer_ids()) {
            Selection::Apply(t) => t.header.version,
            other => panic!("{other:?}"),
        }
    };
    let reg = load(&[(&d, Origin::User)], &Pins::new());
    assert_eq!(pick(&reg), Version(0, 2, 0));
    let reg = load(
        &[(&d, Origin::User)],
        &Pins::from([(id.to_string(), Version(0, 3, 0))]),
    );
    assert_eq!(pick(&reg), Version(0, 3, 0));
    assert!(reg.notes.iter().all(|n| !n.starts_with("pin:")));
    // a pin that cannot be honoured falls back visibly to the highest passing active version
    let reg = load(
        &[(&d, Origin::User)],
        &Pins::from([(id.to_string(), Version(9, 0, 0))]),
    );
    assert_eq!(pick(&reg), Version(0, 2, 0));
    assert!(reg.notes.iter().any(|n| n.starts_with("pin:")));
}

/// MEDIUM 5: a byte-identical copy that failed (here: its folder lacks the golden spectra) must not suppress
/// the working copy.
#[test]
fn identical_duplicate_keeps_the_loaded_copy() {
    let a = folder("dup-a");
    let b = folder("dup-b");
    let f = "collagen_2045_ryder2026_n140.spyder-model.json";
    std::fs::copy(plugins().join(f), a.join(f)).unwrap();
    std::fs::copy(plugins().join(f), b.join(f)).unwrap();
    std::fs::copy(
        plugins().join("golden_spectra_public_v2.json"),
        b.join("golden_spectra_public_v2.json"),
    )
    .unwrap();
    let reg = load(&[(&a, Origin::User), (&b, Origin::User)], &Pins::new());
    assert!(
        reg.model("collagen.ryder2026.2045").is_some(),
        "the working copy is selected"
    );
    let states: Vec<State> = reg
        .entries
        .iter()
        .filter(|e| e.path.file_name().unwrap() == f)
        .map(|e| e.state)
        .collect();
    assert!(
        states.contains(&State::Loaded) && states.contains(&State::Ignored),
        "{states:?}"
    );
}

/// MEDIUM 6: noise and band goldens must test every declared gain (model x transfer key, consensus).
#[test]
fn table_goldens_must_test_every_gain() {
    let d = folder("gains");
    for f in [
        "golden_spectra_public_v2.json",
        "hires_to_std_v0_1.spyder-transfer.json",
        "hires_to_std_v0_2.spyder-transfer.json",
    ] {
        std::fs::copy(plugins().join(f), d.join(f)).unwrap();
    }
    let mut ng = read("noise_gains.json");
    for c in ng["golden"]["cases"].as_array_mut().unwrap() {
        if c["transfer_key"] != "none" {
            c["expected"]
                .as_object_mut()
                .unwrap()
                .remove("SD:collagen.ryder2026.2045");
        }
    }
    write(&d, "noise_gains.json", &ng);
    let mut b = read("bands.json");
    for c in b["golden"]["cases"].as_array_mut().unwrap() {
        c["expected"].as_object_mut().unwrap().remove("SD_E:CH2284");
    }
    write(&d, "bands.json", &b);
    let reg = load(&[(&d, Origin::User)], &Pins::new());
    for f in ["noise_gains.json", "bands.json"] {
        let e = entry(&reg, f);
        assert_eq!(e.state, State::Disabled, "{f}");
        assert_eq!(e.error.as_ref().unwrap().kind, ErrorKind::GoldenInvalid);
        assert!(e
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("no case tests the gains"));
    }
}

#[test]
fn instrument_header_hint_ranges_parse_strictly_and_match_inclusively() {
    let base = read("instruments.json");
    let inst = parse_instruments(&Node::root(&base)).unwrap();
    // the shipped ranges, edges inclusive; both gains must agree; the gap and zero match nothing
    assert_eq!(inst.class_of_swir_gains(1, 60), Some(CLASS_HIRES));
    assert_eq!(inst.class_of_swir_gains(16, 21), Some(CLASS_HIRES));
    assert_eq!(
        inst.class_of_swir_gains(150, 65535),
        Some(spyder_core::plugins::CLASS_STD)
    );
    for (a, b) in [(61, 61), (149, 149), (0, 0), (16, 450), (450, 16), (0, 16)] {
        assert_eq!(inst.class_of_swir_gains(a, b), None, "({a}, {b})");
    }
    // malformed ranges are schema errors, never panics
    let hires = "asd.labspec4.hires";
    for (bad, why) in [
        (json!([1]), "[min, max]"),
        (json!([1, 2, 3]), "[min, max]"),
        (json!([60, 1]), "above max"),
        (json!([1, 200]), "overlaps"),
        (json!(["a", 2]), ""),
    ] {
        let mut v = base.clone();
        v["classes"][hires]["header_hint"]["swir_gain"] = bad.clone();
        let err = parse_instruments(&Node::root(&v)).expect_err(&format!("{bad}"));
        assert!(err.to_string().contains(why), "{bad}: {err}");
    }
    // a registry without hints still parses and simply suggests nothing
    let mut v = base.clone();
    for c in ["asd.labspec4.std", hires] {
        v["classes"][c]
            .as_object_mut()
            .unwrap()
            .remove("header_hint");
    }
    let inst = parse_instruments(&Node::root(&v)).unwrap();
    assert_eq!(inst.class_of_swir_gains(16, 16), None);
}
