//! DECISIONS 137 Phase 2: the computed noise and band-readability gains equal the frozen Python reference
//! (reference/spyder_ref/gains.py) within 1e-9 relative, and every output that has no gain carries the same status
//! text, for every shipped model and readable band, and every linear statistic of the checks (one detector term per N2
//! window; defined in the fixture), through ten corrections on ten scan layouts (tests/goldens/gains_parity_v1.json,
//! written by tests/gen/gen_gains_parity.py). Also: the computed gains for the
//! shipped transfer files round to the gains format 1 recorded per transfer file (tests/goldens/recorded_gains_v1.json).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;
use spyder_core::model::parse_model;
use spyder_core::plugins::gains::{
    band_gain, model_terms, parse_instrument_noise, stat_term_gain, Correction, InstrumentNoise,
    LinearStat,
};
use spyder_core::plugins::tables::{parse_bands, parse_noise_gains, parse_sg, Kernel};
use spyder_core::plugins::{sha256_hex, Node};
use spyder_core::preprocess::{parse_chain, Op};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json(p: PathBuf) -> Value {
    serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .unwrap()
}

struct Corr {
    ops: Vec<Op>,
    noise: InstrumentNoise,
}

fn corrections(v: &Value) -> BTreeMap<String, Option<Corr>> {
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, c)| {
            let c = (!c.is_null()).then(|| Corr {
                ops: parse_chain(&c["operator"]).unwrap(),
                noise: c
                    .get("instrument_noise")
                    .map(|n| parse_instrument_noise(&Node::root(n)).unwrap())
                    .unwrap_or_default(),
            });
            (k.clone(), c)
        })
        .collect()
}

/// A linear statistic as the fixture writes it ({kernel: {absorbance_clip, savgol, scale}, windows: [[lo, hi, w]]}).
fn stat(v: &Value) -> LinearStat {
    let k = &v["kernel"];
    let (savgol, mode_stated) = parse_sg(&Node::root(&k["savgol"])).unwrap();
    LinearStat {
        kernel: Kernel {
            absorbance_clip_min: k["absorbance_clip"].as_f64().unwrap(),
            savgol,
            mode_stated,
            scale: k["scale"].as_f64().unwrap(),
        },
        windows: v["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| {
                let w: Vec<f64> = w
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect();
                (w[0], w[1], w[2])
            })
            .collect(),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1e-300)
}

