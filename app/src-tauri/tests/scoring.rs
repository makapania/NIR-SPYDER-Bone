//! The app's scoring path (session -> spyder-core -> `ScanResult`) against `spyder analyse`.
//!
//! Public (always): the 54 end-to-end oracle golden scans are put through
//! the app's session for both instrument classes and all three analysis types; every mapped verdict, rule step,
//! shown note, flag, evidence level and ZooMS pattern equals the record `spyder analyse` produces (the same
//! `Engine` call), which the core's own gate holds equal to the frozen oracle. The display arrays are checked
//! against the records: the band readings of the drawn evidence kernel equal the record's E, and the
//! OH-corrected close-ups imply exactly the CONS3 components' readings. The OH/water directions shipped for
//! those close-ups are re-derived from the public mmc2 fixture. The export equals the core's CSV.
//!
//! Private (only with SPYDER_PRIVATE_HR40): a private 40-scan high-res set with the High-res switch, flipped to
//! Standard and back, against `spyder analyse --class hires|std`. Counts only; no specimen name is printed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use spyder_bone_app_lib::display::VIEW_ORDER;
use spyder_bone_app_lib::engine::{bundled_dir, Core};
use spyder_bone_app_lib::mapping::{ui_note_key, ui_verdict, ScanResult};
use spyder_bone_app_lib::scoring;
use spyder_bone_app_lib::session::{Input, NewScan, Session};
use spyder_core::pipeline::export::{csv_row, csv_text};
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::{Context, ANALYSES};
use spyder_core::plugins::{CLASS_HIRES, CLASS_STD};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn b64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

fn core() -> Core {
    let c = Core::load(&bundled_dir(None), None);
    assert!(c.error.is_none(), "{:?}", c.error);
    c
}

fn wl() -> Vec<f64> {
    (0..2151).map(|i| 350.0 + i as f64).collect()
}

struct Case {
    name: String,
    input: Input,
    ctx: Value,
}

fn golden_cases() -> Vec<Case> {
    let gdir = repo().join("reference/oracle/goldens");
    let e: Value =
        serde_json::from_slice(&std::fs::read(gdir.join("oracle_public_v1.json")).unwrap())
            .unwrap();
    let sp: Value = serde_json::from_slice(
        &std::fs::read(gdir.join(e["spectra_file"].as_str().unwrap())).unwrap(),
    )
    .unwrap();
    let spectra: BTreeMap<String, Vec<f64>> = sp["spectra"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["sha256"].as_str().unwrap().to_string(),
                s["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect(),
            )
        })
        .collect();
    e["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let ctx = c["context"].clone();
            let input = match c["asd_b64"].as_str() {
                Some(b) => Input::Asd(b64(b).into()),
                None => Input::Spectrum {
                    wl: wl(),
                    r: spectra[c["spectrum_sha256"].as_str().unwrap()].clone(),
                    joins: ctx["splices_nm"]
                        .as_array()
                        .map(|a| a.iter().map(|v| v.as_f64().unwrap()).collect())
                        .unwrap_or(vec![1000.0, 1800.0]),
                    serial: ctx["serial"].as_u64(),
                },
            };
            Case {
                name: c["name"].as_str().unwrap().to_string(),
                input,
                ctx,
            }
        })
        .collect()
}

/// What `spyder analyse` computes for one input (the same Engine calls as `analyse_cmd.rs`).
fn direct(core: &Core, case: &Case, class: &str) -> Obj {
    let eng = core.engine().unwrap();
    match &case.input {
        Input::Asd(b) => eng.analyse_bytes(b, &format!("{}.asd", case.name), class, "user"),
        Input::Spectrum {
            wl,
            r,
            joins,
            serial,
        } => eng.analyse_spectrum(
            wl,
            r,
            &Context {
                instrument_class: class.to_string(),
                class_source: "user".into(),
                serial: *serial,
                splices_nm: joins.clone(),
            },
        ),
        Input::Unreadable(_) => unreachable!(),
    }
}

