//! `spyder validate` and `spyder predict` end to end on the shipped plug-ins and synthetic scans, plus the
//! Codex Phase 1 review regressions for the CLI (non-UTF-8 arguments, directory cycles).

#![allow(clippy::field_reassign_with_default)]

#[path = "../../spyder-core/tests/common/mod.rs"]
mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use serde_json::Value;

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spyder-cli-p2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[OsString]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_spyder"))
        .args(args)
        .env_remove("SPYDER_USER_PLUGINS_DIR")
        .env_remove("SPYDER_PLUGINS_DIR")
        .output()
        .expect("run spyder");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn spyder(args: &[&str]) -> (i32, String, String) {
    run(&args.iter().map(OsString::from).collect::<Vec<_>>())
}

#[test]
fn validate_the_shipped_folder_and_one_file() {
    let p = plugins();
    let (code, out, err) = spyder(&["validate", p.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "{out}\n{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "spyder-bone/validate");
    assert_eq!(v["ok"], true);
    let files = v["files"].as_array().unwrap();
    assert!(files.len() >= 20);
    assert!(files.iter().all(|f| f["state"] == "loaded"));
    let cons = files
        .iter()
        .find(|f| f["id"] == "collagen.spyder.consensus3_median")
        .unwrap();
    assert!(cons["golden_checks"].as_u64().unwrap() > 0);
    assert_eq!(cons["golden_failed"], 0);
    assert_eq!(cons["selected"], true);
    // one file, text
    let f = p.join("collagen_2045_ryder2026_n140.spyder-model.json");
    let (code, out, _) = spyder(&["validate", f.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("collagen.ryder2026.2045@2.0.0"));
    assert!(out.contains("OK: 1 file(s), 1 loaded"));
}

#[test]
fn validate_reports_a_rejected_file() {
    let d = tmpdir("reject");
    std::fs::copy(
        plugins().join("golden_spectra_public_v2.json"),
        d.join("golden_spectra_public_v2.json"),
    )
    .unwrap();
    let mut m: Value = serde_json::from_slice(
        &std::fs::read(plugins().join("collagen_2045_ryder2026_n140.spyder-model.json")).unwrap(),
    )
    .unwrap();
    m["golden"]["cases"] = serde_json::json!([]);
    std::fs::write(d.join("empty.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let (code, out, _) = spyder(&["validate", d.join("empty.json").to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("DISABLED"), "{out}");
    assert!(out.contains("at least 3 cases"), "{out}");
    // as a bundled folder (no profile, a failing active file): the startup-error state
    let (code, out, _) = spyder(&["validate", d.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(out.contains("STARTUP ERROR"), "{out}");
    // as a user folder: the file is disabled, but there is no startup error
    let (code, out, _) = spyder(&["validate", d.to_str().unwrap(), "--user"]);
    assert_eq!(code, 1);
    assert!(!out.contains("STARTUP ERROR"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn predict_runs_every_active_model_with_identities() {
    let d = tmpdir("predict");
    std::fs::write(d.join("bone.asd"), AsdSpec::default().build()).unwrap();
    let mut refsave = AsdSpec::default();
    refsave.sample = refsave.reference.clone();
    std::fs::write(d.join("white.asd"), refsave.build()).unwrap();
    std::fs::write(d.join("broken.asd"), b"as8 nope").unwrap();
    let p = plugins();
    for class in ["std", "hires"] {
        let (code, out, err) = spyder(&[
            "predict",
            d.to_str().unwrap(),
            "--class",
            class,
            "--json",
            "--plugins",
            p.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "{err}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["schema"], "spyder-bone/predict");
        assert!(v["plugins"]["startup_errors"]
            .as_array()
            .unwrap()
            .is_empty());
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 3);
        let bone = files.iter().find(|f| f["file_name"] == "bone.asd").unwrap();
        assert_eq!(bone["status"], "accepted");
        let models = bone["models"].as_array().unwrap();
        assert!(models.len() >= 6, "{models:?}");
        // CONS3 first, with its components
        assert_eq!(models[0]["id"], "collagen.spyder.consensus3_median");
        assert_eq!(
            models[0]["prediction"]["components"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        for m in models {
            assert_eq!(m["sha256"].as_str().unwrap().len(), 64);
            assert!(m["version"].as_str().unwrap().contains('.'));
            assert!(m["prediction"]["value"].as_f64().unwrap().is_finite());
        }
        let ryder = models
            .iter()
            .find(|m| m["id"] == "collagen.ryder2026.2045")
            .unwrap();
        let s1 = models
            .iter()
            .find(|m| m["id"] == "collagen.spyder.1500_snv_transfer_free")
            .unwrap();
        // S1_R2 never needs a transfer
        assert!(s1["transfer"].is_null());
        if class == "hires" {
            assert_eq!(ryder["transfer"]["id"], "transfer.labspec4.hires_to_std");
            assert_eq!(ryder["transfer"]["version"], "0.2.0");
            assert_eq!(ryder["transfer"]["provisional"], true);
        } else {
            assert!(ryder["transfer"].is_null());
        }
        let white = files
            .iter()
            .find(|f| f["file_name"] == "white.asd")
            .unwrap();
        assert_eq!(white["status"], "not_scored");
        let broken = files
            .iter()
            .find(|f| f["file_name"] == "broken.asd")
            .unwrap();
        assert_eq!(broken["status"], "rejected");
    }
    // text output
    let (code, out, _) = spyder(&[
        "predict",
        d.join("bone.asd").to_str().unwrap(),
        "--class",
        "hires",
        "--plugins",
        p.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(
        out.contains("collagen.spyder.consensus3_median@1.0.0 ["),
        "{out}"
    );
    assert!(out.contains("provisional"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_serial_listed_as_the_other_class_adds_a_gentle_note() {
    let d = tmpdir("serial");
    let spec = AsdSpec {
        serial: 28313, // listed as high-res in instruments.json
        ..Default::default()
    };
    std::fs::write(d.join("a.asd"), spec.build()).unwrap();
    let (code, out, _) = spyder(&[
        "predict",
        d.to_str().unwrap(),
        "--class",
        "std",
        "--json",
        "--plugins",
        plugins().to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    let note = v["files"][0]["class_note"].as_str().unwrap();
    assert!(
        note.contains("28313") && note.contains("High-res"),
        "{note}"
    );
    // the switch wins: no transfer for the standard class
    assert!(v["files"][0]["models"][0]["transfer"].is_null());
    let _ = std::fs::remove_dir_all(&d);
}

/// Codex Phase 1 review, MEDIUM 5: a non-UTF-8 argument must not panic.
#[test]
fn non_utf8_arguments_do_not_panic() {
    #[cfg(windows)]
    let bad: OsString = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0x0062, 0xD800, 0x0061]) // lone surrogate
    };
    #[cfg(unix)]
    let bad: OsString = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'b', 0xff, b'a'])
    };
    for cmd in ["read", "validate"] {
        let (code, _, err) = run(&[OsString::from(cmd), bad.clone()]);
        assert_eq!(code, 1, "{cmd}: {err}");
        assert!(!err.contains("panicked"), "{err}");
    }
    let (code, _, err) = run(&[
        OsString::from("predict"),
        bad.clone(),
        OsString::from("--class"),
        OsString::from("std"),
    ]);
    assert_eq!(code, 1, "{err}");
    let (code, _, err) = run(&[bad]);
    assert_eq!(code, 2, "{err}");
    assert!(!err.contains("panicked"));
}

/// Make `link` point at the directory `target`: a symlink, or on Windows a junction when symlinks need
/// privileges. None if neither is possible here.
fn dir_link(target: &Path, link: &Path) -> Option<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).ok()
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return Some(());
        }
        let ok = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .ok()?
            .status
            .success();
        ok.then_some(())
    }
}

/// Codex Phase 1 review, MEDIUM 4: a recursive walk must not follow directory cycles.
#[test]
fn recursive_walk_survives_a_directory_cycle() {
    let d = tmpdir("cycle");
    let sub = d.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(d.join("a.asd"), AsdSpec::default().build()).unwrap();
    std::fs::write(sub.join("b.asd"), AsdSpec::default().build()).unwrap();
    if dir_link(&d, &sub.join("loop")).is_none() {
        eprintln!(
            "SKIPPED recursive_walk_survives_a_directory_cycle: cannot create a directory link here"
        );
        return;
    }
    let (code, out, err) = spyder(&["read", d.to_str().unwrap(), "--recursive", "--json"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    let names: Vec<&str> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["file_name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["a.asd", "b.asd"], "links are not followed");
    let _ = std::fs::remove_dir(sub.join("loop"));
    let _ = std::fs::remove_dir_all(&d);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

#[test]
fn analyse_gives_verdicts_csv_and_manifest() {
    let d = tmpdir("analyse");
    std::fs::write(d.join("bone.asd"), AsdSpec::default().build()).unwrap();
    let joins = AsdSpec {
        splices: [1000.0, 1830.0],
        ..Default::default()
    };
    std::fs::write(d.join("joins.asd"), joins.build()).unwrap();
    let csv = d.join("out.csv");
    let p = plugins();
    let (code, out, err) = spyder(&[
        "analyse",
        d.to_str().unwrap(),
        "--class",
        "hires",
        "--profile",
        "isotopes",
        "--json",
        "--csv",
        csv.to_str().unwrap(),
        "--plugins",
        p.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "spyder-bone/analyse");
    assert_eq!(v["profile"], "isotopes");
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    let bone = &files[0];
    let rec = &bone["record"];
    assert_eq!(rec["oracle_version"], "1.0.0");
    assert!(rec["isotopes"]["verdict"].is_string());
    assert!(rec["models"]["cons3"]["value"].is_number());
    assert_eq!(
        rec["stream"]["transfer"],
        "transfer.labspec4.hires_to_std@0.2.0"
    );
    assert_eq!(bone["manifest"]["format"], "spyder-bone/analysis_manifest");
    assert_eq!(
        bone["manifest"]["input_sha256"],
        rec["input"]["input_sha256"]
    );
    assert!(bone["manifest"]["dependencies"].as_object().unwrap().len() > 10);
    // the other-joins file: Unsupported at B1, with the oracle's words
    let j = &files[1]["record"];
    assert_eq!(j["isotopes"]["verdict"], "Unsupported");
    assert_eq!(
        j["checks"]["B1"]["reason"],
        "unsupported: detector joins [1000.0, 1830.0] (only [1000.0, 1800.0] are supported)"
    );
    // CSV: BOM, CRLF, one header and two rows
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with('\u{feff}'));
    assert_eq!(text.matches("\r\n").count(), 3);
    assert!(text
        .lines()
        .next()
        .unwrap()
        .contains("verdict,rule_step,model_verdict"));
    // text output
    let (code, out, _) = spyder(&[
        "analyse",
        d.join("bone.asd").to_str().unwrap(),
        "--class",
        "std",
        "--plugins",
        p.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(
        out.contains("radiocarbon: instrument class Standard"),
        "{out}"
    );
    assert!(out.contains("CONS3"));
    assert_eq!(
        spyder(&["analyse", "x.asd", "--class", "std", "--profile", "dating"]).0,
        2
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Codex Phase 2 HIGH 3: a bundled startup error stops `predict` and `analyse` (exit 1, no scores).
#[test]
fn startup_error_stops_scoring() {
    let d = tmpdir("startup");
    let b = d.join("plugins");
    copy_dir(&plugins(), &b);
    let f = b.join("collagen_2045_ryder2026_n140.spyder-model.json");
    let mut m: Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
    m["golden"]["cases"][0]["expected"]["value"] = serde_json::json!(99.0);
    std::fs::write(&f, serde_json::to_vec(&m).unwrap()).unwrap();
    std::fs::write(d.join("bone.asd"), AsdSpec::default().build()).unwrap();
    for cmd in ["predict", "analyse"] {
        let (code, out, err) = spyder(&[
            cmd,
            d.join("bone.asd").to_str().unwrap(),
            "--class",
            "std",
            "--plugins",
            b.to_str().unwrap(),
        ]);
        assert_eq!(code, 1, "{cmd}: {out}");
        assert!(err.contains("startup error"), "{cmd}: {err}");
        assert!(err.contains("collagen.ryder2026.2045"), "{cmd}: {err}");
        assert!(out.is_empty(), "{cmd} printed scores: {out}");
    }
    let _ = std::fs::remove_dir_all(&d);
}
