//! Acquisition checks B1-B8 on synthetic scans; B6 with made-up gains (the real ones come from Phase 0b).

#![allow(clippy::field_reassign_with_default, clippy::needless_range_loop)] // test fixtures read better this way

mod common;

use common::*;
use spyder_core::n2::N2Set;
use spyder_core::qc::{self, *};
use spyder_core::read::read_bytes;
use spyder_core::status::Status;

fn scan_with(r: &[f64]) -> spyder_core::Scan {
    read_bytes(&AsdSpec::default().with_reflectance(r).build()).unwrap()
}

fn outcome(c: &CheckResult) -> Outcome {
    assert_eq!(
        c.assessment,
        Status::Assessed,
        "{} not assessed: {:?}",
        c.id,
        c.assessment
    );
    c.outcome.unwrap()
}

#[test]
fn clean_bone_passes_everything() {
    let read = read_bytes(&AsdSpec::default().build());
    let scan = read.as_ref().unwrap();
    let n2 = N2Set::compute(&scan.reflectance, &scan.wavelengths_nm);
    let checks = acquisition_checks(&read, Some(&n2));
    let ids: Vec<&str> = checks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["B1", "B2", "B3", "B4", "B5", "B6b", "B7", "B8"]);
    for c in &checks {
        assert_eq!(outcome(c), Outcome::Pass, "{}: {:?}", c.id, c.values);
        assert!(c.message.is_none());
    }
    assert_eq!(worst_outcome(&checks), Outcome::Pass);
}

#[test]
fn b1_failed_read_is_unusable_and_the_rest_not_assessed() {
    let mut spec = AsdSpec::default();
    spec.splices = [1000.0, 1830.0];
    let read = read_bytes(&spec.build());
    let checks = acquisition_checks(&read, None);
    assert_eq!(outcome(&checks[0]), Outcome::Unusable);
    assert!(checks[0]
        .message
        .as_deref()
        .unwrap()
        .contains("unsupported"));
    assert_eq!(checks[0].values["error_kind"], "unsupported");
    for c in &checks[1..] {
        assert!(
            matches!(c.assessment, Status::NotAssessed { .. }),
            "{}",
            c.id
        );
        assert!(c.outcome.is_none());
    }
}

#[test]
fn b2_saturation() {
    let mut spec = AsdSpec::default();
    spec.sample[idx(1500.0)] = 65_000.0;
    let s = read_bytes(&spec.build()).unwrap();
    assert_eq!(outcome(&qc::b2_saturation(&s)), Outcome::Unusable);
    // only in the VNIR: Note
    let mut spec = AsdSpec::default();
    spec.reference[idx(500.0)] = 65_535.0;
    let s = read_bytes(&spec.build()).unwrap();
    assert_eq!(outcome(&qc::b2_saturation(&s)), Outcome::Note);
    // flat-topped maxima: 3 identical consecutive values at the SWIR1 maximum
    let mut spec = AsdSpec::default();
    let m = spec.sample[idx(1001.0)..=idx(1800.0)]
        .iter()
        .copied()
        .fold(f64::MIN, f64::max);
    for i in idx(1300.0)..idx(1303.0) {
        spec.sample[i] = m + 100.0;
    }
    let s = read_bytes(&spec.build()).unwrap();
    let c = qc::b2_saturation(&s);
    assert_eq!(outcome(&c), Outcome::Unusable);
    assert_eq!(c.values["saturated_channels_1000_2450"], 3);
    // two identical maxima are not a flat top
    let mut spec = AsdSpec::default();
    for i in idx(1300.0)..idx(1302.0) {
        spec.sample[i] = m + 100.0;
    }
    let s = read_bytes(&spec.build()).unwrap();
    assert_eq!(outcome(&qc::b2_saturation(&s)), Outcome::Pass);
}

#[test]
fn b3_impossible_reflectance() {
    let mut r = bone_reflectance();
    r[idx(1200.0)] = -0.001;
    let c = qc::b3_impossible_reflectance(&scan_with(&r));
    assert_eq!(outcome(&c), Outcome::Unusable);
    assert_eq!(c.values["first_nm"], 1200.0);
    // below 1000 nm (UV noise) it does not count
    let mut r = bone_reflectance();
    r[idx(400.0)] = -0.001;
    assert_eq!(
        outcome(&qc::b3_impossible_reflectance(&scan_with(&r))),
        Outcome::Pass
    );
}

