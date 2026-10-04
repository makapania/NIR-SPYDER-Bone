//! Watcher tests (PLAN v3.2 section 8 Phase 5 gate): slow writer, rename into place, same-name rewrite,
//! preallocation, paused same-size overwrite, 200-file burst, folder removed and recreated, event loss, POLL
//! mode; plus include-existing, reference saves, unrecognised bytes, a file that never completes, and (Windows)
//! a writer holding the file exclusively. Temp folders and synthetic ASD-shaped bytes only.

use spyder_watch::asd_shape::synthetic;
use spyder_watch::asd_shape::KindHint;
use spyder_watch::volume::VolumeKind;
use spyder_watch::{
    ActiveMode, Arrival, FolderWatch, ModeChoice, Timing, WatchConfig, WatchEvent, WatchState,
};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread::sleep;
use std::time::{Duration, Instant};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "spyder-watch-{}-{}-{}",
            name,
            std::process::id(),
            spyder_watch::unix_ms(std::time::SystemTime::now())
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn timing() -> Timing {
    Timing {
        give_up_local: Duration::from_secs(4),
        give_up_network: Duration::from_secs(8),
        rescan_interval: Duration::from_secs(2),
        ..Timing::default()
    }
}

fn config(dir: &Path) -> WatchConfig {
    WatchConfig {
        timing: timing(),
        ..WatchConfig::new(dir)
    }
}

fn start(cfg: WatchConfig) -> (FolderWatch, Receiver<(Instant, WatchEvent)>) {
    let (tx, rx) = mpsc::channel();
    let w = FolderWatch::start(cfg, move |e| {
        let _ = tx.send((Instant::now(), e));
    })
    .unwrap();
    (w, rx)
}

/// Collects events until `n` arrivals have been seen or `timeout` passes.
fn arrivals(
    rx: &Receiver<(Instant, WatchEvent)>,
    n: usize,
    timeout: Duration,
) -> Vec<(Instant, Arrival)> {
    let end = Instant::now() + timeout;
    let mut out = Vec::new();
    while out.len() < n {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok((t, WatchEvent::Arrived(a))) => out.push((t, a)),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    out
}

/// Everything that arrives within `dur`.
fn drain(rx: &Receiver<(Instant, WatchEvent)>, dur: Duration) -> Vec<WatchEvent> {
    let end = Instant::now() + dur;
    let mut out = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok((_, e)) => out.push(e),
            Err(_) => break,
        }
    }
    out
}

fn only_arrivals(ev: &[WatchEvent]) -> Vec<&Arrival> {
    ev.iter()
        .filter_map(|e| match e {
            WatchEvent::Arrived(a) => Some(a),
            _ => None,
        })
        .collect()
}

fn wait_status(
    rx: &Receiver<(Instant, WatchEvent)>,
    want: WatchState,
    timeout: Duration,
) -> Vec<WatchEvent> {
    let end = Instant::now() + timeout;
    let mut seen = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok((_, WatchEvent::Status(s))) if s.state == want => return seen,
            Ok((_, e)) => seen.push(e),
            Err(_) => break,
        }
    }
    panic!("status {want:?} not reached; saw {seen:?}");
}

/// Writes `bytes` in `chunk`-byte pieces with `gap` between them; returns when the last byte is written.
fn slow_write(path: &Path, bytes: &[u8], chunk: usize, gap: Duration) -> Instant {
    let mut f = fs::File::create(path).unwrap();
    for c in bytes.chunks(chunk) {
        f.write_all(c).unwrap();
        f.flush().unwrap();
        sleep(gap);
    }
    drop(f);
    Instant::now()
}

fn assert_no_more_arrivals(rx: &Receiver<(Instant, WatchEvent)>, dur: Duration) {
    let ev = drain(rx, dur);
    let a = only_arrivals(&ev);
    assert!(
        a.is_empty(),
        "unexpected arrivals: {:?}",
        a.iter()
            .map(|a| (&a.file_name, a.revision))
            .collect::<Vec<_>>()
    );
}