fn get<'a>(o: &'a Obj, path: &[&str]) -> Option<&'a V> {
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

fn gs<'a>(o: &'a Obj, path: &[&str]) -> Option<&'a str> {
    get(o, path).and_then(V::as_str)
}

/// Compare one mapped result with the record; returns the mismatches.
fn compare(r: &ScanResult, rec: &Obj, a: &str, label: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let v = gs(rec, &[a, "verdict"]).unwrap_or("");
    match v {
        "Not scored" => {
            if r.scan_kind != "reference" {
                bad.push(format!("{label}: Not scored -> {}", r.scan_kind));
            }
            return bad;
        }
        "Unsupported" => {
            if r.scan_kind != "unreadable" {
                bad.push(format!("{label}: Unsupported -> {}", r.scan_kind));
            }
            return bad;
        }
        _ => {}
    }
    if r.verdict != ui_verdict(v) {
        bad.push(format!("{label}: verdict {} != {v}", r.verdict));
    }
    if r.rule_step != gs(rec, &[a, "rule_step"]).unwrap_or("") {
        bad.push(format!("{label}: rule step {}", r.rule_step));
    }
    // the OH-corrected 1545 nm band (DECISIONS 75): the vote flag is the record's, and a band that cast the
    // ZooMS vote is never drawn flat (the drawn state follows the vote's lit line)
    let vote = get(rec, &["zooms_pattern", "vote_1545"]).is_some_and(V::truthy);
    if r.zooms.vote1545 != vote {
        bad.push(format!("{label}: vote1545 {} != {vote}", r.zooms.vote1545));
    }
    // the ZooMS band check's call (shown in the details; DECISIONS 80 amended) is the record's, after the vote
    if let Some(zv) = gs(rec, &["zooms_pattern", "verdict"]) {
        if r.zooms.verdict != ui_verdict(zv) {
            bad.push(format!(
                "{label}: ZooMS band check {} != {zv}",
                r.zooms.verdict
            ));
        }
    }
    if let Some(b) = r.evidence.bands.iter().find(|b| b.id == "nh1545") {
        let vote_lit = get(rec, &["zooms_pattern", "vote_band", "lit"]).is_some_and(V::truthy);
        if b.lit != vote_lit || (vote && (b.state == "flat" || b.state == "cant_tell")) {
            bad.push(format!(
                "{label}: 1545 band drawn {} (lit {}) vs vote lit {vote_lit}",
                b.state, b.lit
            ));
        }
    }
    let shown: Vec<String> = match get(rec, &[a, "notes_shown"]) {
        Some(V::List(l)) => l.iter().filter_map(V::as_str).map(ui_note_key).collect(),
        _ => Vec::new(),
    };
    if r.notes_shown != shown {
        bad.push(format!(
            "{label}: notes shown {:?} != {shown:?}",
            r.notes_shown
        ));
    }
    let all: Vec<String> = match get(rec, &[a, "notes_all"]) {
        Some(V::List(l)) => l
            .iter()
            .filter_map(|n| n.as_obj().and_then(|o| o.get("key")).and_then(V::as_str))
            .map(ui_note_key)
            .collect(),
        _ => Vec::new(),
    };
    let got: Vec<String> = r
        .notes
        .iter()
        .map(|n| n.key.clone())
        .filter(|k| k != "serial_class_mismatch" && k != "no_transfer_available")
        .collect();
    if got != all {
        bad.push(format!("{label}: notes {got:?} != {all:?}"));
    }
    let nflags = match get(rec, &[a, "flags"]) {
        Some(V::List(l)) => l.len(),
        _ => 0,
    };
    if r.flags.len() != nflags {
        bad.push(format!("{label}: flags"));
    }
    let fired_flagged = r.signs.iter().filter(|s| s.fired).count();
    let flagged: usize = r.flags.iter().map(|f| f.signs.len()).sum();
    if fired_flagged != flagged {
        bad.push(format!(
            "{label}: fired signs {fired_flagged} != flagged {flagged}"
        ));
    }
    if r.verdict != "rescan" {
        let lev = gs(rec, &["evidence", "level"]).unwrap_or("can't tell");
        let want = if lev == "can't tell" {
            "cant_tell"
        } else {
            lev
        };
        if r.evidence.level != want {
            bad.push(format!("{label}: level {} != {lev}", r.evidence.level));
        }
        if r.zooms.pattern.as_deref() != gs(rec, &["zooms_pattern", "pattern"]) {
            bad.push(format!("{label}: pattern"));
        }
        let m = get(rec, &["models", "cons3", "value"]).and_then(V::as_f64);
        if r.models.cons3.as_ref().and_then(|x| x.value) != m.filter(|x| x.is_finite()) {
            bad.push(format!("{label}: CONS3"));
        }
        for (k, key) in [("wc2045", "wc2045"), ("wc1500", "wc1500"), ("F05", "f05")] {
            let want = get(rec, &["models", "cons3", "components", k]).and_then(V::as_f64);
            let gotv = match key {
                "wc2045" => r.models.wc2045.as_ref(),
                "wc1500" => r.models.wc1500.as_ref(),
                _ => r.models.f05.as_ref(),
            }
            .and_then(|x| x.value);
            if gotv != want.filter(|x| x.is_finite()) {
                bad.push(format!("{label}: component {k}"));
            }
        }
    }
    bad
}

