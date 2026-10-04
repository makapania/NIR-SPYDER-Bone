//! Session operations (Codex review of the assembled app, findings 2 and 3):
//! * Overlapping Open / Watch operations never mix files and never score one folder's files under another
//!   folder's class: an Open builds its own session and publishes it only while it is still the newest operation.
//! * Every reanalysis of a logged scan (the Standard / High-res switch, a re-watch under another configuration)
//!   appends a new result revision to the JSONL session log; the same result is never logged twice.
//!
//! Synthetic ASD-shaped bytes and invented file names only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use spyder_bone_app_lib::commands::{lock, Shared};
use spyder_bone_app_lib::engine::{bundled_dir, Core};
use spyder_bone_app_lib::session::{result_key, Input, NewScan, Session};
use spyder_core::plugins::{CLASS_HIRES, CLASS_STD};
use spyder_watch::asd_shape::synthetic;
use spyder_watch::session::{read_records, SessionLog};

fn core() -> Core {
    let c = Core::load(&bundled_dir(None), None);
    assert!(c.error.is_none(), "{:?}", c.error);
    c
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "spyder-app-{}-{}-{}",
            name,
            std::process::id(),
            spyder_watch::unix_ms(std::time::SystemTime::now())
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `n` synthetic scans `X10001_1.asd`, `X10001_2.asd`, ... in `dir`.
fn write_scans(dir: &Path, n: u32) -> Vec<PathBuf> {
    (1..=n)
        .map(|i| {
            let p = dir.join(format!("X10001_{i}.asd"));
            std::fs::write(&p, synthetic::as8(i)).unwrap();
            p
        })
        .collect()
}

#[test]
fn an_open_superseded_by_a_watch_never_mixes_files_or_classes() {
    let d = TempDir::new("ops");
    let files = write_scans(d.path(), 3);
    let shared = Shared::new(core());

    // Open A (a folder remembered as High-res) is reserved; before its worker runs, Watch B starts.
    let ga = shared.reserve();
    let gb = shared.new_session(Some("B".into()), true, None, None);
    assert!(gb > ga);
    assert_eq!(
        shared.open_files(ga, Some("A".into()), Some(CLASS_HIRES), &files),
        None,
        "a superseded Open must not publish"
    );
    {
        let s = lock(&shared.session);
        assert_eq!(s.gen, gb);
        assert_eq!(s.folder.as_deref(), Some("B"));
        assert_eq!(s.class, CLASS_STD, "B must not inherit A's class");
        assert!(s.entries.is_empty(), "A's files leaked into B's session");
    }

    // Two Opens: the later one wins even when the earlier one finishes last.
    let g1 = shared.reserve();
    let g2 = shared.reserve();
    assert_eq!(
        shared.open_files(g2, Some("D2".into()), Some(CLASS_STD), &files[..1]),
        Some(1)
    );
    assert_eq!(
        shared.open_files(g1, Some("D1".into()), Some(CLASS_HIRES), &files),
        None
    );
    let s = lock(&shared.session);
    assert_eq!(s.gen, g2);
    assert_eq!(s.folder.as_deref(), Some("D2"));
    assert_eq!(s.class, CLASS_STD);
    assert_eq!(s.entries.len(), 1);
    // Scan ids carry the session generation: a new session never reuses an old session's ids.
    assert!(
        s.entries[0].id.starts_with(&format!("s{g2}-")),
        "{}",
        s.entries[0].id
    );
}

#[test]
fn concurrent_opens_publish_exactly_the_newest_one_whole() {
    let d = TempDir::new("race");
    let files = write_scans(d.path(), 6);
    let shared = Arc::new(Shared::new(core()));
    let hs: Vec<_> = (0..6usize)
        .map(|k| {
            let sh = Arc::clone(&shared);
            let fs: Vec<PathBuf> = files[..=k].to_vec();
            std::thread::spawn(move || {
                let g = sh.reserve();
                let class = if k % 2 == 0 { CLASS_HIRES } else { CLASS_STD };
                (
                    g,
                    k,
                    sh.open_files(g, Some(format!("F{k}")), Some(class), &fs),
                )
            })
        })
        .collect();
    let done: Vec<_> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    let newest = done.iter().max_by_key(|x| x.0).unwrap();
    assert_eq!(newest.2, Some(newest.1 + 1), "the newest Open must publish");
    let s = lock(&shared.session);
    assert_eq!(s.gen, newest.0);
    assert_eq!(s.folder.as_deref(), Some(format!("F{}", newest.1).as_str()));
    assert_eq!(
        s.entries.len(),
        newest.1 + 1,
        "files from different Opens mixed"
    );
    let want = if newest.1 % 2 == 0 {
        CLASS_HIRES
    } else {
        CLASS_STD
    };
    assert_eq!(s.class, want);
}

fn arrival(path: &str, seed: u32) -> NewScan {
    let bytes = synthetic::as8(seed);
    let sha = spyder_core::plugins::sha256_hex(&bytes);
    NewScan {
        path: path.into(),
        file: Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        input: Input::Asd(bytes.into()),
        arrived_seq: Some(1),
        revision: 1,
        modified_ms: None,
        log_meta: Some(json!({ "path": path, "sha256": sha, "seq": 1 })),
    }
}

/// A synthetic scan with another serial and the given header SWIR1 / SWIR2 gains.
fn with_header(path: &str, seed: u32, serial: u16, gains: (u16, u16)) -> NewScan {
    let mut b = synthetic::as8(seed);
    b[400..402].copy_from_slice(&serial.to_le_bytes());
    b[436..438].copy_from_slice(&gains.0.to_le_bytes());
    b[438..440].copy_from_slice(&gains.1.to_le_bytes());
    // the reference flag as LabSpec writes it (TRUE = 0xffff), so the core reads the file and its notes show
    let flag = 484 + 8 * 2151;
    b[flag..flag + 2].copy_from_slice(&[0xff, 0xff]);
    NewScan {
        input: Input::Asd(b.into()),
        log_meta: None,
        ..arrival(path, seed)
    }
}

#[test]
fn header_gains_preset_an_unlisted_serial_and_never_override_the_user() {
    use spyder_bone_app_lib::mapping::scan_result;
    use spyder_bone_app_lib::session::ClassSource;
    let core = core();
    let notes = |s: &Session, i: usize| -> Vec<String> {
        scan_result(&core, &s.entries[i], "radiocarbon", s.class, s.source)
            .notes
            .into_iter()
            .map(|n| n.key)
            .collect()
    };

    assert_eq!(core.class_of_swir_gains(16, 18), Some(CLASS_HIRES));
    assert_eq!(core.class_of_swir_gains(300, 450), Some(CLASS_STD));

    // Unlisted serial, high-res-like gains: the switch is preset to High-res from the header.
    let mut s = Session::new(1, Some("H".into()), false, None);
    s.add(&core, with_header("H/a.asd", 1, 40001, (16, 18)));
    assert_eq!(s.entries[0].swir_gains(), Some((16, 18)));
    assert_eq!((s.class, s.source), (CLASS_HIRES, ClassSource::Header));
    // A standard-looking file in the same folder keeps the preset and gets a gentle note.
    s.add(&core, with_header("H/b.asd", 2, 40002, (300, 450)));
    assert_eq!((s.class, s.source), (CLASS_HIRES, ClassSource::Header));
    assert!(!notes(&s, 0).contains(&"header_class_mismatch".to_string()));
    assert!(notes(&s, 1).contains(&"header_class_mismatch".to_string()));
    // A listed serial outranks the header preset.
    s.add(&core, with_header("H/c.asd", 3, 28404, (300, 450)));
    assert_eq!((s.class, s.source), (CLASS_STD, ClassSource::Preset));

    // Unlisted serial, standard-like gains: preset to Standard.
    let mut s = Session::new(2, Some("S".into()), false, None);
    s.add(&core, with_header("S/a.asd", 1, 40001, (300, 450)));
    assert_eq!((s.class, s.source), (CLASS_STD, ClassSource::Header));

    // Gains that fit no class (between the ranges, split, or absent): no preset, no note, no error.
    for (k, g) in [(100, 100), (16, 450), (0, 0)].into_iter().enumerate() {
        let mut s = Session::new(3 + k as u64, Some("U".into()), false, None);
        s.add(&core, with_header("U/a.asd", 1, 40001, g));
        assert_eq!(
            (s.class, s.source),
            (CLASS_STD, ClassSource::Default),
            "{g:?}"
        );
        assert!(!notes(&s, 0).contains(&"header_class_mismatch".to_string()));
    }

    // The user's remembered switch is never preset over.
    let mut s = Session::new(9, Some("R".into()), false, Some(CLASS_STD));
    s.add(&core, with_header("R/a.asd", 1, 40001, (16, 16)));
    assert_eq!((s.class, s.source), (CLASS_STD, ClassSource::User));
    assert!(notes(&s, 0).contains(&"header_class_mismatch".to_string()));
}

fn scans(log: &Path) -> Vec<Value> {
    read_records(log)
        .unwrap()
        .into_iter()
        .filter(|r| r["kind"] == "scan")
        .collect()
}

#[test]
fn every_reanalysis_appends_a_result_revision_and_rewatching_dedups_on_configuration() {
    let core = core();
    let d = TempDir::new("revisions");
    let log = d.path().join("session.jsonl");
    let path = "W/X10001_1.asd";

    let mut s = Session::new(1, Some("W".into()), true, None)
        .with_log(Some(SessionLog::open(&log).unwrap()));
    // A first scan without a serial: analysed and logged under the default (Standard).
    let flat = "W/X10001_0.asd";
    s.add(
        &core,
        NewScan {
            input: Input::Spectrum {
                wl: (0..2151).map(|i| 350.0 + f64::from(i)).collect(),
                r: vec![0.5; 2151],
                joins: vec![1000.0, 1800.0],
                serial: None,
            },
            log_meta: Some(json!({ "path": flat, "sha256": "00", "seq": 0 })),
            ..arrival(flat, 1)
        },
    );
    let r = scans(&log);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["reason"], "arrival");
    assert_eq!(r[0]["resultRevision"], 1);
    assert_eq!(r[0]["resultClass"], "standard");
    assert_eq!(r[0]["classSource"], "default");
    assert!(r[0]["result"].is_object());

    // The synthetic scan's serial is a known High-res unit: the serial preset re-analyses the session. The new
    // scan is logged as its arrival (High-res, preset) and the first scan gets a revision under the preset.
    s.add(&core, arrival(path, 7));
    let r = scans(&log);
    assert_eq!(r.len(), 3);
    let first = r.iter().filter(|x| x["path"] == flat).collect::<Vec<_>>();
    assert_eq!(first.len(), 2);
    assert_eq!(first[1]["reason"], "reanalysis");
    assert_eq!(first[1]["resultRevision"], 2);
    assert_eq!(first[1]["resultClass"], "hires");
    assert_eq!(first[1]["classSource"], "serial_preset");
    let mine =
        |r: &[Value]| -> Vec<Value> { r.iter().filter(|x| x["path"] == path).cloned().collect() };
    let m = mine(&r);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0]["reason"], "arrival");
    assert_eq!(m[0]["resultClass"], "hires");
    assert_eq!(m[0]["seq"], 1, "the arrival's file facts are kept");

    // The switch re-analyses: a new revision of every scan with the new configuration.
    s.set_class(&core, CLASS_STD);
    let r = scans(&log);
    assert_eq!(r.len(), 5);
    let m = mine(&r);
    assert_eq!(m[1]["reason"], "reanalysis");
    assert_eq!(m[1]["resultRevision"], 2);
    assert_eq!(m[1]["resultClass"], "standard");
    assert_eq!(m[1]["classSource"], "user");
    assert_eq!(m[1]["sha256"], m[0]["sha256"]);
    assert_ne!(m[1]["resultKey"], m[0]["resultKey"]);
    // The same switch again changes nothing, so nothing is appended.
    s.set_class(&core, CLASS_STD);
    assert_eq!(scans(&log).len(), 5);
    drop(s);

    // Re-watched under the same configuration: the log already holds this exact result.
    let mut s = Session::new(2, Some("W".into()), true, Some(CLASS_STD))
        .with_log(Some(SessionLog::open(&log).unwrap()));
    s.add(&core, arrival(path, 7));
    assert_eq!(scans(&log).len(), 5);
    drop(s);

    // Re-watched under another configuration: appended as the next revision.
    let mut s = Session::new(3, Some("W".into()), true, Some(CLASS_HIRES))
        .with_log(Some(SessionLog::open(&log).unwrap()));
    s.add(&core, arrival(path, 7));
    let r = scans(&log);
    assert_eq!(r.len(), 6);
    let m = mine(&r);
    assert_eq!(m[2]["resultRevision"], 3);
    assert_eq!(m[2]["resultClass"], "hires");
    assert_eq!(m[2]["classSource"], "user");

    // New content under the same name starts its own revisions.
    s.add(&core, arrival(path, 8));
    let m = mine(&scans(&log));
    assert_eq!(m.len(), 4);
    assert_eq!(m[3]["resultRevision"], 1);
    assert_ne!(m[3]["sha256"], m[2]["sha256"]);

    // Opened files (no log metadata) are never logged.
    s.add(
        &core,
        NewScan {
            log_meta: None,
            ..arrival("W/X10001_2.asd", 9)
        },
    );
    assert_eq!(scans(&log).len(), 7);

    // The key covers the dependency hashes and the engine version, not just the input and the class.
    let rec = |dep: &str, eng: &str| json!({"oracle_version": eng, "dependencies": {"model.a@1.0.0": dep}});
    let k = |r: &Value| result_key(path, "ab", "standard", "user", r);
    assert_ne!(k(&rec("01", "1")), k(&rec("02", "1")));
    assert_ne!(k(&rec("01", "1")), k(&rec("01", "2")));
    assert_eq!(k(&rec("01", "1")), k(&rec("01", "1")));
}