#[test]
fn slow_writer_is_delivered_once_complete_after_it_finishes() {
    let d = TempDir::new("slow");
    let (_w, rx) = start(config(d.path()));
    let bytes = synthetic::as8(1);
    let done = slow_write(
        &d.file("Spectrum00001.asd"),
        &bytes,
        1024,
        Duration::from_millis(50),
    );
    let got = arrivals(&rx, 1, Duration::from_secs(10));
    assert_eq!(got.len(), 1);
    let (t, a) = &got[0];
    assert!(*t >= done, "delivered before the writer finished");
    assert_eq!(&a.bytes[..], &bytes[..]);
    assert_eq!(a.revision, 1);
    assert!(a.recognised);
    assert_eq!(a.kind_hint, Some(KindHint::Sample));
    assert_eq!(a.size, 35_132);
    assert_no_more_arrivals(&rx, Duration::from_secs(2));
}

#[test]
fn rename_into_place_and_temp_names_are_ignored() {
    let d = TempDir::new("rename");
    let (_w, rx) = start(config(d.path()));
    let bytes = synthetic::as8(2);
    // A slow write under a temp name, then junk that must never be delivered, then the rename.
    slow_write(
        &d.file("Spectrum00002.asd.tmp"),
        &bytes,
        4096,
        Duration::from_millis(30),
    );
    fs::write(d.file("._Spectrum00002.asd"), b"AppleDouble").unwrap();
    fs::write(d.file("~$Spectrum00002.asd"), b"lock").unwrap();
    fs::write(d.file("notes.txt"), b"hello").unwrap();
    fs::rename(d.file("Spectrum00002.asd.tmp"), d.file("Spectrum00002.asd")).unwrap();
    let got = arrivals(&rx, 1, Duration::from_secs(8));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.file_name, "Spectrum00002.asd");
    assert_eq!(&got[0].1.bytes[..], &bytes[..]);
    assert_no_more_arrivals(&rx, Duration::from_secs(3));
}

#[test]
fn same_name_rewrite_is_a_new_revision_and_identical_content_is_not() {
    let d = TempDir::new("rewrite");
    let (_w, rx) = start(config(d.path()));
    let p = d.file("X10001_1.asd");
    let (a, b) = (synthetic::as8(10), synthetic::as8(11));
    fs::write(&p, &a).unwrap();
    let r1 = arrivals(&rx, 1, Duration::from_secs(6));
    assert_eq!(r1[0].1.revision, 1);
    // Let the post-delivery check pass, then rewrite with new content.
    sleep(Duration::from_millis(1500));
    fs::write(&p, &b).unwrap();
    let r2 = arrivals(&rx, 1, Duration::from_secs(8));
    assert_eq!(r2.len(), 1, "rewrite not delivered");
    assert_eq!(r2[0].1.revision, 2);
    assert_eq!(&r2[0].1.bytes[..], &b[..]);
    // Writing the same bytes again (a "save" without changes) is not a new scan.
    sleep(Duration::from_millis(1500));
    fs::write(&p, &b).unwrap();
    assert_no_more_arrivals(&rx, Duration::from_secs(4));
}

#[test]
fn preallocated_file_is_not_delivered_until_filled() {
    let d = TempDir::new("prealloc");
    let (_w, rx) = start(config(d.path()));
    let bytes = synthetic::as8(3);
    let p = d.file("Spectrum00003.asd");
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&p)
        .unwrap();
    f.set_len(bytes.len() as u64).unwrap(); // full length, zero-filled
    f.write_all(&bytes[..20_000]).unwrap(); // header + sample block + part of the reference block
    f.flush().unwrap();
    sleep(Duration::from_millis(2500)); // far longer than the settle window: size and mtime are stable
    let early = drain(&rx, Duration::from_millis(10));
    assert!(
        only_arrivals(&early).is_empty(),
        "a zero-filled file was delivered"
    );
    f.write_all(&bytes[20_000..]).unwrap();
    f.flush().unwrap();
    drop(f);
    let got = arrivals(&rx, 1, Duration::from_secs(10));
    assert_eq!(got.len(), 1);
    assert_eq!(&got[0].1.bytes[..], &bytes[..]);
    assert_no_more_arrivals(&rx, Duration::from_secs(2));
}