#[test]
fn app_scoring_equals_spyder_analyse_on_the_public_goldens() {
    let core = core();
    let cases = golden_cases();
    assert_eq!(cases.len(), 54);
    let (mut bad, mut n) = (Vec::new(), 0usize);
    for class in [CLASS_STD, CLASS_HIRES] {
        let mut s = Session::new(1, Some("goldens".into()), false, Some(class));
        for c in &cases {
            s.add(
                &core,
                NewScan {
                    path: c.name.clone(),
                    file: format!("{}.asd", c.name),
                    input: c.input.clone(),
                    arrived_seq: None,
                    revision: 1,
                    modified_ms: None,
                    log_meta: None,
                },
            );
        }
        for a in ANALYSES {
            let res = scoring::results(&core, &s, a);
            assert_eq!(res.len(), cases.len());
            for (r, c) in res.iter().zip(&cases) {
                let rec = direct(&core, c, class);
                bad.extend(compare(r, &rec, a, &format!("{} {class} {a}", c.name)));
                n += 1;
            }
        }
        // the export is the core's CSV of the same records
        for a in ANALYSES {
            let rows: Vec<Obj> = cases
                .iter()
                .map(|c| csv_row(&direct(&core, c, class), a))
                .collect();
            let (text, k) = scoring::export_csv(&s, a);
            assert_eq!(k, cases.len());
            assert_eq!(text, csv_text(&rows), "{class} {a}: CSV differs");
        }
    }
    eprintln!(
        "app scoring vs spyder analyse: {n} results compared, {} mismatches",
        bad.len()
    );
    // one verdict for every analysis (DECISIONS 80 amended): the app's results carry each of the three ZooMS lines on
    // the public goldens, and never on a Good verdict
    let mut s = Session::new(1, None, false, Some(CLASS_STD));
    for c in &cases {
        s.add(
            &core,
            NewScan {
                path: c.name.clone(),
                file: format!("{}.asd", c.name),
                input: c.input.clone(),
                arrived_seq: None,
                revision: 1,
                modified_ms: None,
                log_meta: None,
            },
        );
    }
    let res = scoring::results(&core, &s, "radiocarbon");
    // (the stronger protein line has no public real spectrum; the profile goldens cover it)
    for k in [
        "zooms_better_good",
        "zooms_faint_protein",
        "zooms_better_1545",
        "zooms_better_1545_flat",
    ] {
        assert!(
            res.iter().any(|r| r.notes.iter().any(|n| n.key == k)),
            "no {k} on the public goldens"
        );
    }
    assert!(res
        .iter()
        .filter(|r| r.verdict == "good")
        .all(|r| r.notes.iter().all(|n| !n.key.starts_with("zooms_"))));
    // the faint sign carries its lit N-type bands as UI band ids (the text names 2175 when it is the only one)
    let faint = res
        .iter()
        .flat_map(|r| r.notes.iter())
        .find(|n| n.key == "zooms_faint_protein")
        .unwrap();
    let lit = faint.params["lit"].as_array().unwrap();
    assert!(
        !lit.is_empty()
            && lit
                .iter()
                .all(|b| ["nh2044", "amide2175", "nh1545"].contains(&b.as_str().unwrap()))
    );
    assert!(bad.is_empty(), "{}", bad[..bad.len().min(30)].join("\n"));
    // the oracle's expected verdicts (the context class) hold through the app too
    for c in &cases {
        let class = c.ctx["instrument_class"].as_str().unwrap();
        let mut s = Session::new(
            1,
            None,
            false,
            Some(if class == CLASS_HIRES {
                CLASS_HIRES
            } else {
                CLASS_STD
            }),
        );
        s.add(
            &core,
            NewScan {
                path: c.name.clone(),
                file: format!("{}.asd", c.name),
                input: c.input.clone(),
                arrived_seq: None,
                revision: 1,
                modified_ms: None,
                log_meta: None,
            },
        );
        let rec = direct(&core, c, class);
        for a in ANALYSES {
            let r = &scoring::results(&core, &s, a)[0];
            assert!(compare(r, &rec, a, &c.name).is_empty(), "{} {a}", c.name);
        }
    }
}

