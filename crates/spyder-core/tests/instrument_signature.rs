//! An instrument is known by its file format AND its detector joins, never by the format alone (DECISIONS 137,
//! instruments.json 2.1.0). as7 is ASD's file version 7: the Trek-type handheld writes it (blank header joins, the
//! SWIR1/SWIR2 join found in the white-reference DN at 1785 nm), and a LabSpec 4 on older ASD software may write it
//! (joins 1000/1800, as on every LabSpec 4 as8 file). An as7 file with the LabSpec joins is scored exactly like the
//! same scan in as8: the switch decides its class, no uncalibrated note. Any other as7 file stays the Trek-type
//! class, as does one whose joins could not be found (a fallback says nothing about the instrument). The same cases
//! run against the oracle in reference/tests/test_instrument_signature.py. Synthetic files on a public spectrum.

#![allow(clippy::field_reassign_with_default)]

mod common;

use std::path::PathBuf;

use common::{reference_dn, wl, AsdSpec};
use serde_json::Value;
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::Engine;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};
use spyder_core::plugins::tables::parse_instruments;
use spyder_core::plugins::{Node, CLASS_HIRES, CLASS_STD};

const TREK: &str = "asd.trek";
const FS3: &str = "asd.fieldspec3";

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
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

/// A public bone spectrum (JAS mmc2 reference sample 44, as the end-to-end goldens use).
fn bone() -> Vec<f64> {
    let g: Value = serde_json::from_slice(
        &std::fs::read(plugins().join("golden_spectra_public_v2.json")).unwrap(),
    )
    .unwrap();
    g["spectra"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "mmc2_Reference_Sample_44")
        .unwrap()["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

/// The white reference with its detector steps after `j1` and `j2` nm (common::reference_dn has them at 1000/1800).
fn reference_with_steps(j1: f64, j2: f64) -> Vec<f64> {
    let flat: Vec<f64> = wl()
        .iter()
        .map(|&w| 20_000.0 + 8_000.0 * (-(((w - 1300.0) / 600.0).powi(2))).exp())
        .collect();
    wl().iter()
        .zip(flat)
        .map(|(&w, b)| {
            if w <= j1 {
                b * 0.8
            } else if w <= j2 {
                b
            } else {
                b * 0.6
            }
        })
        .collect()
}

/// The scan (a public bone spectrum) with the given header joins and white reference.
fn spec(header_joins: [f32; 2], reference: Vec<f64>) -> AsdSpec {
    let mut s = AsdSpec::default();
    s.reference = reference;
    s.splices = header_joins;
    s.with_reflectance(&bone())
}

/// The same scan as as7, written as the Trek-type files are (dark flag 0) with the header joins of `s`.
fn as7(s: &AsdSpec) -> Vec<u8> {
    let mut t = s.clone();
    t.dark_corrected = 0;
    t.build_as7(1)
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

fn s<'a>(o: &'a Obj, path: &[&str]) -> &'a str {
    at(o, path).as_str().unwrap()
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

#[test]
fn the_registry_names_as7_by_its_detector_joins() {
    let reg = shipped();
    let ins = reg.instruments().expect("instrument registry");
    assert_eq!(ins.header.version.to_string(), "2.1.0");
    // the LabSpec joins: a LabSpec 4 file, whatever the format (the switch decides)
    assert_eq!(ins.class_of_file("as7", Some(&[1000.0, 1800.0])), None);
    assert_eq!(ins.class_of_file("as8", Some(&[1000.0, 1800.0])), None);
    // the Trek-type signature, and any other joins found in an as7 file: the Trek-type class
    assert_eq!(
        ins.class_of_file("as7", Some(&[1000.0, 1785.0])),
        Some(TREK)
    );
    assert_eq!(
        ins.class_of_file("as7", Some(&[1000.0, 1830.0])),
        Some(TREK)
    );
    assert_eq!(
        ins.class_of_file("as7", Some(&[1001.0, 1800.0])),
        Some(TREK)
    );
    // no joins found (the reader's 1000/1800 fallback): nothing says LabSpec, so the format's class stands
    assert_eq!(ins.class_of_file("as7", None), Some(TREK));
    // as8 is never another instrument's; legacy v1 is FieldSpec 3's whatever its joins
    assert_eq!(ins.class_of_file("as8", Some(&[1000.0, 1785.0])), None);
    assert_eq!(ins.class_of_file("as8", None), None);
    assert_eq!(
        ins.class_of_file("legacy", Some(&[1000.0, 1800.0])),
        Some(FS3)
    );
    assert_eq!(ins.class_of_file("legacy", None), Some(FS3));
}

#[test]
fn unless_joins_must_be_numbers_and_come_with_a_file_format() {
    let doc: Value =
        serde_json::from_slice(&std::fs::read(plugins().join("instruments.json")).unwrap())
            .unwrap();
    assert!(parse_instruments(&Node::root(&doc)).is_ok());
    let with = |hint: Value| {
        let mut d = doc.clone();
        d["classes"][TREK]["header_hint"] = hint;
        parse_instruments(&Node::root(&d))
            .err()
            .map(|e| e.to_string())
    };
    let e = with(serde_json::json!({"unless_joins_nm": [1000, 1800]})).expect("refused");
    assert!(e.contains("unless_joins_nm needs file_format"), "{e}");
    let e =
        with(serde_json::json!({"file_format": ["as7"], "unless_joins_nm": []})).expect("refused");
    assert!(e.contains("at least one join"), "{e}");
    assert!(
        with(serde_json::json!({"file_format": ["as7"], "unless_joins_nm": ["1000"]})).is_some()
    );
    assert!(with(serde_json::json!({"file_format": ["as7"]})).is_none());
}

#[test]
fn an_as7_file_with_the_labspec_joins_is_scored_exactly_like_as8() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let sp = spec([1000.0, 1800.0], reference_dn());
    let (b7, b8) = (as7(&sp), sp.build());
    for class in [CLASS_STD, CLASS_HIRES] {
        let r7 = eng.analyse_bytes(&b7, "a.asd", class, "user");
        let r8 = eng.analyse_bytes(&b8, "a.asd", class, "user");
        assert_eq!(s(&r7, &["input", "format"]), "as7");
        assert!(r7
            .get("input")
            .unwrap()
            .as_obj()
            .unwrap()
            .get("joins_source")
            .is_none());
        assert_eq!(s(&r7, &["instrument", "class"]), class);
        assert_eq!(s(&r7, &["instrument", "class_source"]), "user");
        assert!(!note_keys(&r7).contains(&"instrument_uncalibrated".to_string()));
        // every block but the input's own fields (format, content hash) is the as8 record's
        let keys = |r: &Obj| r.0.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&r7), keys(&r8));
        for (k, v) in &r8.0 {
            if k != "input" {
                assert_eq!(r7.get(k), Some(v), "{class}: {k}");
            }
        }
    }
    // the high-res switch reads it through the high-res correction, as it does an as8 file
    let r = eng.analyse_bytes(&b7, "a.asd", CLASS_HIRES, "user");
    assert_ne!(s(&r, &["stream", "transfer"]), "none");
    // a blank header whose white reference shows the LabSpec joins: found joins 1000/1800, a LabSpec file too
    let blank = as7(&spec([0.0, 0.0], reference_dn()));
    let r = eng.analyse_bytes(&blank, "b.asd", CLASS_STD, "user");
    assert_eq!(s(&r, &["input", "joins_source"]), "reference_dn");
    assert_eq!(s(&r, &["instrument", "class"]), CLASS_STD);
    assert_eq!(s(&r, &["instrument", "class_source"]), "user");
}