/// In-place overwrite with the same size (no truncation). A pause shorter than the settle window never
/// exposes the mixed file. A longer pause can (no stat-based rule can tell), but the finished content always
/// arrives as the LAST revision, so the scan on screen ends up right.
#[test]
fn paused_same_size_overwrite() {
    let d = TempDir::new("samesize");
    let (_w, rx) = start(config(d.path()));
    let p = d.file("AB CD12a_1.asd");
    let (a, b, c) = (synthetic::as8(20), synthetic::as8(21), synthetic::as8(22));
    assert_eq!(a.len(), b.len());
    fs::write(&p, &a).unwrap();
    assert_eq!(arrivals(&rx, 1, Duration::from_secs(6)).len(), 1);
    sleep(Duration::from_millis(1500));

    let overwrite = |new: &[u8], pause: Duration| {
        let mut f = OpenOptions::new().write(true).open(&p).unwrap();
        f.seek(SeekFrom::Start(0)).unwrap();
        let half = new.len() / 2;
        f.write_all(&new[..half]).unwrap();
        f.flush().unwrap();
        sleep(pause);
        f.write_all(&new[half..]).unwrap();
        f.flush().unwrap();
    };

    // Short pause (< settle): exactly one new revision, the finished bytes.
    overwrite(&b, Duration::from_millis(250));
    let got = arrivals(&rx, 1, Duration::from_secs(8));
    assert_eq!(got.len(), 1);
    assert_eq!(&got[0].1.bytes[..], &b[..]);
    assert_eq!(got[0].1.revision, 2);
    assert_no_more_arrivals(&rx, Duration::from_secs(2));

    // Long pause (> twice the settle window): the last revision is the finished file.
    overwrite(&c, Duration::from_millis(1600));
    let ev = drain(&rx, Duration::from_secs(6));
    let revs = only_arrivals(&ev);
    assert!(!revs.is_empty(), "the overwrite was never delivered");
    let last = revs.last().unwrap();
    assert_eq!(
        &last.bytes[..],
        &c[..],
        "the final revision is not the finished file"
    );
    assert!(revs.windows(2).all(|w| w[1].revision == w[0].revision + 1));
}

#[test]
fn burst_of_200_files_each_delivered_once() {
    let d = TempDir::new("burst");
    let (w, rx) = start(config(d.path()));
    let mut expected = std::collections::HashMap::new();
    for i in 0..200u32 {
        let name = format!("Spectrum{:05}.asd", i + 1);
        let bytes = synthetic::as8(1000 + i);
        fs::write(d.file(&name), &bytes).unwrap();
        expected.insert(name, bytes);
    }
    let got = arrivals(&rx, 200, Duration::from_secs(30));
    assert_eq!(got.len(), 200, "only {} of 200 delivered", got.len());
    let mut names = std::collections::HashSet::new();
    for (k, (_, a)) in got.iter().enumerate() {
        assert_eq!(a.seq, k as u64 + 1);
        assert!(
            names.insert(a.file_name.clone()),
            "{} delivered twice",
            a.file_name
        );
        assert_eq!(&a.bytes[..], &expected[&a.file_name][..], "{}", a.file_name);
    }
    assert_no_more_arrivals(&rx, Duration::from_secs(3));
    assert_eq!(w.status().delivered, 200);
    assert_eq!(w.status().pending, 0);
}