/// Mean of y over centre +/- 2 nm on the 350 nm grid.
fn band(y: &[f64], nm: f64) -> f64 {
    let i = (nm - 350.0) as usize;
    y[i - 2..=i + 2].iter().sum::<f64>() / 5.0
}

#[test]
fn display_arrays_match_what_the_rules_and_models_read() {
    let core = core();
    let reg = core.registry().unwrap();
    let n = 2151;
    let (mut checked, mut ohc) = (0, 0);
    for class in [CLASS_STD, CLASS_HIRES] {
        let mut s = Session::new(1, None, false, Some(class));
        for c in golden_cases() {
            s.add(
                &core,
                NewScan {
                    path: c.name.clone(),
                    file: format!("{}.asd", c.name),
                    input: c.input.clone(),
                    arrived_seq: None,
                    revision: 1,
                    modified_ms: None,
                    log_meta: None,
                },
            );
        }
        for e in &s.entries {
            let Some(rec) = e.record.as_deref() else {
                continue;
            };
            if gs(rec, &["radiocarbon", "verdict"])
                .is_some_and(|v| matches!(v, "Rescan" | "Unsupported" | "Not scored"))
            {
                continue;
            }
            let v = scoring::scan_views(&core, &s, &e.id, 31).unwrap();
            assert_eq!(v.len(), n * VIEW_ORDER.len());
            let view = |k: &str| {
                let j = VIEW_ORDER.iter().position(|x| *x == k).unwrap();
                &v[j * n..(j + 1) * n]
            };
            // the evidence kernel drawn (D2_31) gives exactly the record's band readings
            if let Some(V::Map(zb)) = get(rec, &["zooms_pattern", "bands"]) {
                for (b, d) in &zb.0 {
                    let nm: f64 = b[2..].parse().unwrap();
                    let want = d
                        .as_obj()
                        .and_then(|o| o.get("E"))
                        .and_then(V::as_f64)
                        .unwrap();
                    let got = band(view("D2_31"), nm);
                    assert!(
                        (got - want).abs() <= 1e-9 * want.abs().max(1.0),
                        "{} {b}: {got} vs {want}",
                        e.id
                    );
                    checked += 1;
                }
            }
            // the chart smoothing at 31 is the kernel itself
            assert_eq!(
                view("D2")
                    .to_vec()
                    .iter()
                    .map(|x| x.to_bits())
                    .collect::<Vec<_>>(),
                view("D2_31")
                    .iter()
                    .map(|x| x.to_bits())
                    .collect::<Vec<_>>()
            );
            // the OH-corrected windows imply exactly the CONS3 components' readings
            for (label, comp) in [("2045", "wc2045"), ("1500", "wc1500")] {
                let w = core.display.window(label).expect("OH window available");
                let x: Vec<f64> = w
                    .wavelengths()
                    .iter()
                    .map(|l| view("D2_31_ohc")[(*l - 350.0) as usize] / -1e5)
                    .collect();
                let want = get(rec, &["models", "cons3", "components", comp])
                    .and_then(V::as_f64)
                    .unwrap();
                // the reading the displayed features imply, by the model's own coefficients
                let implied = w.direct(&x);
                assert!(
                    (implied - want).abs() < 1e-6,
                    "{} {label}: implied {implied} vs model {want}",
                    e.id
                );
                // and the OH/water direction is gone from the displayed features
                let m = reg.model(&w.info.model_id).unwrap();
                let c = match &m.body {
                    spyder_core::model::Body::Regression {
                        x_center: Some(c), ..
                    } => c.clone(),
                    _ => unreachable!(),
                };
                let along: f64 = x
                    .iter()
                    .zip(&c)
                    .zip(w.direction())
                    .map(|((a, c), v)| (a - c) * v)
                    .sum();
                assert!(
                    along.abs() < 1e-12,
                    "{label}: residual along the OH direction {along}"
                );
                // outside the windows the corrected view is empty (nothing implied there)
                assert!(view("D2_31_ohc")[(1700.0 - 350.0) as usize].is_nan());
                ohc += 1;
            }
            // standard-res: the as-measured views equal the stream; high-res: they differ (the transfer)
            let same = view("R") == view("R_meas");
            assert_eq!(same, class == CLASS_STD, "{} {class}", e.id);
        }
    }
    eprintln!("display: {checked} band readings and {ohc} OH-corrected windows checked against the records");
    assert!(checked > 300 && ohc > 100);
    // the references: five levels, eight views each, corrected windows present
    let (meta, refs) = scoring::reference_views(&core, 31);
    assert_eq!(meta.len(), 5);
    assert_eq!(refs.len(), 5 * VIEW_ORDER.len() * n);
    assert_eq!(meta[1].legend, "1%");
    let ohc_ref = &refs[4 * n..5 * n];
    assert!(ohc_ref[2045 - 350].is_finite() && ohc_ref[1525 - 350].is_finite());
}