#[test]
fn b4_panel_or_empty_probe() {
    // panel-like but not a reference save: mean 0.97, SD ~0.02
    let r: Vec<f64> = wl()
        .iter()
        .map(|w| 0.97 + 0.028 * (w / 50.0).sin())
        .collect();
    let s = scan_with(&r);
    assert_eq!(s.kind, spyder_core::ScanKind::Sample);
    assert_eq!(outcome(&qc::b4_panel_or_empty(&s)), Outcome::Unusable);
    // empty probe
    let s = scan_with(&vec![0.02; N]);
    assert_eq!(outcome(&qc::b4_panel_or_empty(&s)), Outcome::Unusable);
    // a reference save is excepted
    let mut spec = AsdSpec::default();
    spec.sample = spec.reference.clone();
    let s = read_bytes(&spec.build()).unwrap();
    let c = qc::b4_panel_or_empty(&s);
    assert_eq!(
        c.assessment,
        Status::not_assessed("reference scan (not scored)")
    );
}

#[test]
fn b5_dark_spot() {
    let s = scan_with(&vec![0.05; N]);
    assert_eq!(outcome(&qc::b5_dark_spot(&s)), Outcome::Note);
    assert_eq!(outcome(&qc::b4_panel_or_empty(&s)), Outcome::Pass);
    let s = scan_with(&vec![0.09; N]);
    assert_eq!(outcome(&qc::b5_dark_spot(&s)), Outcome::Pass);
}

#[test]
fn b7_above_one_is_a_note_only() {
    let mut r = bone_reflectance();
    r[idx(1500.0)] = 1.2;
    let c = qc::b7_above_one(&scan_with(&r));
    assert_eq!(outcome(&c), Outcome::Note);
    // outside 1001-1800 nm it does not count
    let mut r = bone_reflectance();
    r[idx(1000.0)] = 1.2;
    r[idx(2000.0)] = 1.2;
    assert_eq!(outcome(&qc::b7_above_one(&scan_with(&r))), Outcome::Pass);
}

#[test]
fn b8_splice_step() {
    // SWIR2 shifted by -8% of R relative to SWIR1: Note; -20%: Check
    for (shift, want) in [
        (0.0, Outcome::Pass),
        (-0.08, Outcome::Note),
        (-0.20, Outcome::Check),
    ] {
        let r: Vec<f64> = wl()
            .iter()
            .map(|&w| if w > 1800.0 { 0.4 * (1.0 + shift) } else { 0.4 })
            .collect();
        let s = scan_with(&r);
        let c = qc::b8_splice_step(&s);
        assert_eq!(outcome(&c), want, "{:?}", c.values);
        let step2 = c.values["step2_pct"].as_f64().unwrap();
        assert!((step2 - 100.0 * shift.abs()).abs() < 1e-9, "{step2}");
    }
    // the lower join, from the SWIR1 side
    let r: Vec<f64> = wl()
        .iter()
        .map(|&w| if w <= 1000.0 { 0.46 } else { 0.4 })
        .collect();
    let c = qc::b8_splice_step(&scan_with(&r));
    assert_eq!(outcome(&c), Outcome::Note);
    assert!((c.values["step1_pct"].as_f64().unwrap() - 15.0).abs() < 1e-9);
    assert!(c.message.unwrap().contains("1000"));
}

#[test]
fn b8_line_fit_matches_closed_form() {
    // a straight line extrapolates exactly
    let y = [1.0, 1.5, 2.0, 2.5, 3.0];
    assert!((line_fit_eval(&y, -1.0) - 0.5).abs() < 1e-15);
    assert!((line_fit_eval(&y, 5.0) - 3.5).abs() < 1e-15);
}

fn made_up_gains() -> NoiseGains {
    NoiseGains {
        check_above_pct: 0.5,
        entries: vec![
            NoiseGain {
                model_id: "m.single".into(),
                transfer: "none".into(),
                terms: vec![GainTerm {
                    n2_window_nm: (2000.0, 2100.0),
                    gain: 0.01,
                }],
            },
            NoiseGain {
                model_id: "m.single".into(),
                transfer: "abc123".into(),
                terms: vec![GainTerm {
                    n2_window_nm: (2000.0, 2100.0),
                    gain: 0.005,
                }],
            },
            NoiseGain {
                model_id: "m.two".into(),
                transfer: "none".into(),
                terms: vec![
                    GainTerm {
                        n2_window_nm: (1500.0, 1550.0),
                        gain: 0.03,
                    },
                    GainTerm {
                        n2_window_nm: (2000.0, 2100.0),
                        gain: 0.04,
                    },
                ],
            },
        ],
    }
}

fn n2s(swir2: f64, swir1: f64) -> N2Set {
    N2Set {
        n2_2000_2100: Some(swir2),
        n2_1500_1600: Some(swir1),
        n2_1500_1550: Some(swir1),
        n2_2300_2400: None,
    }
}