#[test]
fn folder_removed_and_recreated_resumes() {
    let d = TempDir::new("reconnect");
    let mut cfg = config(d.path());
    cfg.timing.rescan_interval = Duration::from_millis(700);
    let (_w, rx) = start(cfg);
    let one = synthetic::as8(30);
    fs::write(d.file("one.asd"), &one).unwrap();
    assert_eq!(arrivals(&rx, 1, Duration::from_secs(6)).len(), 1);
    sleep(Duration::from_millis(1200));

    retry(|| fs::remove_dir_all(d.path()));
    wait_status(&rx, WatchState::FolderMissing, Duration::from_secs(8));

    // Windows keeps a deleted, still-watched folder "delete pending" until the watch lets go: retry the create.
    retry(|| fs::create_dir(d.path()));
    let two = synthetic::as8(31);
    fs::write(d.file("one.asd"), &one).unwrap(); // the same scan back again: not a new arrival
    fs::write(d.file("two.asd"), &two).unwrap();
    wait_status(&rx, WatchState::Watching, Duration::from_secs(8));
    let got = arrivals(&rx, 1, Duration::from_secs(8));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.file_name, "two.asd");
    assert_no_more_arrivals(&rx, Duration::from_secs(3));

    // Native events work again after the reconnect (a new file arrives well before the next rescan would
    // matter, and in any case it arrives).
    fs::write(d.file("three.asd"), synthetic::as8(32)).unwrap();
    let got = arrivals(&rx, 1, Duration::from_secs(6));
    assert_eq!(got[0].1.file_name, "three.asd");
}

fn retry<T, E: std::fmt::Debug>(mut f: impl FnMut() -> Result<T, E>) -> T {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match f() {
            Ok(v) => return v,
            Err(e) if Instant::now() > end => panic!("gave up: {e:?}"),
            Err(_) => sleep(Duration::from_millis(100)),
        }
    }
}

#[test]
fn event_loss_is_caught_by_the_rescan() {
    let d = TempDir::new("eventloss");
    let mut cfg = config(d.path());
    cfg.drop_native_events = true;
    cfg.timing.rescan_interval = Duration::from_secs(1);
    let (w, rx) = start(cfg);
    assert_eq!(w.status().mode, ActiveMode::Native);
    for i in 0..5 {
        fs::write(d.file(&format!("lost{i}.asd")), synthetic::as8(40 + i)).unwrap();
    }
    let got = arrivals(&rx, 5, Duration::from_secs(10));
    assert_eq!(got.len(), 5);
}

#[test]
fn poll_mode_on_a_network_volume_uses_the_long_settle() {
    let d = TempDir::new("poll");
    let mut cfg = config(d.path());
    cfg.force_volume = Some(VolumeKind::Network);
    cfg.drop_native_events = true; // no native watcher is created in POLL mode anyway
    let (w, rx) = start(cfg);
    let st = w.status();
    assert_eq!(st.mode, ActiveMode::Poll);
    assert_eq!(st.volume, VolumeKind::Network);
    assert!(st.mode_reason.contains("every 2 s"), "{}", st.mode_reason);
    let bytes = synthetic::as8(50);
    let done = slow_write(&d.file("net.asd"), &bytes, 2048, Duration::from_millis(100));
    let got = arrivals(&rx, 1, Duration::from_secs(15));
    assert_eq!(got.len(), 1);
    assert!(
        got[0].0 >= done + Duration::from_millis(1800),
        "network settle (2 s) not honoured"
    );
    assert_eq!(&got[0].1.bytes[..], &bytes[..]);
}

#[test]
fn manual_poll_mode_on_a_local_folder() {
    let d = TempDir::new("manualpoll");
    let mut cfg = config(d.path());
    cfg.mode = ModeChoice::Poll;
    let (w, rx) = start(cfg);
    assert_eq!(w.status().mode, ActiveMode::Poll);
    fs::write(d.file("a.asd"), synthetic::as8(51)).unwrap();
    assert_eq!(arrivals(&rx, 1, Duration::from_secs(10)).len(), 1);
}

#[test]
fn existing_files_included_or_baselined() {
    let d = TempDir::new("existing");
    fs::write(d.file("old1.asd"), synthetic::as8(60)).unwrap();
    fs::write(d.file("old2.asd"), synthetic::as8(61)).unwrap();

    let (w, rx) = start(config(d.path()));
    let got = arrivals(&rx, 2, Duration::from_secs(6));
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|(_, a)| a.existing));
    drop(w);

    let mut cfg = config(d.path());
    cfg.include_existing = false;
    let (_w, rx) = start(cfg);
    assert_no_more_arrivals(&rx, Duration::from_secs(2));
    fs::write(d.file("new.asd"), synthetic::as8(62)).unwrap();
    sleep(Duration::from_millis(50));
    fs::write(d.file("old2.asd"), synthetic::as8(63)).unwrap(); // an old file rewritten counts
    let got = arrivals(&rx, 2, Duration::from_secs(6));
    let mut names: Vec<_> = got
        .iter()
        .map(|(_, a)| (a.file_name.clone(), a.existing))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            ("new.asd".to_string(), false),
            ("old2.asd".to_string(), false)
        ]
    );
}