/// The shipped OH/water direction of the 2045 window, re-derived from public data only: the mmc2 reference
/// bones' window features and the model's folded coefficients (v = unit(g - (g.b / b.b) b)).
#[test]
fn shipped_oh_direction_is_recoverable_from_public_data() {
    use spyder_core::preprocess::{absorbance, savgol, SgMode, SgParams, Spectrum};
    let core = core();
    let w = core.display.window("2045").expect("2045 window");
    let reg = core.registry().unwrap();
    let m = reg.model(&w.info.model_id).unwrap();
    let b = match &m.body {
        spyder_core::model::Body::Regression { coefficients, .. } => coefficients.clone(),
        _ => unreachable!(),
    };
    let fx: Value = serde_json::from_slice(
        &std::fs::read(
            repo().join("crates/spyder-core/tests/fixtures/cal100/mmc2_2045_window.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let start = fx["wl_start_nm"].as_f64().unwrap();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for bone in fx["bones"].as_array().unwrap() {
        let r: Vec<f64> = bone["reflectance"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let wl: Vec<f64> = (0..r.len()).map(|i| start + i as f64).collect();
        let a = Spectrum::new(wl.clone(), absorbance(&r, 1e-6));
        let d = savgol(
            &a,
            &SgParams {
                window: 31,
                polyorder: 3,
                deriv: 2,
                delta: 1.0,
                mode: SgMode::Interp,
            },
        )
        .unwrap();
        xs.push(
            w.wavelengths()
                .iter()
                .map(|l| d.x[(*l - start) as usize])
                .collect::<Vec<f64>>(),
        );
        ys.push(bone["collagen_pct"].as_f64().unwrap());
    }
    let n = xs.len() as f64;
    let p = b.len();
    let ybar = ys.iter().sum::<f64>() / n;
    let xbar: Vec<f64> = (0..p)
        .map(|j| xs.iter().map(|x| x[j]).sum::<f64>() / n)
        .collect();
    let g: Vec<f64> = (0..p)
        .map(|j| {
            xs.iter()
                .zip(&ys)
                .map(|(x, y)| (x[j] - xbar[j]) * (y - ybar))
                .sum()
        })
        .collect();
    let k =
        g.iter().zip(&b).map(|(a, c)| a * c).sum::<f64>() / b.iter().map(|c| c * c).sum::<f64>();
    let mut v: Vec<f64> = g.iter().zip(&b).map(|(gi, bi)| gi - k * bi).collect();
    let nv = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    v.iter_mut().for_each(|x| *x /= nv);
    let cos: f64 = v
        .iter()
        .zip(w.direction())
        .map(|(a, c)| a * c)
        .sum::<f64>()
        .abs();
    eprintln!("2045 OH direction from public mmc2: |cos| with the shipped one = {cos}");
    assert!(cos > 1.0 - 1e-9, "{cos}");
}

/// The private high-res folder (40 scans), named by `SPYDER_PRIVATE_HR40`; the test is skipped without it.
/// The Ryder 2045 ZooMS line (Matt; phase 0d REPORT_zooms_models) through the app: its params are the published model's
/// reading the details panel shows (same value, same transfer: as measured on Standard, transfer v0.2 on High-res) and the
/// profile's cut; and it clears when the scan is rescored with an input it no longer applies to.
#[test]
fn ryder_line_maps_the_displayed_reading_and_clears_on_rescore() {
    let core = core();
    let cases = golden_cases();
    let by = |n: &str| cases.iter().find(|c| c.name == n).unwrap();
    let (ryder, plain) = (by("zooms_ryder_line"), by("zooms_pattern_B"));
    let new = |c: &Case, revision: u32| NewScan {
        path: "scan.asd".into(),
        file: "scan.asd".into(),
        input: c.input.clone(),
        arrived_seq: None,
        revision,
        modified_ms: None,
        log_meta: None,
    };
    for class in [CLASS_STD, CLASS_HIRES] {
        let mut s = Session::new(1, None, false, Some(class));
        s.add(&core, new(ryder, 1));
        let r = &scoring::results(&core, &s, "radiocarbon")[0];
        let rec = direct(&core, ryder, class);
        assert_eq!(r.verdict, "unlikely", "{class}");
        let note = r
            .notes
            .iter()
            .find(|n| n.key == "zooms_better_ryder")
            .unwrap_or_else(|| panic!("{class}: no Ryder line"));
        assert_eq!(
            r.notes_shown.first().map(String::as_str),
            Some("zooms_better_ryder")
        );
        let shown = r.models.ryder2045.as_ref().unwrap();
        assert_eq!(
            note.params["r"].as_f64(),
            shown.value,
            "{class}: the line's reading is the one displayed"
        );
        assert_eq!(note.params["at_least"].as_f64(), Some(0.34), "{class}");
        assert!(shown.value.unwrap() >= 0.34);
        let rec_transfer = gs(&rec, &["models", "ryder2045", "transfer"]).map(str::to_string);
        assert_eq!(
            shown.transfer, rec_transfer,
            "{class}: the displayed transfer is the record's"
        );
        if class == CLASS_HIRES {
            assert!(
                shown.transfer.as_deref().unwrap_or("").contains("@0.2"),
                "{:?}",
                shown.transfer
            );
        } else {
            assert_eq!(shown.transfer.as_deref(), Some("none"));
        }
        // the file changes on disk: rescored, the line no longer applies and is gone
        s.add(&core, new(plain, 2));
        let r2 = &scoring::results(&core, &s, "radiocarbon")[0];
        assert_eq!(r2.file_revision, 2);
        assert!(
            r2.notes.iter().all(|n| n.key != "zooms_better_ryder"),
            "{class}: {:?}",
            r2.notes
        );
        assert!(r2.models.ryder2045.as_ref().unwrap().value.unwrap() < 0.34);
    }
}

fn private_folder() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("SPYDER_PRIVATE_HR40")?);
    p.is_dir().then_some(p)
}

fn counts(rs: &[ScanResult]) -> BTreeMap<&'static str, usize> {
    let mut m = BTreeMap::new();
    for r in rs {
        *m.entry(r.verdict).or_insert(0) += 1;
    }
    m
}