#[test]
fn the_trek_signature_and_other_as7_joins_keep_the_trek_class() {
    let reg = shipped();
    let eng = Engine::new(&reg).unwrap();
    let cases = [
        // the Trek-type files: blank header, the SWIR1/SWIR2 join found in the white reference at 1785 nm
        (
            "trek",
            spec([0.0, 0.0], reference_with_steps(1000.0, 1785.0)),
            "reference_dn",
        ),
        // other plausible header joins (the FieldSpec 3 manual's 1830): not the LabSpec joins
        ("other", spec([1000.0, 1830.0], reference_dn()), "header"),
        // no joins found anywhere: the 1000/1800 fallback is not evidence of a LabSpec 4
        ("none", spec([0.0, 0.0], vec![20_000.0; 2151]), "default"),
    ];
    for (tag, sp, source) in cases {
        for class in [CLASS_STD, CLASS_HIRES] {
            let r = eng.analyse_bytes(&as7(&sp), "t.asd", class, "user");
            assert_eq!(s(&r, &["instrument", "class"]), TREK, "{tag} {class}");
            assert_eq!(
                s(&r, &["instrument", "class_source"]),
                "file_format",
                "{tag}"
            );
            assert!(
                note_keys(&r).contains(&"instrument_uncalibrated".to_string()),
                "{tag}"
            );
            assert_eq!(s(&r, &["stream", "transfer"]), "none", "{tag}");
            let src = r
                .get("input")
                .unwrap()
                .as_obj()
                .unwrap()
                .get("joins_source")
                .and_then(V::as_str)
                .unwrap_or("header");
            assert_eq!(src, source, "{tag}");
        }
    }
    // the same Trek-type scan in as8 is a LabSpec file (as8 is never another instrument's)
    let sp = spec([0.0, 0.0], reference_with_steps(1000.0, 1785.0));
    let r = eng.analyse_bytes(&sp.build(), "t8.asd", CLASS_STD, "user");
    assert_eq!(s(&r, &["instrument", "class"]), CLASS_STD);
}