#[test]
fn reference_saves_and_unrecognised_bytes_are_delivered_and_labelled() {
    let d = TempDir::new("kinds");
    let (_w, rx) = start(config(d.path()));
    fs::write(d.file("whiteref.asd"), synthetic::as8_reference_save(70)).unwrap();
    fs::write(d.file("garbage.ASD"), b"this is not an ASD file at all").unwrap();
    let got = arrivals(&rx, 2, Duration::from_secs(6));
    assert_eq!(got.len(), 2);
    for (_, a) in &got {
        match a.file_name.as_str() {
            "whiteref.asd" => assert_eq!(a.kind_hint, Some(KindHint::WhiteReferenceSave)),
            "garbage.ASD" => {
                assert!(!a.recognised);
                assert!(a.detail.contains("not an ASD"), "{}", a.detail);
            }
            other => panic!("{other}"),
        }
    }
}

#[test]
fn a_file_that_never_completes_is_reported_then_delivered_when_finished() {
    let d = TempDir::new("stalled");
    let (_w, rx) = start(config(d.path()));
    let bytes = synthetic::as8(80);
    let p = d.file("stalled.asd");
    fs::write(&p, &bytes[..30_000]).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let mut reported = false;
    while Instant::now() < end && !reported {
        if let Ok((_, e)) = rx.recv_timeout(Duration::from_millis(200)) {
            match e {
                WatchEvent::Incomplete {
                    file_name, reason, ..
                } => {
                    assert_eq!(file_name, "stalled.asd");
                    assert!(reason.contains("will retry"), "{reason}");
                    reported = true;
                }
                WatchEvent::Arrived(a) => panic!("truncated file delivered: {}", a.file_name),
                _ => {}
            }
        }
    }
    assert!(reported, "never reported as incomplete");
    let mut f = OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(&bytes[30_000..]).unwrap();
    drop(f);
    let got = arrivals(&rx, 1, Duration::from_secs(8));
    assert_eq!(got.len(), 1);
    assert_eq!(&got[0].1.bytes[..], &bytes[..]);
}

/// Codex review of the assembled app, finding 5: a file that keeps changing past the give-up time never
/// settles, so the "incomplete, will retry" notice must come from the overall deadline while waiting for it to
/// settle. Reported exactly once, before the writer finishes; delivered once it settles.
#[test]
fn a_file_that_keeps_changing_is_reported_once_then_delivered_when_it_settles() {
    let d = TempDir::new("keepschanging");
    let (_w, rx) = start(config(d.path())); // give-up 4 s
    let bytes = synthetic::as8(81);
    let p = d.file("X10001_1.asd");
    // About 7 s of writing, a piece every 100 ms: it never stays unchanged for the 500 ms settle window.
    let writer = {
        let p = p.clone();
        let bytes = bytes.clone();
        std::thread::spawn(move || slow_write(&p, &bytes, 500, Duration::from_millis(100)))
    };
    writer.join().unwrap();
    let during = drain(&rx, Duration::from_millis(10));
    let reports: Vec<&String> = during
        .iter()
        .filter_map(|e| match e {
            WatchEvent::Incomplete {
                file_name, reason, ..
            } if file_name == "X10001_1.asd" => Some(reason),
            _ => None,
        })
        .collect();
    assert!(
        only_arrivals(&during).is_empty(),
        "delivered while still being written"
    );
    assert_eq!(
        reports.len(),
        1,
        "reported {} times: {reports:?}",
        reports.len()
    );
    assert!(reports[0].contains("will retry"), "{}", reports[0]);
    let after = drain(&rx, Duration::from_secs(4));
    let a = only_arrivals(&after);
    assert_eq!(a.len(), 1, "not delivered once it settled");
    assert_eq!(&a[0].bytes[..], &bytes[..]);
    assert!(
        !after
            .iter()
            .any(|e| matches!(e, WatchEvent::Incomplete { .. })),
        "reported again after it settled"
    );
}

