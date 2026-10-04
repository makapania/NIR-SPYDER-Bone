//! Plug-in rules (PLAN section 4) on mutated copies of the shipped public files, in temporary folders:
//! the hardened golden runner (empty goldens rejected, ...), strict JSON, sidecars, reserved operators,
//! engine_min, conflicts, duplicates, pins, the startup-error state and transfer selection.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use spyder_core::plugins::npy;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry, State};
use spyder_core::plugins::{sha256_hex, ErrorKind, ScanContext, Version, CLASS_HIRES, CLASS_STD};
use spyder_core::transfer::{select, Selection};

const RYDER: &str = "collagen_2045_ryder2026_n140.spyder-model.json";
const GOLDEN: &str = "golden_spectra_public_v2.json";
const T02: &str = "hires_to_std_v0_2.spyder-transfer.json";

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn read(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(plugins().join(name)).unwrap()).unwrap()
}

/// A fresh temporary folder holding the golden spectra file.
fn folder(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spyder-rules-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(plugins().join(GOLDEN), d.join(GOLDEN)).unwrap();
    d
}

fn write(dir: &Path, name: &str, v: &Value) {
    std::fs::write(dir.join(name), serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

fn load_one(dir: &Path, origin: Origin) -> Registry {
    Registry::load(
        &[Location {
            dir: dir.to_path_buf(),
            origin,
        }],
        &Pins::new(),
    )
}

/// The state and error kind of the entry loaded from `name`.
fn outcome(reg: &Registry, name: &str) -> (State, Option<ErrorKind>, String) {
    let e = reg
        .entries
        .iter()
        .find(|e| e.path.file_name().unwrap() == name)
        .unwrap_or_else(|| panic!("{name} not found"));
    (
        e.state,
        e.error.as_ref().map(|x| x.kind),
        e.error
            .as_ref()
            .map(|x| x.message.clone())
            .unwrap_or_default(),
    )
}

/// Load a mutated Ryder model alone and return its outcome.
fn ryder_with(tag: &str, f: impl FnOnce(&mut Value)) -> (State, Option<ErrorKind>, String) {
    let d = folder(tag);
    let mut m = read(RYDER);
    f(&mut m);
    write(&d, "m.json", &m);
    let reg = load_one(&d, Origin::User);
    outcome(&reg, "m.json")
}

fn assert_rejected(o: (State, Option<ErrorKind>, String), kind: ErrorKind) {
    assert_eq!(o.0, State::Disabled, "{o:?}");
    assert_eq!(o.1, Some(kind), "{o:?}");
}

#[test]
fn unmodified_copy_loads() {
    let o = ryder_with("ok", |_| {});
    assert_eq!(o.0, State::Loaded, "{o:?}");
}

#[test]
fn empty_and_thin_goldens_are_rejected() {
    // the A10 regression: empty goldens used to pass as "0 checks, 0 failed"
    assert_rejected(
        ryder_with("empty", |m| m["golden"]["cases"] = json!([])),
        ErrorKind::GoldenInvalid,
    );
    assert_rejected(
        ryder_with("nocases", |m| {
            m["golden"].as_object_mut().unwrap().remove("cases");
        }),
        ErrorKind::GoldenInvalid,
    );
    assert_rejected(
        ryder_with("two", |m| {
            let c = m["golden"]["cases"].as_array_mut().unwrap();
            c.truncate(2);
        }),
        ErrorKind::GoldenInvalid,
    );
    // an empty expected block
    assert_rejected(
        ryder_with("emptyexp", |m| {
            m["golden"]["cases"][1]["expected"] = json!({})
        }),
        ErrorKind::GoldenInvalid,
    );
    // the required output missing
    assert_rejected(
        ryder_with("novalue", |m| {
            m["golden"]["cases"][1]["expected"]
                .as_object_mut()
                .unwrap()
                .remove("value");
        }),
        ErrorKind::GoldenInvalid,
    );
    // an output the engine does not know
    assert_rejected(
        ryder_with("unknown", |m| {
            m["golden"]["cases"][1]["expected"]["magic"] = json!(1.0)
        }),
        ErrorKind::GoldenInvalid,
    );
    // no case carries the feature vector
    assert_rejected(
        ryder_with("nofeat", |m| {
            for c in m["golden"]["cases"].as_array_mut().unwrap() {
                c.as_object_mut().unwrap().remove("features");
            }
        }),
        ErrorKind::GoldenInvalid,
    );
    // a feature vector of the wrong length
    assert_rejected(
        ryder_with("featlen", |m| {
            m["golden"]["cases"][0]["features"]
                .as_array_mut()
                .unwrap()
                .pop();
        }),
        ErrorKind::GoldenInvalid,
    );
    // a spectrum that is not in the golden spectra file
    assert_rejected(
        ryder_with("nospec", |m| {
            m["golden"]["cases"][1]["spectrum_sha256"] = json!("0".repeat(64))
        }),
        ErrorKind::GoldenInvalid,
    );
    // a non-numeric expected value
    assert_rejected(
        ryder_with("boolexp", |m| {
            m["golden"]["cases"][1]["expected"]["value"] = json!(true)
        }),
        ErrorKind::GoldenInvalid,
    );
}

#[test]
fn tolerance_is_engine_owned() {
    // looser than 1e-9: rejected
    assert_rejected(
        ryder_with("loose", |m| {
            m["golden"]["tolerance"] = json!({"abs": 1e-8, "rel": 1e-9})
        }),
        ErrorKind::GoldenInvalid,
    );
    assert_rejected(
        ryder_with("negtol", |m| {
            m["golden"]["tolerance"] = json!({"abs": -1.0, "rel": 1e-9})
        }),
        ErrorKind::GoldenInvalid,
    );
    // tighter: accepted (and still passes)
    let o = ryder_with("tight", |m| {
        m["golden"]["tolerance"] = json!({"abs": 1e-10, "rel": 1e-10})
    });
    assert_eq!(o.0, State::Loaded, "{o:?}");
    // missing: the engine maximum
    let o = ryder_with("notol", |m| {
        m["golden"].as_object_mut().unwrap().remove("tolerance");
    });
    assert_eq!(o.0, State::Loaded, "{o:?}");
}

#[test]
fn a_wrong_golden_value_fails() {
    let o = ryder_with("wrong", |m| {
        let v = m["golden"]["cases"][2]["expected"]["value"]
            .as_f64()
            .unwrap();
        m["golden"]["cases"][2]["expected"]["value"] = json!(v + 1e-6);
    });
    assert_rejected(o.clone(), ErrorKind::GoldenFailed);
    assert!(o.2.contains("value"), "{}", o.2);
    // a wrong coefficient is caught by the goldens
    let o = ryder_with("coef", |m| {
        let b = m["regression"]["coefficients"][5].as_f64().unwrap();
        m["regression"]["coefficients"][5] = json!(b * (1.0 + 1e-5));
    });
    assert_rejected(o, ErrorKind::GoldenFailed);
}

#[test]
fn schema_level_problems_are_rejected() {
    assert_rejected(
        ryder_with("engine", |m| m["engine_min"] = json!("1.2")),
        ErrorKind::NeedsNewerEngine,
    );
    assert_rejected(
        ryder_with("fv", |m| m["format_version"] = json!(2)),
        ErrorKind::Schema,
    );
    assert_rejected(
        ryder_with("ver", |m| m["version"] = json!("2.0")),
        ErrorKind::Schema,
    );
    assert_rejected(
        ryder_with("status", |m| m["status"] = json!("final")),
        ErrorKind::Schema,
    );
    assert_rejected(
        ryder_with("id", |m| m["id"] = json!("Ryder 2045")),
        ErrorKind::Schema,
    );
    assert_rejected(
        ryder_with("coeflen", |m| {
            m["regression"]["coefficients"]
                .as_array_mut()
                .unwrap()
                .pop();
        }),
        ErrorKind::Schema,
    );
    assert_rejected(
        ryder_with("nokind", |m| {
            m.as_object_mut().unwrap().remove("kind");
        }),
        ErrorKind::Schema,
    );
    assert_rejected(
        ryder_with("classifier", |m| m["kind"] = json!("classifier")),
        ErrorKind::Unsupported,
    );
    // reserved operators are "unsupported operator", never a guess
    assert_rejected(
        ryder_with("splice", |m| {
            m["preprocessing"].as_array_mut().unwrap().insert(
                0,
                json!({"op": "splice_correct", "joins": "from_scan", "fit_points_lower": 5, "fit_points_upper": 6}),
            )
        }),
        ErrorKind::Unsupported,
    );
    assert_rejected(
        ryder_with("wholesnv", |m| {
            m["preprocessing"]
                .as_array_mut()
                .unwrap()
                .push(json!({"op": "snv", "ddof": 0}))
        }),
        ErrorKind::Unsupported,
    );
    // a misspelled operator parameter
    assert_rejected(
        ryder_with("typo", |m| m["preprocessing"][1]["windw"] = json!(31)),
        ErrorKind::Schema,
    );
}

#[test]
fn non_finite_json_is_rejected() {
    let d = folder("nan");
    let text = String::from_utf8(std::fs::read(plugins().join(RYDER)).unwrap()).unwrap();
    let off = "\"offset\": 4.537214285714286";
    assert!(text.contains(off));
    std::fs::write(d.join("nan.json"), text.replacen(off, "\"offset\": NaN", 1)).unwrap();
    std::fs::write(
        d.join("inf.json"),
        text.replacen(off, "\"offset\": 1e999", 1),
    )
    .unwrap();
    let reg = load_one(&d, Origin::User);
    for f in ["nan.json", "inf.json"] {
        let o = outcome(&reg, f);
        assert_eq!(o.0, State::Disabled);
        // the identity cannot be read without parsing, so the error is a JSON one
        assert_eq!(o.1, Some(ErrorKind::Json), "{f}: {o:?}");
    }
}

fn npy_bytes(v: &[f64]) -> Vec<u8> {
    npy::encode(&[v.len()], v)
}

#[test]
fn sidecars_are_checked() {
    let m = read(RYDER);
    let coef: Vec<f64> = m["regression"]["coefficients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let bytes = npy_bytes(&coef);
    let sha = sha256_hex(&bytes);
    let with_sidecar = |tag: &str, name: &str, sha: &str, file: Option<(&str, Vec<u8>)>| {
        let d = folder(tag);
        if let Some((f, b)) = file {
            if let Some(parent) = Path::new(f).parent() {
                std::fs::create_dir_all(d.join(parent)).unwrap();
            }
            std::fs::write(d.join(f), b).unwrap();
        }
        let mut mm = m.clone();
        mm["regression"]["coefficients"] = json!({"npy": name, "sha256": sha});
        write(&d, "m.json", &mm);
        outcome(&load_one(&d, Origin::User), "m.json")
    };
    // a valid sidecar loads and the model passes its goldens (bit-exact coefficients)
    let o = with_sidecar(
        "sc_ok",
        "sub/coef.npy",
        &sha,
        Some(("sub/coef.npy", bytes.clone())),
    );
    assert_eq!(o.0, State::Loaded, "{o:?}");
    // tampered: hash mismatch
    let mut t = bytes.clone();
    let n = t.len();
    t[n - 1] ^= 1;
    assert_rejected(
        with_sidecar("sc_tamper", "coef.npy", &sha, Some(("coef.npy", t))),
        ErrorKind::Sidecar,
    );
    // missing file, escaping paths, backslashes, absolute paths, not .npy
    assert_rejected(
        with_sidecar("sc_missing", "coef.npy", &sha, None),
        ErrorKind::Sidecar,
    );
    for (tag, name) in [
        ("sc_dotdot", "../coef.npy"),
        ("sc_back", "sub\\coef.npy"),
        ("sc_abs", "/coef.npy"),
        ("sc_drive", "C:coef.npy"),
        ("sc_dot", "./coef.npy"),
    ] {
        assert_rejected(
            with_sidecar(tag, name, &sha, Some(("coef.npy", bytes.clone()))),
            ErrorKind::Sidecar,
        );
    }
    let o = with_sidecar(
        "sc_ext",
        "coef.bin",
        &sha,
        Some(("coef.bin", bytes.clone())),
    );
    assert_rejected(o, ErrorKind::Sidecar);
    // wrong dtype (big-endian) with a matching hash: still rejected
    let be = String::from_utf8_lossy(&bytes)
        .replacen("'<f8'", "'>f8'", 1)
        .into_bytes();
    let be_sha = sha256_hex(&be);
    assert_rejected(
        with_sidecar("sc_be", "coef.npy", &be_sha, Some(("coef.npy", be))),
        ErrorKind::Sidecar,
    );
    // non-finite values
    let mut c2 = coef.clone();
    c2[3] = f64::NAN;
    let nb = npy_bytes(&c2);
    let ns = sha256_hex(&nb);
    assert_rejected(
        with_sidecar("sc_nan", "coef.npy", &ns, Some(("coef.npy", nb))),
        ErrorKind::Sidecar,
    );
    // over 16 MiB
    let big = npy_bytes(&vec![0.5; 16 * 1024 * 1024 / 8 + 16]);
    let bs = sha256_hex(&big);
    assert_rejected(
        with_sidecar("sc_big", "coef.npy", &bs, Some(("coef.npy", big))),
        ErrorKind::Sidecar,
    );
}

fn bundled_and_user(tag: &str) -> (PathBuf, PathBuf) {
    let b = folder(&format!("{tag}-b"));
    let u = folder(&format!("{tag}-u"));
    std::fs::copy(plugins().join(RYDER), b.join(RYDER)).unwrap();
    (b, u)
}

fn load_two(b: &Path, u: &Path, pins: &Pins) -> Registry {
    Registry::load(
        &[
            Location {
                dir: b.to_path_buf(),
                origin: Origin::Bundled,
            },
            Location {
                dir: u.to_path_buf(),
                origin: Origin::User,
            },
        ],
        pins,
    )
}

#[test]
fn same_id_and_version_with_different_bytes_are_both_rejected() {
    let (b, u) = bundled_and_user("conflict");
    let mut m = read(RYDER);
    m["description"] = json!("edited text only");
    write(&u, "copy.json", &m);
    let reg = load_two(&b, &u, &Pins::new());
    assert_rejected(outcome(&reg, RYDER), ErrorKind::Conflict);
    assert_rejected(outcome(&reg, "copy.json"), ErrorKind::Conflict);
    assert!(reg.model("collagen.ryder2026.2045").is_none());
    // the bundled active file failed: startup-error state
    assert!(reg
        .startup_errors
        .iter()
        .any(|e| e.contains("collagen.ryder2026.2045")));
}

#[test]
fn byte_identical_duplicates_are_ignored() {
    let (b, u) = bundled_and_user("dup");
    std::fs::copy(plugins().join(RYDER), u.join("again.json")).unwrap();
    let reg = load_two(&b, &u, &Pins::new());
    assert_eq!(outcome(&reg, RYDER).0, State::Loaded);
    assert_eq!(outcome(&reg, "again.json").0, State::Ignored);
    assert!(reg.model("collagen.ryder2026.2045").is_some());
}

#[test]
fn a_failing_user_file_is_disabled_but_is_no_startup_error() {
    let (b, u) = bundled_and_user("userfail");
    let mut m = read(RYDER);
    m["version"] = json!("3.0.0");
    m["golden"]["cases"] = json!([]);
    write(&u, "bad.json", &m);
    let reg = load_two(&b, &u, &Pins::new());
    assert_rejected(outcome(&reg, "bad.json"), ErrorKind::GoldenInvalid);
    assert!(!reg.startup_errors.iter().any(|e| e.contains("bad.json")));
    // the bundled 2.0.0 is still selected
    assert_eq!(
        reg.model("collagen.ryder2026.2045").unwrap().header.version,
        Version(2, 0, 0)
    );
}

#[test]
fn highest_active_version_wins_and_pins_beat_it() {
    let (b, u) = bundled_and_user("pins");
    let mut m = read(RYDER);
    m["version"] = json!("2.1.0");
    write(&u, "v210.json", &m);
    m["version"] = json!("2.2.0");
    m["status"] = json!("withdrawn");
    write(&u, "v220.json", &m);
    m["version"] = json!("2.3.0");
    m["status"] = json!("experimental");
    write(&u, "v230.json", &m);
    let id = "collagen.ryder2026.2045";
    let ver = |reg: &Registry| reg.model(id).map(|m| m.header.version);
    // no pin: the highest ACTIVE version (withdrawn and experimental are never auto-selected)
    let reg = load_two(&b, &u, &Pins::new());
    assert_eq!(ver(&reg), Some(Version(2, 1, 0)));
    // a pin to an older active version
    let pin = |v| Pins::from([(id.to_string(), v)]);
    let reg = load_two(&b, &u, &pin(Version(2, 0, 0)));
    assert_eq!(ver(&reg), Some(Version(2, 0, 0)));
    assert!(reg.notes.iter().all(|n| !n.starts_with("pin:")));
    // a pin may select an experimental version (the user asked for it)
    let reg = load_two(&b, &u, &pin(Version(2, 3, 0)));
    assert_eq!(ver(&reg), Some(Version(2, 3, 0)));
    // pinned version withdrawn: fall back to the highest passing active version, with a visible note
    let reg = load_two(&b, &u, &pin(Version(2, 2, 0)));
    assert_eq!(ver(&reg), Some(Version(2, 1, 0)));
    assert!(
        reg.notes.iter().any(|n| n.contains("withdrawn")),
        "{:?}",
        reg.notes
    );
    // pinned version absent
    let reg = load_two(&b, &u, &pin(Version(9, 9, 9)));
    assert_eq!(ver(&reg), Some(Version(2, 1, 0)));
    assert!(reg.notes.iter().any(|n| n.contains("not installed")));
    // pinned version fails its goldens
    m["version"] = json!("2.4.0");
    m["status"] = json!("active");
    m["golden"]["cases"][0]["expected"]["value"] = json!(123.0);
    write(&u, "v240.json", &m);
    let reg = load_two(&b, &u, &pin(Version(2, 4, 0)));
    assert_eq!(ver(&reg), Some(Version(2, 1, 0)));
    assert!(reg.notes.iter().any(|n| n.contains("failed to load")));
    // nothing in the user folder was removed
    for f in ["v210.json", "v220.json", "v230.json", "v240.json"] {
        assert!(u.join(f).is_file());
    }
}

#[test]
fn consensus_components_must_match_loaded_files() {
    let d = folder("cons");
    for f in [
        "collagen_consensus3_median.spyder-model.json",
        "collagen_2045_oh_corrected.spyder-model.json",
        "collagen_1500_oh_corrected.spyder-model.json",
        "collagen_nh3_oh_corrected.spyder-model.json",
    ] {
        std::fs::copy(plugins().join(f), d.join(f)).unwrap();
    }
    let reg = load_one(&d, Origin::User);
    assert_eq!(
        outcome(&reg, "collagen_consensus3_median.spyder-model.json").0,
        State::Loaded
    );
    // a component file whose bytes changed (one more space): its sha256 no longer matches
    let p = d.join("collagen_1500_oh_corrected.spyder-model.json");
    let mut b = std::fs::read(&p).unwrap();
    b.push(b'\n');
    std::fs::write(&p, b).unwrap();
    let reg = load_one(&d, Origin::User);
    assert_rejected(
        outcome(&reg, "collagen_consensus3_median.spyder-model.json"),
        ErrorKind::CrossFile,
    );
}

#[test]
fn noise_gain_transfer_keys_must_match_transfer_files() {
    let d = folder("keys");
    for f in [
        "noise_gains.json",
        T02,
        "hires_to_std_v0_1.spyder-transfer.json",
    ] {
        std::fs::copy(plugins().join(f), d.join(f)).unwrap();
    }
    assert_eq!(
        outcome(&load_one(&d, Origin::User), "noise_gains.json").0,
        State::Loaded
    );
    // the transfer file changed (CRLF line endings): its file sha256 changes, the keys no longer match
    let p = d.join(T02);
    let text = String::from_utf8(std::fs::read(&p).unwrap()).unwrap();
    std::fs::write(&p, text.replace('\n', "\r\n")).unwrap();
    let o = outcome(&load_one(&d, Origin::User), "noise_gains.json");
    assert_eq!(o.0, State::Disabled, "{o:?}");
}

#[test]
fn transfer_goldens_are_hardened_too() {
    let d = folder("tg");
    let mut t = read(T02);
    t["golden"]["cases"] = json!([]);
    t["version"] = json!("0.2.1");
    write(&d, "t_empty.json", &t);
    let mut t = read(T02);
    t["golden"]["cases"][0]["expected"]["transferred_at"] = json!({});
    t["version"] = json!("0.2.2");
    write(&d, "t_emptyat.json", &t);
    let mut t = read(T02);
    let v = t["golden"]["cases"][1]["expected"]["transferred_at"]["2045"]
        .as_f64()
        .unwrap();
    t["golden"]["cases"][1]["expected"]["transferred_at"]["2045"] = json!(v * (1.0 + 1e-6));
    t["version"] = json!("0.2.3");
    write(&d, "t_wrong.json", &t);
    let mut t = read(T02);
    t["golden"]["cases"][1]["expected"]["transferred_at"]["9999"] = json!(0.5);
    t["version"] = json!("0.2.4");
    write(&d, "t_offgrid.json", &t);
    let mut t = read(T02);
    t["operator"][1]["gain"] = json!([0.6343, 0.705]);
    t["version"] = json!("0.2.5");
    write(&d, "t_segments.json", &t);
    let reg = load_one(&d, Origin::User);
    assert_rejected(outcome(&reg, "t_empty.json"), ErrorKind::GoldenInvalid);
    assert_rejected(outcome(&reg, "t_emptyat.json"), ErrorKind::GoldenInvalid);
    assert_rejected(outcome(&reg, "t_wrong.json"), ErrorKind::GoldenFailed);
    assert_rejected(outcome(&reg, "t_offgrid.json"), ErrorKind::GoldenFailed);
    // two gains for a three-segment scan: the transfer cannot run -> failed, never a guess
    assert_rejected(outcome(&reg, "t_segments.json"), ErrorKind::GoldenFailed);
}

#[test]
fn transfer_selection_rule() {
    let d = folder("sel");
    let base = read(T02);
    let mut class_wide = base.clone();
    class_wide["id"] = json!("transfer.test.class");
    class_wide["version"] = json!("1.0.0");
    write(&d, "class.json", &class_wide);
    let mut newer = class_wide.clone();
    newer["version"] = json!("1.1.0");
    newer["id"] = json!("transfer.test.class_newer");
    write(&d, "class_newer.json", &newer);
    let mut serial = base.clone();
    serial["id"] = json!("transfer.test.serial");
    serial["version"] = json!("0.0.1");
    serial["source_serial"] = json!(28313);
    write(&d, "serial.json", &serial);
    let mut exp = base.clone();
    exp["id"] = json!("transfer.test.experimental");
    exp["version"] = json!("9.0.0");
    exp["status"] = json!("experimental");
    write(&d, "exp.json", &exp);
    let mut wd = base.clone();
    wd["id"] = json!("transfer.test.withdrawn");
    wd["version"] = json!("9.1.0");
    wd["status"] = json!("withdrawn");
    write(&d, "wd.json", &wd);
    std::fs::copy(plugins().join(RYDER), d.join(RYDER)).unwrap();
    std::fs::copy(
        plugins().join("collagen_1500_snv_transfer_free.spyder-model.json"),
        d.join("s1.json"),
    )
    .unwrap();
    let reg = load_one(&d, Origin::User);
    for f in [
        "class.json",
        "class_newer.json",
        "serial.json",
        "exp.json",
        "wd.json",
    ] {
        assert_eq!(outcome(&reg, f).0, State::Loaded, "{f}");
    }
    let all: Vec<&spyder_core::transfer::Transfer> = reg
        .entries
        .iter()
        .filter_map(|e| match &e.item {
            Some(spyder_core::plugins::registry::Item::Transfer(t)) => Some(t.as_ref()),
            _ => None,
        })
        .collect();
    let ryder = reg.model("collagen.ryder2026.2045").unwrap();
    let s1 = reg.model("collagen.spyder.1500_snv_transfer_free").unwrap();
    let ctx = |class: &str, serial: Option<&str>| {
        ScanContext::new(vec![1000.0, 1800.0], class, serial.map(str::to_string))
    };
    // same class: no transfer
    assert!(matches!(
        select(ryder, &ctx(CLASS_STD, None), &all),
        Selection::NotNeeded
    ));
    // also_valid_for: no transfer (S1_R2 on high-res)
    assert!(matches!(
        select(s1, &ctx(CLASS_HIRES, None), &all),
        Selection::NotNeeded
    ));
    // serial-specific beats class-wide (even with a lower version)
    match select(ryder, &ctx(CLASS_HIRES, Some("28313")), &all) {
        Selection::Apply(t) => assert_eq!(t.header.id, "transfer.test.serial"),
        other => panic!("{other:?}"),
    }
    // another serial: the highest ACTIVE class-wide version (experimental and withdrawn never selected)
    match select(ryder, &ctx(CLASS_HIRES, Some("28247")), &all) {
        Selection::Apply(t) => assert_eq!(t.header.id, "transfer.test.class_newer"),
        other => panic!("{other:?}"),
    }
    // no transfer from this class: the model still runs (Missing -> gentle note)
    assert!(matches!(
        select(ryder, &ctx("asd.labspec4.other", None), &all),
        Selection::Missing
    ));
}

#[test]
fn startup_error_without_a_verdict_model() {
    // a bundled folder with only Ryder 2045: no profile names a verdict model
    let d = folder("noverdict");
    std::fs::copy(plugins().join(RYDER), d.join(RYDER)).unwrap();
    let reg = load_one(&d, Origin::Bundled);
    assert!(reg.startup_error());
    assert!(reg
        .startup_errors
        .iter()
        .any(|e| e.contains("no verdict model")));
    // a missing bundled folder is a startup error too
    let reg = load_one(&d.join("absent"), Origin::Bundled);
    assert!(reg.startup_error());
    // a missing user folder is only a note
    let reg = Registry::load(
        &[
            Location {
                dir: plugins(),
                origin: Origin::Bundled,
            },
            Location {
                dir: d.join("absent"),
                origin: Origin::User,
            },
        ],
        &Pins::new(),
    );
    assert!(!reg.startup_error(), "{:?}", reg.startup_errors);
}

#[test]
fn engine_check_and_profile_goldens_must_test_something() {
    let d = folder("deferred");
    // check goldens now run on load (Phase 3): the checks' golden spectra file sits beside them
    std::fs::copy(
        plugins().join("checks/golden_spectra_checks_v1.json"),
        d.join("golden_spectra_checks_v1.json"),
    )
    .unwrap();
    for f in ["checks/check.longwave.json", "profiles/profile.zooms.json"] {
        let mut v = read(f);
        let name = Path::new(f)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        write(&d, &name, &v);
        v["golden"]["cases"] = json!([]);
        v["version"] = json!("1.0.1");
        write(&d, &format!("empty_{name}"), &v);
    }
    let reg = load_one(&d, Origin::User);
    let o = outcome(&reg, "check.longwave.json");
    assert_eq!(o.0, State::Loaded, "{o:?}");
    assert_rejected(
        outcome(&reg, "empty_check.longwave.json"),
        ErrorKind::GoldenInvalid,
    );
    assert_rejected(
        outcome(&reg, "empty_profile.zooms.json"),
        ErrorKind::GoldenInvalid,
    );
    // the profile names models that are not loaded in this folder
    assert_rejected(outcome(&reg, "profile.zooms.json"), ErrorKind::CrossFile);
}

#[test]
fn models_disagree_rule_must_be_known() {
    // DECISIONS 65: the only rule is "components in different verdict bands"; the old span threshold is gone
    type Mutate = fn(&mut Value);
    let mutants: [(&str, Mutate); 3] = [
        ("nospan", |v| {
            v["parameters"]["notes"]
                .as_object_mut()
                .unwrap()
                .remove("models_disagree_min_span");
        }),
        ("unknown", |v| {
            v["parameters"]["notes"]["models_disagree_when"] = json!("span_above");
        }),
        ("old", |v| {
            let nt = v["parameters"]["notes"].as_object_mut().unwrap();
            nt.remove("models_disagree_when");
            nt.insert("models_disagree_span_above".into(), json!(3.0));
        }),
    ];
    for (tag, f) in mutants {
        let d = folder(&format!("disagree-{tag}"));
        let mut v = read("profiles/profile.radiocarbon.json");
        f(&mut v);
        write(&d, "p.json", &v);
        let o = outcome(&load_one(&d, Origin::User), "p.json");
        assert_rejected(o.clone(), ErrorKind::Schema);
        assert!(o.2.contains("models_disagree_"), "{tag}: {o:?}");
    }
}

#[test]
fn discovery_skips_hidden_files_and_non_plugins() {
    let d = folder("disc");
    std::fs::copy(plugins().join(RYDER), d.join("._m.json")).unwrap();
    std::fs::copy(plugins().join(RYDER), d.join(".hidden.json")).unwrap();
    std::fs::write(d.join("notes.json"), b"{\"hello\": 1}").unwrap();
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::copy(plugins().join(RYDER), d.join("sub").join("m.json")).unwrap();
    std::fs::copy(plugins().join(GOLDEN), d.join("sub").join(GOLDEN)).unwrap();
    let reg = load_one(&d, Origin::User);
    let names: Vec<String> = reg
        .entries
        .iter()
        .map(|e| e.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(!names.iter().any(|n| n.starts_with('.')), "{names:?}");
    assert_eq!(outcome(&reg, "notes.json").0, State::Ignored);
    assert_eq!(outcome(&reg, "m.json").0, State::Loaded);
}

/// Codex review (ZooMS line build): an empty `n_type_bands` list would make every scan pass the protein line's strength
/// test in one engine and fail it in the other. The loader rejects it (as the schema does, minItems 1), so the stronger
/// protein line can never be enabled by an empty list; the shipped list is non-empty.
#[test]
fn empty_protein_band_list_does_not_enable_strong_line() {
    let shipped = read("profiles/profile.radiocarbon.json");
    let bands = shipped["parameters"]["notes"]["zooms_better"]["protein"]["n_type_bands"]
        .as_array()
        .unwrap();
    assert!(!bands.is_empty());
    let d = folder("empty-protein-bands");
    let mut v = shipped.clone();
    v["parameters"]["notes"]["zooms_better"]["protein"]["n_type_bands"] = json!([]);
    write(&d, "p.json", &v);
    let o = outcome(&load_one(&d, Origin::User), "p.json");
    assert_rejected(o.clone(), ErrorKind::Schema);
    assert!(o.2.contains("n_type_bands"), "{o:?}");
}