#[test]
fn computed_gains_match_the_python_reference() {
    let fx = json(repo().join("crates/spyder-core/tests/goldens/gains_parity_v1.json"));
    let plugins = repo().join("plugins");
    // the fixture must describe the shipped files (rerun gen_gains_parity.py after any change)
    for (f, sha) in fx["plugin_files"].as_object().unwrap() {
        let got = sha256_hex(&std::fs::read(plugins.join(f)).unwrap());
        assert_eq!(
            &got,
            sha.as_str().unwrap(),
            "{f} changed: rerun tests/gen/gen_gains_parity.py"
        );
    }
    let ng = parse_noise_gains(&Node::root(&json(plugins.join("noise_gains.json")))).unwrap();
    let bd = parse_bands(&Node::root(&json(plugins.join("bands.json")))).unwrap();
    let corrs = corrections(&fx["corrections"]);
    let stats: BTreeMap<String, LinearStat> = fx["statistics"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), stat(v)))
        .collect();
    let mut n_stat = 0;
    let mut models = BTreeMap::new();
    for f in fx["plugin_files"].as_object().unwrap().keys() {
        if f.ends_with(".spyder-model.json") {
            models.insert(
                f.clone(),
                parse_model(&Node::root(&json(plugins.join(f)))).unwrap(),
            );
        }
    }
    let layouts: BTreeMap<String, (Vec<f64>, Vec<f64>)> = fx["layouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            let (s, d, n) = (
                l["wl_start"].as_f64().unwrap(),
                l["wl_step"].as_f64().unwrap(),
                l["n"].as_u64().unwrap() as usize,
            );
            // the same grids as numpy arange (exact for these integer and half-integer steps)
            let wl: Vec<f64> = (0..n).map(|i| s + d * i as f64).collect();
            let joins: Vec<f64> = l["joins"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            (l["name"].as_str().unwrap().to_string(), (wl, joins))
        })
        .collect();
    let (mut n_gain, mut n_status, mut bad) = (0, 0, Vec::new());
    for e in fx["entries"].as_array().unwrap() {
        let (wl, joins) = &layouts[e["layout"].as_str().unwrap()];
        let cname = e["correction"].as_str().unwrap();
        let corr = corrs[cname].as_ref().map(|c| Correction {
            ops: &c.ops,
            noise: &c.noise,
            key: cname,
        });
        let label = format!(
            "{} / {} / {}",
            e["layout"],
            cname,
            e.get("model")
                .or(e.get("band"))
                .or(e.get("statistic"))
                .unwrap()
        );
        if let Some(sname) = e.get("statistic").and_then(Value::as_str) {
            let w: Vec<f64> = e["window"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            let got = stat_term_gain(&stats[sname], wl, joins, corr, 1.25, (w[0], w[1]), sname);
            let want = &e["result"];
            n_stat += 1;
            match (got, want.get("error")) {
                (Err(g), Some(wt)) => {
                    if g.0 != wt.as_str().unwrap() {
                        bad.push(format!("{label}: status {:?} want {wt}", g.0));
                    }
                }
                (Ok(g), None) => {
                    if !close(g, want["gain"].as_f64().unwrap()) {
                        bad.push(format!("{label} {w:?}: {g:e} want {want}"));
                    }
                }
                (g, wt) => bad.push(format!("{label} {w:?}: got {g:?}, want error {wt:?}")),
            }
            continue;
        }
        if let Some(m) = e.get("model") {
            let want = &e["terms"];
            let got = model_terms(&ng, &models[m.as_str().unwrap()], wl, joins, corr);
            match (got, want.get("error")) {
                (Err(g), Some(w)) => {
                    n_status += 1;
                    if g.0 != w.as_str().unwrap() {
                        bad.push(format!("{label}: status {:?} want {w}", g.0));
                    }
                }
                (Ok(t), None) => {
                    let want = want.as_object().unwrap();
                    if t.len() != want.len() {
                        bad.push(format!(
                            "{label}: {} components, want {}",
                            t.len(),
                            want.len()
                        ));
                        continue;
                    }
                    for (name, terms) in &t {
                        let w = &want[name.as_deref().unwrap_or("")];
                        let w = w.as_array().unwrap();
                        if w.len() != terms.len() {
                            bad.push(format!("{label}: {} terms, want {}", terms.len(), w.len()));
                            continue;
                        }
                        for (((lo, hi), g), wt) in terms.iter().zip(w) {
                            n_gain += 1;
                            let wt: Vec<f64> = wt
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|x| x.as_f64().unwrap())
                                .collect();
                            if *lo != wt[0] || *hi != wt[1] || !close(*g, wt[2]) {
                                bad.push(format!("{label}: term ({lo}, {hi}, {g:e}) want {wt:?}"));
                            }
                        }
                    }
                }
                (g, w) => bad.push(format!("{label}: got {g:?}, want error {w:?}")),
            }
        } else {
            let b = e["band"].as_str().unwrap();
            let want = &e["gain"];
            match (band_gain(&bd, b, wl, joins, corr), want.get("error")) {
                (Err(g), Some(w)) => {
                    n_status += 1;
                    if g.0 != w.as_str().unwrap() {
                        bad.push(format!("{label}: status {:?} want {w}", g.0));
                    }
                }
                (Ok(((lo, hi), g)), None) => {
                    n_gain += 1;
                    let w = want["window"].as_array().unwrap();
                    if lo != w[0].as_f64().unwrap()
                        || hi != w[1].as_f64().unwrap()
                        || !close(g, want["gain"].as_f64().unwrap())
                    {
                        bad.push(format!("{label}: ({lo}, {hi}, {g:e}) want {want}"));
                    }
                }
                (g, w) => bad.push(format!("{label}: got {g:?}, want error {w:?}")),
            }
        }
    }
    eprintln!(
        "gains parity: {n_gain} gains within 1e-9, {n_status} identical statuses, {n_stat} statistic terms"
    );
    assert!(
        n_gain > 300 && n_status > 300 && n_stat > 500,
        "{n_gain} {n_status} {n_stat}"
    );
    assert!(bad.is_empty(), "{}", bad[..bad.len().min(20)].join("\n"));
}

