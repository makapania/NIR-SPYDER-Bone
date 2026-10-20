//! `spyder read` end to end on synthetic files (no private data).

#![allow(clippy::field_reassign_with_default, clippy::needless_range_loop)] // test fixtures read better this way

#[path = "../../spyder-core/tests/common/mod.rs"]
mod common;

use std::path::PathBuf;
use std::process::Command;

use common::*;
use serde_json::Value;

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spyder-cli-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn spyder(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_spyder"))
        .args(args)
        .output()
        .expect("run spyder");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn folder(tag: &str) -> PathBuf {
    let d = tmpdir(tag);
    std::fs::write(d.join("a_good.asd"), AsdSpec::default().build()).unwrap();
    let mut refsave = AsdSpec::default();
    refsave.sample = refsave.reference.clone();
    std::fs::write(d.join("b_reference.ASD"), refsave.build()).unwrap();
    // an as8 that holds reflectance: outside the matrix (other joins are resolved since DECISIONS 87)
    let unsupported = AsdSpec {
        data_type: 1,
        ..Default::default()
    };
    std::fs::write(d.join("c_unsupported.asd"), unsupported.build()).unwrap();
    let full = AsdSpec::default().build();
    std::fs::write(d.join("d_truncated.asd"), &full[..20_000]).unwrap();
    std::fs::write(d.join("notes.txt"), b"not a spectrum").unwrap();
    std::fs::write(d.join("._a_good.asd"), b"AppleDouble").unwrap();
    d
}

#[test]
fn json_output_is_stable_and_complete() {
    let d = folder("json");
    let (code, out, err) = spyder(&["read", d.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["schema"], "spyder-bone/read");
    assert_eq!(v["schema_version"], 2);
    let files = v["files"].as_array().unwrap();
    let names: Vec<&str> = files
        .iter()
        .map(|f| f["file_name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "a_good.asd",
            "b_reference.ASD",
            "c_unsupported.asd",
            "d_truncated.asd"
        ]
    );
    let status: Vec<&str> = files
        .iter()
        .map(|f| f["acceptance"]["status"].as_str().unwrap())
        .collect();
    assert_eq!(status, ["accepted", "not_scored", "rejected", "rejected"]);
    assert_eq!(
        files[1]["acceptance"]["label"],
        "reference scan (not scored)"
    );
    assert_eq!(files[2]["acceptance"]["error_kind"], "unsupported");
    assert!(files[2]["acceptance"]["reason"]
        .as_str()
        .unwrap()
        .contains("not a raw"));
    assert_eq!(files[3]["acceptance"]["error_kind"], "truncated");
    let good = &files[0];
    assert_eq!(good["kind"], "sample");
    assert_eq!(good["header"]["serial"], 12345);
    assert_eq!(good["header"]["splice1_nm"], 1000.0);
    assert_eq!(good["timestamps"]["utc_offset_minutes"], -240);
    assert!(good["n2"]["2000_2100"].as_f64().unwrap() >= 0.0);
    let ids: Vec<&str> = good["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["B1", "B2", "B3", "B4", "B5", "B6", "B6b", "B7", "B8"]);
    assert_eq!(good["checks"][5]["assessment"]["status"], "not_assessed");
    assert_eq!(good["worst_outcome"], "pass");
    // a rejected file: B1 unusable, nothing else assessed
    let bad = &files[2];
    assert_eq!(bad["checks"][0]["outcome"], "unusable");
    assert_eq!(bad["worst_outcome"], "unusable");
    assert!(bad["header"].is_null());
    // field order in the text is the documented one
    let order = [
        "\"path\"",
        "\"file_name\"",
        "\"acceptance\"",
        "\"kind\"",
        "\"header\"",
        "\"timestamps\"",
        "\"splices_nm\"",
        "\"segments\"",
        "\"reflectance\"",
        "\"n2\"",
        "\"checks\"",
        "\"worst_outcome\"",
        "\"warnings\"",
    ];
    let pos: Vec<usize> = order.iter().map(|k| out.find(k).unwrap()).collect();
    assert!(pos.windows(2).all(|p| p[0] < p[1]), "{pos:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn text_output_and_single_file() {
    let d = folder("text");
    let (code, out, _) = spyder(&["read", d.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(
        out.contains("4 file(s): 1 accepted, 1 reference scan(s) not scored, 2 rejected"),
        "{out}"
    );
    assert!(out.contains("REJECTED (unsupported)"));
    let (code, out, _) = spyder(&["read", d.join("a_good.asd").to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(out.contains("serial 12345"));
    assert!(out.contains("1 file(s): 1 accepted"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn usage_errors() {
    assert_eq!(spyder(&["frobnicate"]).0, 2);
    assert_eq!(spyder(&["read"]).0, 2);
    assert_eq!(spyder(&["read", "a", "b"]).0, 2);
    assert_eq!(spyder(&["read", "--bogus", "x"]).0, 2);
    assert_eq!(spyder(&["read", "/definitely/not/here.asd"]).0, 1);
    let (code, out, _) = spyder(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("usage: spyder <command>"));
    for c in ["read", "validate", "predict"] {
        let (code, out, _) = spyder(&[c, "--help"]);
        assert_eq!(code, 0);
        assert!(out.contains(&format!("usage: spyder {c}")), "{out}");
    }
    assert_eq!(spyder(&["predict", "x.asd"]).0, 2, "--class is required");
    assert_eq!(spyder(&["predict", "x.asd", "--class", "medium"]).0, 2);
    assert_eq!(spyder(&["validate", "--pin", "nonsense", "x"]).0, 2);
}

/// DECISIONS 137: a file another instrument wrote is scored as that instrument's class (the registry names it by its
/// file format and detector joins), whatever `--class` says, uncorrected and labelled uncalibrated; `--class` takes any
/// registry class. An as7 file with the LabSpec joins 1000/1800 (a LabSpec 4 on older ASD software) is a LabSpec file:
/// `--class` decides, as for as8.
#[test]
fn other_instruments_are_scored_as_their_own_class() {
    let d = tmpdir("other");
    // the Trek-type signature: blank header joins, the SWIR1/SWIR2 step in the white reference after 1785 nm
    let mut trek = AsdSpec {
        dark_corrected: 0,
        splices: [0.0, 0.0],
        swir_gains: [390, 396],
        ..Default::default()
    };
    trek.serial = 4321;
    for (w, r) in wl().iter().zip(trek.reference.iter_mut()) {
        if *w > 1785.0 && *w <= 1800.0 {
            *r *= 0.6;
        }
    }
    let trek = trek.with_reflectance(&bone_reflectance());
    std::fs::write(d.join("trek.asd"), trek.build_as7(1)).unwrap();
    let lab7 = AsdSpec {
        dark_corrected: 0,
        ..Default::default()
    };
    std::fs::write(d.join("lab7.asd"), lab7.build_as7(1)).unwrap();
    let fs3 = AsdSpec {
        swir_gains: [633, 206],
        ..Default::default()
    };
    std::fs::write(d.join("fs3.001"), fs3.build_legacy(&bone_reflectance())).unwrap();
    let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let p = plugins.to_str().unwrap();
    for class in ["hires", "std", "asd.fieldspec3"] {
        let (code, out, err) = spyder(&[
            "predict",
            d.to_str().unwrap(),
            "--class",
            class,
            "--json",
            "--plugins",
            p,
        ]);
        assert_eq!(code, 0, "{err}");
        let v: Value = serde_json::from_str(&out).unwrap();
        let resolved = match class {
            "hires" => "asd.labspec4.hires",
            "std" => "asd.labspec4.std",
            c => c,
        };
        assert_eq!(v["files"].as_array().unwrap().len(), 3);
        for f in v["files"].as_array().unwrap() {
            let want = match f["file_name"].as_str().unwrap() {
                "trek.asd" => "asd.trek",
                "lab7.asd" => resolved,
                _ => "asd.fieldspec3",
            };
            assert_eq!(f["instrument_class"], want, "{class} {}", f["file_name"]);
            if f["file_name"] == "lab7.asd" {
                // a LabSpec file: the high-res correction on the High-res switch, as for as8
                let corrected = f["models"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| !m["transfer"].is_null());
                assert_eq!(corrected, class == "hires", "{class}");
                continue;
            }
            for m in f["models"].as_array().unwrap() {
                assert!(m["transfer"].is_null(), "{m}");
            }
        }
    }
    let (code, out, err) = spyder(&[
        "analyse",
        d.to_str().unwrap(),
        "--class",
        "hires",
        "--json",
        "--plugins",
        p,
    ]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    for f in v["files"].as_array().unwrap() {
        let ins = &f["record"]["instrument"];
        if f["path"].as_str().unwrap().ends_with("lab7.asd") {
            assert_eq!(ins["class"], "asd.labspec4.hires", "{ins}");
            assert_eq!(ins["class_source"], "user", "{ins}");
            assert!(
                ins["notes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|n| n["key"] != "instrument_uncalibrated"),
                "{ins}"
            );
            assert_ne!(f["record"]["stream"]["transfer"], "none");
            continue;
        }
        assert_eq!(ins["class_source"], "file_format", "{ins}");
        assert!(
            ins["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n["key"] == "instrument_uncalibrated"),
            "{ins}"
        );
        assert_eq!(f["record"]["stream"]["transfer"], "none");
        // S1_R2 (transfer-free) is shown on another instrument's scans
        assert!(f["record"]["models"]["s1_r2"].is_object());
    }
    assert_eq!(
        spyder(&[
            "predict",
            d.to_str().unwrap(),
            "--class",
            "asd.nothing",
            "--plugins",
            p
        ])
        .0,
        2
    );
    let _ = std::fs::remove_dir_all(&d);
}