/// `spyder analyse --class <c> --json` on the folder, if the CLI is built: verdicts by file name.
fn cli_verdicts(folder: &Path, class: &str, profile: &str) -> Option<BTreeMap<String, String>> {
    // the CLI built alongside this test (honours CARGO_TARGET_DIR), reading the repo's plug-ins so a stale bundled
    // copy next to an old binary can't be compared (LESSONS 41)
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { repo().join(p) })
        .unwrap_or_else(|| repo().join("target"));
    let exe = target
        .join("debug")
        .join(format!("spyder{}", std::env::consts::EXE_SUFFIX));
    if !exe.is_file() {
        return None;
    }
    let out = std::process::Command::new(exe)
        .arg("analyse")
        .arg(folder)
        .args([
            "--class",
            class,
            "--profile",
            profile,
            "--json",
            "--plugins",
        ])
        .arg(repo().join("plugins"))
        .output()
        .ok()?;
    let doc: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(
        doc["files"]
            .as_array()?
            .iter()
            .map(|f| {
                (
                    f["record"]["input"]["file"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    f["record"][profile]["verdict"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                )
            })
            .collect(),
    )
}

#[test]
fn private_high_res_40_session() {
    let Some(folder) = private_folder() else {
        eprintln!("SKIPPED: set SPYDER_PRIVATE_HR40 to the private 40-scan high-res folder to run this test");
        return;
    };
    let core = core();
    let files = scoring::folder_files(&folder).unwrap();
    assert_eq!(files.len(), 40);
    let mut s = Session::new(1, Some(folder.to_string_lossy().into_owned()), false, None);
    s.set_class(&core, CLASS_HIRES);
    scoring::add_files(&core, &mut s, &files);
    let check = |s: &Session, cls: &str, cli: &str| {
        for a in ANALYSES {
            let rs = scoring::results(&core, s, a);
            // each result equals the engine run `spyder analyse` makes
            for (r, e) in rs.iter().zip(&s.entries) {
                let bytes = std::fs::read(&e.path).unwrap();
                let rec = core.analyse_bytes(&bytes, &e.file, cls, "user").unwrap();
                let bad = compare(r, &rec, a, "private scan");
                assert!(bad.is_empty(), "{}", bad.join("\n"));
            }
            if let Some(cv) = cli_verdicts(&folder, cli, a) {
                for r in &rs {
                    assert_eq!(ui_verdict(&cv[&r.file]), r.verdict, "CLI vs app ({a})");
                }
                eprintln!("{cli} {a}: the CLI's 40 verdicts equal the app's");
            }
            eprintln!("{cli} {a}: {:?}", counts(&rs));
        }
    };
    check(&s, CLASS_HIRES, "hires");
    let rc = counts(&scoring::results(&core, &s, "radiocarbon"));
    assert_eq!(
        (rc.get("good"), rc.get("borderline"), rc.get("unlikely")),
        (Some(&32), Some(&7), Some(&1))
    );
    s.set_class(&core, CLASS_STD);
    check(&s, CLASS_STD, "std");
    s.set_class(&core, CLASS_HIRES);
    let back = counts(&scoring::results(&core, &s, "radiocarbon"));
    assert_eq!(back, rc, "flipping back restores the High-res verdicts");
    let mean_ms = s.entries.iter().map(|e| e.score_ms).sum::<f64>() / s.entries.len() as f64;
    eprintln!("analysis time per scan (this build): mean {mean_ms:.1} ms");
}
