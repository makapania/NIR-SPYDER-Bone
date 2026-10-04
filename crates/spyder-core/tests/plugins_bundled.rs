//! PLAN Phase 2 gate, part 1 (Rust side): every bundled plug-in in `<repo>/plugins` (or $SPYDER_PLUGINS_DIR)
//! loads and passes its goldens in the Rust engine; the bundled set is not in the startup-error state; file
//! identities (consensus component hashes, transfer keys, catalog) match the bytes on disk.
//!
//! The Python side (spyder_ref on the same goldens) and the check-count comparison are run by
//! `crates/spyder-core/tests/gen/run_parity.py`.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;

use spyder_core::model::Body;
use spyder_core::plugins::registry::{Item, Location, Origin, Pins, Registry, State};
use spyder_core::plugins::sha256_hex;

fn plugins_dir() -> PathBuf {
    std::env::var_os("SPYDER_PLUGINS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins"))
}

fn load() -> Registry {
    Registry::load(
        &[Location {
            dir: plugins_dir(),
            origin: Origin::Bundled,
        }],
        &Pins::new(),
    )
}

#[test]
fn every_bundled_plugin_loads_and_passes_its_goldens() {
    let reg = load();
    let mut err = std::io::stderr();
    let mut failed = Vec::new();
    let mut total_checks = 0;
    for e in reg.plugin_entries() {
        let checks = e.golden.as_ref().map(|g| g.checks).unwrap_or(0);
        total_checks += checks;
        let _ = writeln!(
            err,
            "{:<9} {:<55} {:>6} checks  worst {:>9.2e}  deferred {:>3}  {}",
            format!("{:?}", e.state),
            e.label(),
            checks,
            e.golden.as_ref().map(|g| g.worst_ratio).unwrap_or(0.0),
            e.golden_deferred.unwrap_or(0),
            e.error.as_ref().map(|x| x.message.as_str()).unwrap_or("")
        );
        if e.state != State::Loaded {
            failed.push(format!(
                "{}: {}",
                e.path.display(),
                e.error.as_ref().map(|x| x.message.as_str()).unwrap_or("")
            ));
        }
    }
    let _ = writeln!(err, "total golden checks: {total_checks}");
    for n in &reg.notes {
        let _ = writeln!(err, "note: {n}");
    }
    assert!(
        failed.is_empty(),
        "bundled plug-ins failed:\n{}",
        failed.join("\n")
    );
    assert!(
        reg.startup_errors.is_empty(),
        "startup errors: {:?}",
        reg.startup_errors
    );
    // the shipped set is complete
    assert!(reg.model("collagen.spyder.consensus3_median").is_some());
    assert!(reg.model("collagen.ryder2026.2045").is_some());
    assert!(!reg.transfers().is_empty());
    assert!(reg.bands().is_some() && reg.noise_gains().is_some() && reg.instruments().is_some());
    assert!(total_checks > 0);
}

#[test]
fn identities_match_the_bytes_on_disk() {
    // consensus component sha256, noise-gain and band transfer keys, and catalog entries must equal the
    // SHA-256 of the files as read from disk (never normalised)
    let dir = plugins_dir();
    let reg = load();
    let disk: BTreeMap<String, PathBuf> = reg
        .entries
        .iter()
        .map(|e| (sha256_hex(&std::fs::read(&e.path).unwrap()), e.path.clone()))
        .collect();
    for e in &reg.entries {
        assert_eq!(
            disk.get(&e.sha256),
            Some(&e.path),
            "{}: registry hash is not the hash of the bytes on disk",
            e.path.display()
        );
    }
    let mut n = 0;
    for m in reg.models() {
        if let Body::Consensus { components } = &m.body {
            for c in components {
                let p = disk.get(&c.sha256).unwrap_or_else(|| {
                    panic!("{}: component {} sha256 matches no file", m.key(), c.name)
                });
                assert!(p.is_file());
                n += 1;
            }
        }
    }
    assert!(n >= 3, "no consensus components checked");
    let transfer_shas: Vec<String> = reg
        .entries
        .iter()
        .filter(|e| matches!(e.item, Some(Item::Transfer(_))))
        .map(|e| e.sha256.clone())
        .collect();
    let ng = reg.noise_gains().expect("noise gains");
    let mut keys = 0;
    for (mid, keyed) in &ng.models {
        for k in keyed.keys() {
            if k != "none" {
                assert!(
                    transfer_shas.contains(k),
                    "noise_gains {mid}: key {k} is no transfer file"
                );
                keys += 1;
            }
        }
    }
    for (b, band) in &reg.bands().unwrap().bands {
        if let Some(r) = &band.readability {
            for k in r.gain_e_per_n2.keys() {
                assert!(
                    k == "none" || transfer_shas.contains(k),
                    "bands {b}: key {k}"
                );
            }
        }
    }
    assert!(keys > 0, "no transfer keys checked");
    // the catalog matches the folder
    let cat = std::fs::read(dir.join("catalog.json")).unwrap();
    let cat: serde_json::Value = serde_json::from_slice(&cat).unwrap();
    for e in cat["entries"].as_array().unwrap() {
        let f = dir.join(e["file"].as_str().unwrap());
        let bytes =
            std::fs::read(&f).unwrap_or_else(|_| panic!("catalog file {} missing", f.display()));
        assert_eq!(
            sha256_hex(&bytes),
            e["sha256"].as_str().unwrap(),
            "{}",
            f.display()
        );
    }
}
