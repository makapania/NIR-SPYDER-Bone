//! DECISIONS 99: within each verdict group, scans with a contaminant flag (plaster, wax, ester, C1) sort after the
//! clean ones; heat is not a contaminant. Expected values written from the ruling by hand (LESSONS 44). The same cases
//! run against the Python oracle in `reference/tests/test_sort_penalty.py`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::pipeline::verdict::sort_penalty;
use spyder_core::pipeline::Engine;
use spyder_core::plugins::registry::{Location, Origin, Pins, Registry, State};
use spyder_core::plugins::ErrorKind;

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn load(dir: &Path) -> Registry {
    Registry::load(
        &[Location {
            dir: dir.to_path_buf(),
            origin: Origin::User,
        }],
        &Pins::new(),
    )
}

fn flag(key: &str, signs: &[&str]) -> V {
    V::Map(Obj::new().with("key", key).with(
        "signs",
        V::List(signs.iter().map(|s| V::from(*s)).collect()),
    ))
}

fn block(flags: Vec<V>) -> Obj {
    Obj::new()
        .with("verdict", "Unlikely")
        .with("flags", V::List(flags))
}

#[test]
fn penalty_follows_contaminant_flags_only() {
    let reg = load(&plugins());
    let eng = Engine::new(&reg).unwrap();
    for a in ["radiocarbon", "isotopes"] {
        let p = eng.profile(a).unwrap();
        let cases: Vec<(Vec<V>, i64)> = vec![
            (vec![], 0),
            (vec![flag("flag_contaminant", &["wax"])], 1),
            (vec![flag("flag_contaminant", &["ester"])], 1),
            (vec![flag("flag_contaminant", &["plaster"])], 1),
            (vec![flag("flag_contaminant", &["C1"])], 1),
            // heat only lowers readings: no penalty
            (vec![flag("flag_burnt", &["burnt"])], 0),
            (
                vec![
                    flag("flag_contaminant", &["C1"]),
                    flag("flag_burnt", &["burnt"]),
                ],
                1,
            ),
        ];
        for (fl, want) in cases {
            assert_eq!(
                sort_penalty(p, &block(fl.clone())),
                Some(want),
                "{a}: {fl:?}"
            );
        }
    }
    // a profile without penalty_signs writes no sort_penalty key
    let z = eng.profile("zooms").unwrap();
    assert!(z.sort.penalty_signs.is_none());
    assert_eq!(
        sort_penalty(z, &block(vec![flag("flag_contaminant", &["wax"])])),
        None
    );
}

#[test]
fn penalty_sign_must_be_a_contaminant_flag_sign() {
    let d = std::env::temp_dir().join(format!("spyder-sortpen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(
        plugins().join("golden_spectra_public_v2.json"),
        d.join("golden_spectra_public_v2.json"),
    )
    .unwrap();
    let mut v: Value = serde_json::from_slice(
        &std::fs::read(plugins().join("profiles/profile.radiocarbon.json")).unwrap(),
    )
    .unwrap();
    v["parameters"]["sort"]["penalty_signs"] = json!(["burnt"]);
    std::fs::write(d.join("p.json"), serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    let reg = load(&d);
    let e = reg
        .entries
        .iter()
        .find(|e| e.path.file_name().unwrap() == "p.json")
        .unwrap();
    assert_eq!(e.state, State::Disabled);
    let err = e.error.as_ref().unwrap();
    assert_eq!(err.kind, ErrorKind::Schema);
    assert!(err.to_string().contains("penalty_signs"), "{err}");
}
