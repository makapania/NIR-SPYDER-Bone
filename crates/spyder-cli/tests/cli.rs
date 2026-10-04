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
    let joins = AsdSpec {
        splices: [1000.0, 1830.0],
        ..Default::default()
    };
    std::fs::write(d.join("c_joins.asd"), joins.build()).unwrap();
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
    assert_eq!(v["schema_version"], 1);
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
            "c_joins.asd",
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
        .contains("1830"));
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
