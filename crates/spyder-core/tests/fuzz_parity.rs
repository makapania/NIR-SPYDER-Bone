//! PLAN Phase 2 gate: cross-implementation fuzz on 200 perturbed public spectra. The perturbations are
//! regenerated here bit for bit (SplitMix64 + basic IEEE operations, the contract in
//! tests/gen/parity_common.py, checked by content hash); every active model runs through `predict_scan`
//! (transfer selection, transfer, chain, prediction) for both instrument classes and must match the frozen
//! Python reference within 1e-9 + 1e-9 |python| (tests/goldens/fuzz_predictions_v1.json).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::Value;
use spyder_core::plugins::golden::load_golden_spectra;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry};
use spyder_core::plugins::{spectrum_sha256, ScanContext};
use spyder_core::predict::predict_scan;

pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

/// parity_common.perturb, operation for operation.
pub fn perturb(wl: &[f64], x: &[f64], k: u64) -> (Vec<f64>, Vec<f64>) {
    let mut g = SplitMix64(0x5350_5944_4552_0000 + k);
    let scale = 0.6 + 0.8 * g.uniform();
    let offset = (g.uniform() - 0.5) * 0.04;
    let step1 = (g.uniform() - 0.5) * 0.04;
    let step2 = (g.uniform() - 0.5) * 0.04;
    let tilt = (g.uniform() - 0.5) * 2e-5;
    let amp = 1e-4 * (1.0 + 9.0 * g.uniform());
    let j2 = 1800.0 + 10.0 * (g.next_u64() >> 62) as f64;
    let mut out = Vec::with_capacity(x.len());
    for i in 0..x.len() {
        let w = wl[i];
        let mut v = x[i] * scale + offset;
        if w <= 1000.0 {
            v += step1;
        } else if w > j2 {
            v += step2;
        }
        v += tilt * (w - 1500.0);
        v += amp * (2.0 * g.uniform() - 1.0);
        if k % 10 == 7 && i < 40 {
            v = -0.01;
        }
        out.push(v);
    }
    (out, vec![1000.0, j2])
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn close(got: f64, want: f64) -> bool {
    got.is_finite() && (got - want).abs() <= 1e-9 + 1e-9 * want.abs()
}

#[test]
fn rust_matches_python_on_200_perturbed_public_spectra() {
    let plugins = std::env::var_os("SPYDER_PLUGINS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("plugins"));
    let fx: Value = serde_json::from_slice(
        &std::fs::read(repo().join("crates/spyder-core/tests/goldens/fuzz_predictions_v1.json"))
            .expect("fixture"),
    )
    .unwrap();
    let reg = Registry::load(
        &[Location {
            dir: plugins.clone(),
            origin: Origin::Bundled,
        }],
        &Pins::new(),
    );
    // a stale fixture is an error: the plug-in files must be the ones the fixture was generated from
    let on_disk: BTreeMap<String, String> = reg
        .entries
        .iter()
        .filter(|e| e.path.parent() == Some(plugins.as_path()))
        .map(|e| {
            (
                e.path.file_name().unwrap().to_string_lossy().to_string(),
                e.sha256.clone(),
            )
        })
        .collect();
    for (f, sha) in fx["plugin_files"].as_object().unwrap() {
        assert_eq!(
            on_disk.get(f).map(String::as_str),
            sha.as_str(),
            "{f} changed since the fuzz fixture was generated: re-run tests/gen/gen_fuzz_predictions.py"
        );
    }
    let gs =
        load_golden_spectra(&plugins.join(fx["golden_spectra_file"].as_str().unwrap())).unwrap();
    let (mut n, mut worst) = (0usize, 0.0f64);
    let mut bad = Vec::new();
    for c in fx["cases"].as_array().unwrap() {
        let k = c["k"].as_u64().unwrap();
        let base = &gs.spectra[c["base_sha256"].as_str().unwrap()];
        let (x, joins) = perturb(&base.wl, &base.x, k);
        assert_eq!(
            spectrum_sha256(&base.wl, &x),
            c["sha256"].as_str().unwrap(),
            "case {k}: the perturbed spectrum differs from Python's"
        );
        let fj: Vec<f64> = c["joins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(fj, joins, "case {k}: joins");
        for (cls, want) in c["classes"].as_object().unwrap() {
            let ctx = ScanContext::new(joins.clone(), cls, None);
            let got = predict_scan(&reg, &base.wl, &x, &ctx);
            let want = want.as_object().unwrap();
            assert_eq!(got.len(), want.len(), "case {k} {cls}: model count");
            for r in &got {
                let w = want
                    .get(&r.id)
                    .unwrap_or_else(|| panic!("case {k} {cls}: {} not in the fixture", r.id));
                if w.get("error").is_some() {
                    if r.prediction.is_some() {
                        bad.push(format!(
                            "case {k} {cls} {}: Python failed ({}), Rust predicted",
                            r.id, w["error"]
                        ));
                    }
                    n += 1;
                    continue;
                }
                let tr = r.transfer.as_ref().map(|t| t.sha256.as_str());
                if tr != w["transfer"].as_str() {
                    bad.push(format!(
                        "case {k} {cls} {}: transfer {tr:?} vs {}",
                        r.id, w["transfer"]
                    ));
                }
                let Some(p) = &r.prediction else {
                    bad.push(format!(
                        "case {k} {cls} {}: Rust {} but Python predicted",
                        r.id,
                        r.assessment.describe()
                    ));
                    continue;
                };
                for (key, wv) in w.as_object().unwrap() {
                    if key == "transfer" {
                        continue;
                    }
                    let wv = wv.as_f64().unwrap();
                    let g = p.output(key);
                    n += 1;
                    match g {
                        Some(g) if close(g, wv) => {
                            worst = worst.max((g - wv).abs() / (1e-9 + 1e-9 * wv.abs()));
                        }
                        _ => bad.push(format!(
                            "case {k} {cls} {} {key}: rust {g:?} python {wv}",
                            r.id
                        )),
                    }
                }
            }
        }
    }
    let _ = writeln!(
        std::io::stderr(),
        "fuzz parity: {n} comparisons on {} cases x 2 classes, worst |diff| / tolerance {worst:.3e}",
        fx["cases"].as_array().unwrap().len()
    );
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad[..bad.len().min(20)].join("\n")
    );
    // 5 active models since DECISIONS 78 (the uncorrected N-H set is withdrawn): ~8,000 comparisons
    assert!(n > 7_500, "only {n} comparisons");
}