#[test]
fn pause_holds_arrivals_until_resume() {
    let d = TempDir::new("pause");
    let (w, rx) = start(config(d.path()));
    w.pause();
    wait_status(&rx, WatchState::Paused, Duration::from_secs(2));
    fs::write(d.file("while_paused.asd"), synthetic::as8(90)).unwrap();
    assert_no_more_arrivals(&rx, Duration::from_secs(2));
    w.resume();
    let got = arrivals(&rx, 1, Duration::from_secs(6));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.file_name, "while_paused.asd");
}

#[test]
fn a_missing_folder_is_waited_for() {
    let d = TempDir::new("later");
    let sub = d.file("not-yet");
    let (w, rx) = start(config(&sub));
    assert_eq!(w.status().state, WatchState::FolderMissing);
    fs::create_dir(&sub).unwrap();
    wait_status(&rx, WatchState::Watching, Duration::from_secs(6));
    fs::write(sub.join("x.asd"), synthetic::as8(95)).unwrap();
    assert_eq!(arrivals(&rx, 1, Duration::from_secs(6)).len(), 1);
}

/// Codex review finding 3: a lock held past the give-up time, then released WITHOUT any size or mtime change,
/// in POLL mode (no events at all). The file is reported once, then delivered by the quiet periodic retry.
#[cfg(windows)]
#[test]
fn lock_released_after_give_up_without_any_change_is_still_delivered() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = TempDir::new("lockpoll");
    let p = d.file("held.asd");
    let bytes = synthetic::as8(98);
    fs::write(&p, &bytes).unwrap();
    // Hold it exclusively before watching starts, so its stat never changes afterwards.
    let f = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&p)
        .unwrap();
    let mut cfg = config(d.path());
    cfg.mode = ModeChoice::Poll;
    cfg.timing.give_up_network = Duration::from_secs(3);
    let (_w, rx) = start(cfg);
    let end = Instant::now() + Duration::from_secs(12);
    let mut reports = 0;
    while Instant::now() < end && reports == 0 {
        if let Ok((_, WatchEvent::Incomplete { reason, .. })) =
            rx.recv_timeout(Duration::from_millis(200))
        {
            assert!(reason.contains("open in another program"), "{reason}");
            reports += 1;
        }
    }
    assert_eq!(reports, 1, "never reported");
    drop(f);
    let ev = drain(&rx, Duration::from_secs(10));
    let a = only_arrivals(&ev);
    assert_eq!(a.len(), 1, "not delivered after the lock was released");
    assert_eq!(&a[0].bytes[..], &bytes[..]);
    let again = ev
        .iter()
        .filter(|e| matches!(e, WatchEvent::Incomplete { .. }))
        .count();
    assert_eq!(again, 0, "incomplete reported more than once");
}

/// A writer that holds the file open without sharing read access (some instrument software does): we retry
/// on the sharing violation and deliver once it lets go.
#[cfg(windows)]
#[test]
fn exclusive_writer_is_waited_for() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = TempDir::new("exclusive");
    let (_w, rx) = start(config(d.path()));
    let bytes = synthetic::as8(99);
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .share_mode(0)
        .open(d.file("locked.asd"))
        .unwrap();
    f.write_all(&bytes).unwrap();
    f.flush().unwrap();
    sleep(Duration::from_millis(2000));
    let early = drain(&rx, Duration::from_millis(10));
    assert!(only_arrivals(&early).is_empty());
    let released = Instant::now();
    drop(f);
    let got = arrivals(&rx, 1, Duration::from_secs(8));
    assert_eq!(got.len(), 1);
    assert!(got[0].0 >= released);
    assert_eq!(&got[0].1.bytes[..], &bytes[..]);
}