/// The format-1 tables recorded each gain per transfer file (rounded to 6 decimal places; a C-H band's once more after
/// its envelope factor); computed for the same shipped files at the LabSpec joins, every gain must give its recorded
/// value through the same rounding. Kept as a golden only.
#[test]
fn computed_gains_reproduce_the_recorded_format_1_gains() {
    let rec = json(repo().join("crates/spyder-core/tests/goldens/recorded_gains_v1.json"));
    let plugins = repo().join("plugins");
    let ng = parse_noise_gains(&Node::root(&json(plugins.join("noise_gains.json")))).unwrap();
    let bd = parse_bands(&Node::root(&json(plugins.join("bands.json")))).unwrap();
    let wl: Vec<f64> = (0..2151).map(|i| 350.0 + i as f64).collect();
    let joins = [1000.0, 1800.0];
    let mut transfers = BTreeMap::new();
    for (k, c) in rec["corrections"].as_object().unwrap() {
        if c.is_null() {
            continue;
        }
        let bytes = std::fs::read(plugins.join(c["file"].as_str().unwrap())).unwrap();
        assert_eq!(sha256_hex(&bytes), c["sha256"].as_str().unwrap(), "{k}");
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let t =
            spyder_core::transfer::parse_transfer(&Node::root(&v), &sha256_hex(&bytes)).unwrap();
        transfers.insert(k.clone(), t);
    }
    let corr = |k: &str| transfers.get(k).map(|t| t.correction());
    let models: BTreeMap<String, spyder_core::model::Model> = std::fs::read_dir(&plugins)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with(".spyder-model.json")
        })
        .map(|e| {
            let m = parse_model(&Node::root(&json(e.path()))).unwrap();
            (m.header.id.clone(), m)
        })
        .collect();
    let (mut n, mut worst) = (0, 0.0f64);
    // format 1 rounded each raw gain to 6 decimals, and a C-H band's again after the envelope factor (p09): the
    // computed gain, put through the same rounding, must give the recorded value; it is never more than 2.5e-4 away
    let r6 = |x: f64| (x * 1e6).round() / 1e6;
    let mut check = |what: String, got: f64, want: f64, factor: f64| {
        n += 1;
        worst = worst.max((got - want).abs() / want);
        let through = r6(r6(got / factor) * factor);
        assert!(
            (through - want).abs() <= 1e-12 && (got - want).abs() <= 2.5e-4 * want,
            "{what}: computed {got} does not round to the recorded {want}"
        );
    };
    for (mid, keyed) in rec["models"].as_object().unwrap() {
        for (k, terms) in keyed.as_object().unwrap() {
            let t = model_terms(&ng, &models[mid], &wl, &joins, corr(k)).unwrap();
            let got = &t[0].1;
            for wt in terms.as_array().unwrap() {
                let w = wt["window_nm"].as_array().unwrap();
                let (lo, hi) = (w[0].as_f64().unwrap(), w[1].as_f64().unwrap());
                let g = got
                    .iter()
                    .find(|((a, b), _)| *a == lo && *b == hi)
                    .unwrap_or_else(|| panic!("{mid} [{k}]: no term for {lo}-{hi}"))
                    .1;
                check(
                    format!("{mid} [{k}] {lo}-{hi}"),
                    g,
                    wt["gain"].as_f64().unwrap(),
                    1.0,
                );
            }
        }
    }
    for (b, keyed) in rec["bands"].as_object().unwrap() {
        let factor = rec["envelope_factors"]
            .get(b)
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        assert_eq!(
            bd.bands[b].readability.as_ref().unwrap().envelope_factor,
            factor,
            "{b}"
        );
        for (k, want) in keyed.as_object().unwrap() {
            let ((lo, hi), g) = band_gain(&bd, b, &wl, &joins, corr(k)).unwrap();
            let w = rec["band_noise_windows_nm"][b].as_array().unwrap();
            assert_eq!(
                (lo, hi),
                (w[0].as_f64().unwrap(), w[1].as_f64().unwrap()),
                "{b}"
            );
            check(format!("{b} [{k}]"), g, want.as_f64().unwrap(), factor);
        }
    }
    // the non-linear S1_R2 keeps its recorded constant; the CONS3 factor carries over
    let s1 = &rec["nonlinear_models"]["collagen.spyder.1500_snv_transfer_free"]["none"][0];
    let t = model_terms(
        &ng,
        &models["collagen.spyder.1500_snv_transfer_free"],
        &wl,
        &joins,
        None,
    )
    .unwrap();
    assert_eq!(t[0].1[0].1, s1["gain"].as_f64().unwrap());
    assert_eq!(
        ng.consensus["collagen.spyder.consensus3_median"],
        rec["consensus_factor"]["collagen.spyder.consensus3_median"]
            .as_f64()
            .unwrap()
    );
    eprintln!("recorded format-1 gains: {n} reproduced, max relative difference {worst:.2e} (6-decimal rounding)");
    assert!(n >= 36, "{n}");
}