#[test]
fn b6_with_made_up_gains() {
    let g = made_up_gains();
    // 0.01 * 40 = 0.4 -> pass; 0.01 * 60 = 0.6 -> Check (never Unusable)
    let c = qc::b6_noise(&g, "m.single", "none", &n2s(40.0, 10.0));
    assert_eq!(outcome(&c), Outcome::Pass);
    let c = qc::b6_noise(&g, "m.single", "none", &n2s(60.0, 10.0));
    assert_eq!(outcome(&c), Outcome::Check);
    assert!((c.values["implied_sd_pct"].as_f64().unwrap() - 0.6).abs() < 1e-12);
    assert!(c.message.unwrap().contains("0.6%"));
    let c = qc::b6_noise(&g, "m.single", "none", &n2s(1e6, 10.0));
    assert_eq!(outcome(&c), Outcome::Check);
    // keyed by transfer: the transferred stream has its own gain
    let c = qc::b6_noise(&g, "m.single", "abc123", &n2s(60.0, 10.0));
    assert_eq!(outcome(&c), Outcome::Pass);
    // no entry for this model/transfer: not assessed, not "pass"
    let c = qc::b6_noise(&g, "m.single", "zzz", &n2s(60.0, 10.0));
    assert!(matches!(c.assessment, Status::NotAssessed { .. }));
    assert!(c.outcome.is_none());
    // two-window model: sqrt((0.03*10)^2 + (0.04*10)^2) = 0.5 -> not above 0.5
    let c = qc::b6_noise(&g, "m.two", "none", &n2s(10.0, 10.0));
    assert!((c.values["implied_sd_pct"].as_f64().unwrap() - 0.5).abs() < 1e-12);
    assert_eq!(outcome(&c), Outcome::Pass);
    // a needed N2 missing: not assessed
    let mut n = n2s(10.0, 10.0);
    n.n2_1500_1550 = None;
    assert!(matches!(
        qc::b6_noise(&g, "m.two", "none", &n).assessment,
        Status::NotAssessed { .. }
    ));
    // consensus: 1.2 x median of (0.2, 0.4, 0.5) = 0.48 -> pass; with N2 x 1.1 -> 0.528 -> Check
    let c = qc::b6_consensus(
        &g,
        "cons",
        &["m.single", "m.single", "m.two"],
        "none",
        1.2,
        &n2s(20.0, 10.0),
    );
    // components: 0.2, 0.2, sqrt(0.09+0.64)=0.854 -> median 0.2 -> 0.24
    assert!((c.values["implied_sd_pct"].as_f64().unwrap() - 0.24).abs() < 1e-12);
    assert_eq!(outcome(&c), Outcome::Pass);
    let c = qc::b6_consensus(
        &g,
        "cons",
        &["m.single", "m.two"],
        "none",
        1.2,
        &n2s(50.0, 1.0),
    );
    // 0.5 and sqrt(0.0009 + 4) = 2.000225 -> median 1.2501125 -> x1.2 = 1.500135
    assert_eq!(outcome(&c), Outcome::Check);
    let c = qc::b6_consensus(
        &g,
        "cons",
        &["m.single", "nope"],
        "none",
        1.2,
        &n2s(50.0, 1.0),
    );
    assert!(matches!(c.assessment, Status::NotAssessed { .. }));
}

#[test]
fn b6b_gates_the_signs() {
    let c = qc::b6b_signs_noise(&n2s(60.0, 1.0), B6B_N2_CUT);
    assert_eq!(outcome(&c), Outcome::Pass);
    assert_eq!(c.values["signs_gated"], false);
    let c = qc::b6b_signs_noise(&n2s(60.0001, 1.0), B6B_N2_CUT);
    assert_eq!(outcome(&c), Outcome::Note);
    assert_eq!(c.values["signs_gated"], true);
    let mut n = n2s(1.0, 1.0);
    n.n2_2000_2100 = None;
    assert!(matches!(
        qc::b6b_signs_noise(&n, 60.0).assessment,
        Status::NotAssessed { .. }
    ));
}

#[test]
fn reference_saves_are_not_scored() {
    let mut spec = AsdSpec::default();
    spec.sample = spec.reference.clone();
    let read = read_bytes(&spec.build());
    let checks = acquisition_checks(&read, None);
    for c in &checks {
        match c.id.as_str() {
            "B1" | "B2" => assert!(c.assessment.is_assessed()),
            _ => assert_eq!(
                c.assessment,
                Status::not_assessed("reference scan (not scored)"),
                "{}",
                c.id
            ),
        }
    }
}

#[test]
fn check_results_serialise_with_separate_status() {
    let c = qc::b5_dark_spot(&scan_with(&vec![0.05; N]));
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["assessment"]["status"], "assessed");
    assert_eq!(v["outcome"], "note");
    let c = qc::b6_noise(&made_up_gains(), "x", "none", &n2s(1.0, 1.0));
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["assessment"]["status"], "not_assessed");
    assert!(v["assessment"]["reason"]
        .as_str()
        .unwrap()
        .contains("no noise gain"));
    assert!(v["outcome"].is_null());
}
