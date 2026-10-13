//! DECISIONS 103: the foreign-organic sign (C1) also fires on the organic-film contrast (peak 2299 +- 3 nm minus trough
//! 2325 +- 3 nm) at >= 17.8, in the full variant only. Expected values written from the ruling by hand (LESSONS 44) on
//! synthetic E arrays (zero everywhere, so the C-H excess is -a and never fires by itself). The same cases run against
//! the Python oracle in `reference/tests/test_c1_film.py`.

use std::path::PathBuf;

use serde_json::Value;
use spyder_core::pipeline::checks::c1;
use spyder_core::pipeline::val::V;
use spyder_core::plugins::checks::parse_engine_check;
use spyder_core::plugins::Node;

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn params() -> spyder_core::plugins::checks::CheckParams {
    let v: Value =
        serde_json::from_slice(&std::fs::read(plugins().join("checks/check.c1.json")).unwrap())
            .unwrap();
    parse_engine_check(&Node::root(&v), &plugins().join("checks"))
        .unwrap()
        .params
}

/// E on the 350-2500 nm grid: `peak` over 2296-2302 nm, zero elsewhere.
fn spectrum(peak: f64) -> (Vec<f64>, Vec<f64>) {
    let wl: Vec<f64> = (0..2151).map(|i| 350.0 + i as f64).collect();
    let e = wl
        .iter()
        .map(|&w| {
            if (2296.0..=2302.0).contains(&w) {
                peak
            } else {
                0.0
            }
        })
        .collect();
    (wl, e)
}

fn get<'a>(o: &'a spyder_core::pipeline::val::Obj, k: &str) -> &'a V {
    o.get(k).unwrap()
}

#[test]
fn film_fires_c1_at_its_cut_and_not_below() {
    let p = params();
    for (peak, want) in [(20.0, true), (17.8, true), (17.0, false), (0.0, false)] {
        let (wl, e) = spectrum(peak);
        let r = c1(&wl, &e, &p, None, false, false, "truncated");
        assert_eq!(get(&r, "status").as_str(), Some("assessed"));
        assert!(
            (get(&r, "film").as_f64().unwrap() - peak).abs() < 1e-9,
            "film = peak - 0"
        );
        assert_eq!(get(&r, "film_fired"), &V::Bool(want), "peak {peak}");
        assert_eq!(
            get(&r, "fired"),
            &V::Bool(want),
            "the excess alone never fires here (peak {peak})"
        );
        assert!(get(&r, "excess").as_f64().unwrap() < 8.0);
    }
}

#[test]
fn film_does_not_run_in_the_truncated_variant_or_when_gated() {
    let p = params();
    let (wl, e) = spectrum(50.0);
    let r = c1(&wl, &e, &p, None, false, true, "truncated");
    assert!(get(&r, "status")
        .as_str()
        .unwrap()
        .starts_with("assessed: truncated"));
    assert_eq!(get(&r, "film"), &V::Null);
    assert_eq!(get(&r, "film_fired"), &V::Null);
    assert_eq!(get(&r, "fired"), &V::Bool(false));
    let g = c1(
        &wl,
        &e,
        &p,
        Some("B6b too noisy"),
        false,
        false,
        "truncated",
    );
    assert_eq!(get(&g, "film_fired"), &V::Null);
    assert_eq!(get(&g, "fired"), &V::Null);
}
