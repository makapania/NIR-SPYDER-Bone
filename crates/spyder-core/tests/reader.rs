//! Public reader tests on synthetic files built in-test (as8, as7, legacy v1): supported matrix, every truncation
//! length, corrupt magic, wrong channel count, per-file joins (DECISIONS 87), mutations (never panic), folder listing.

#![allow(clippy::field_reassign_with_default, clippy::needless_range_loop)] // test fixtures read better this way

mod common;

use common::*;
use spyder_core::read::{read_bytes, ReadError};
use spyder_core::scan::ScanKind;

#[test]
fn reads_a_supported_file() {
    let spec = AsdSpec::default();
    let bytes = spec.build();
    assert_eq!(bytes.len(), 35_132, "same size as every real file");
    let scan = read_bytes(&bytes).expect("supported");
    let h = &scan.header;
    assert_eq!(h.version, "as8");
    assert_eq!(h.serial, 12345);
    assert_eq!((h.splice1_nm, h.splice2_nm), (1000.0, 1800.0));
    assert_eq!(h.integration_time_ms, 136);
    assert_eq!((h.swir1_gain, h.swir2_gain), (16, 21));
    assert_eq!((h.swir1_offset, h.swir2_offset), (2050, 2075));
    assert_eq!(
        (h.dark_averages, h.reference_averages, h.sample_averages),
        (100, 50, 50)
    );
    assert_eq!(h.instrument_type, 4);
    assert_eq!(scan.wavelengths_nm.len(), N);
    assert_eq!(scan.wavelengths_nm[0], 350.0);
    assert_eq!(scan.wavelengths_nm[N - 1], 2500.0);
    assert_eq!(scan.splices_nm, vec![1000.0, 1800.0]);
    let segs: Vec<(String, usize, usize)> = scan
        .segments
        .iter()
        .map(|s| (s.name.clone(), s.start, s.end))
        .collect();
    assert_eq!(
        segs,
        vec![
            ("VNIR".into(), 0, 651),
            ("SWIR1".into(), 651, 1451),
            ("SWIR2".into(), 1451, 2151)
        ]
    );
    // R = sample / reference, IEEE division, bit for bit
    for i in 0..N {
        assert_eq!(
            scan.reflectance[i].to_bits(),
            (spec.sample[i] / spec.reference[i]).to_bits()
        );
    }
    assert_eq!(scan.kind, ScanKind::Sample);
    assert!(scan.warnings.is_empty(), "{:?}", scan.warnings);
    let t = &scan.timestamps;
    assert_eq!(t.reference_local.as_deref(), Some("2026-09-08T13:41:48"));
    assert_eq!(t.spectrum_local.as_deref(), Some("2026-09-08T13:53:14"));
    assert_eq!(t.acquired_tm_local.as_deref(), Some("2026-09-08T13:53:14"));
    assert_eq!(t.utc_offset_minutes, Some(-240));
    assert!((t.reference_age_s.unwrap() - 686.0).abs() < 1e-3);
    assert_eq!(t.spectrum_ole.to_bits(), spec.spectrum_ole.to_bits());
}

#[test]
fn every_truncation_length_is_an_error() {
    let bytes = AsdSpec::default().build();
    // the last byte of the white-reference block: a file ending there has no trailer at all
    let blocks_end = bytes.len() - 212;
    for len in 0..bytes.len() {
        match read_bytes(&bytes[..len]) {
            Err(ReadError::Truncated { .. }) => {}
            // Phase 3 (oracle parity): no trailer at all is accepted, with a log warning
            Ok(scan) if len == blocks_end => {
                assert!(scan.warnings.iter().any(|w| w.contains("no as8 trailer")));
            }
            other => panic!("length {len}: expected Truncated, got {other:?}"),
        }
    }
    assert!(read_bytes(&bytes).is_ok());
}

#[test]
fn longer_trailer_is_accepted_with_a_note() {
    let mut spec = AsdSpec::default();
    spec.trailer = vec![0u8; 212 + 8 * N];
    spec.trailer[300] = 7;
    let scan = read_bytes(&spec.build()).expect("longer trailer");
    assert!(scan.warnings.iter().any(|w| w.contains("trailer")));
}

#[test]
fn reference_description_is_read() {
    let mut spec = AsdSpec::default();
    spec.description = b"panel 1".to_vec();
    let scan = read_bytes(&spec.build()).unwrap();
    assert_eq!(scan.reference_description, "panel 1");
    // a description length running past the end is a truncation, not a panic
    let mut bytes = AsdSpec::default().build();
    let off = 484 + 8 * N + 18;
    bytes[off..off + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
    assert!(matches!(
        read_bytes(&bytes),
        Err(ReadError::Truncated { .. })
    ));
}

fn expect_unsupported(spec: AsdSpec, needle: &str) {
    match read_bytes(&spec.build()) {
        Err(ReadError::Unsupported { reason }) => {
            assert!(
                reason.contains(needle),
                "{reason:?} should mention {needle:?}"
            )
        }
        other => panic!("expected Unsupported({needle}), got {other:?}"),
    }
}

#[test]
fn outside_the_matrix_is_unsupported() {
    expect_unsupported(
        AsdSpec {
            magic: *b"as6",
            ..Default::default()
        },
        "version",
    );
    expect_unsupported(
        AsdSpec {
            data_type: 1,
            ..Default::default()
        },
        "not a raw",
    );
    expect_unsupported(
        AsdSpec {
            data_format: 0,
            ..Default::default()
        },
        "data format",
    );
    expect_unsupported(
        AsdSpec {
            dark_corrected: 0,
            ..Default::default()
        },
        "dark correction",
    );
    expect_unsupported(
        AsdSpec {
            channels: 2150,
            ..Default::default()
        },
        "channels",
    );
    expect_unsupported(
        AsdSpec {
            first_nm: 351.0,
            ..Default::default()
        },
        "grid",
    );
    expect_unsupported(
        AsdSpec {
            step_nm: 2.0,
            ..Default::default()
        },
        "grid",
    );
    expect_unsupported(
        AsdSpec {
            ref_flag: 0,
            ..Default::default()
        },
        "reference flag",
    );
}

#[test]
fn corrupt_magic_is_not_asd() {
    let mut bytes = AsdSpec::default().build();
    bytes[0..3].copy_from_slice(b"PK\x03");
    assert!(matches!(read_bytes(&bytes), Err(ReadError::NotAsd { .. })));
    assert!(matches!(read_bytes(b"xyz"), Err(ReadError::NotAsd { .. })));
}

#[test]
fn bad_reference_is_invalid() {
    let mut spec = AsdSpec::default();
    spec.reference[idx(1500.0)] = 0.0;
    assert!(matches!(
        read_bytes(&spec.build()),
        Err(ReadError::Invalid { .. })
    ));
    let mut spec = AsdSpec::default();
    spec.reference[idx(2450.0)] = f64::NAN;
    assert!(matches!(
        read_bytes(&spec.build()),
        Err(ReadError::Invalid { .. })
    ));
    let mut spec = AsdSpec::default();
    spec.reference[idx(1000.0)] = -3.0;
    assert!(matches!(
        read_bytes(&spec.build()),
        Err(ReadError::Invalid { .. })
    ));
    // outside 1000-2450 nm a zero reference is allowed (R becomes inf/NaN there, as numpy gives)
    let mut spec = AsdSpec::default();
    spec.reference[idx(400.0)] = 0.0;
    spec.sample[idx(400.0)] = 0.0;
    let scan = read_bytes(&spec.build()).unwrap();
    assert!(scan.reflectance[idx(400.0)].is_nan());
    // flag bytes that are neither TRUE nor FALSE
    let spec = AsdSpec {
        ref_flag: 0x0001,
        ..Default::default()
    };
    assert!(matches!(
        read_bytes(&spec.build()),
        Err(ReadError::Invalid { .. })
    ));
}

#[test]
fn reference_saves_only_from_identical_blocks_never_from_brightness() {
    // a white-reference save: sample DN block identical to the reference DN block
    let mut spec = AsdSpec::default();
    spec.sample = spec.reference.clone();
    let scan = read_bytes(&spec.build()).unwrap();
    assert_eq!(scan.kind, ScanKind::WhiteReferenceSave);
    assert_eq!(scan.kind.label(), "reference scan (not scored)");
    // Codex Phase 1 MEDIUM 2: a flat near-white SAMPLE (R ~ 1 within noise) stays a sample and B4 fails it
    let r: Vec<f64> = wl()
        .iter()
        .map(|w| 1.0 + 0.003 * (w * 0.37).sin())
        .collect();
    let read = read_bytes(&AsdSpec::default().with_reflectance(&r).build());
    assert_eq!(read.as_ref().unwrap().kind, ScanKind::Sample);
    let b4 = spyder_core::qc::b4_panel_or_empty(read.as_ref().unwrap());
    assert_eq!(b4.outcome, Some(spyder_core::qc::Outcome::Unusable));
    // a very dark sample (|R| < 0.01) is NOT a dark save: B4 (empty probe) marks it Unusable
    let r: Vec<f64> = wl().iter().map(|w| 1e-4 * (w * 0.91).sin()).collect();
    let read = read_bytes(&AsdSpec::default().with_reflectance(&r).build());
    let scan = read.as_ref().unwrap();
    assert_eq!(scan.kind, ScanKind::Sample);
    assert_eq!(
        spyder_core::qc::b4_panel_or_empty(scan).outcome,
        Some(spyder_core::qc::Outcome::Unusable)
    );
    // ... and a dark sample with R < 0 reaches B3
    let r: Vec<f64> = wl()
        .iter()
        .map(|w| -2e-3 + 1e-4 * (w * 0.91).sin())
        .collect();
    let read = read_bytes(&AsdSpec::default().with_reflectance(&r).build());
    let scan = read.as_ref().unwrap();
    assert_eq!(scan.kind, ScanKind::Sample);
    assert_eq!(
        spyder_core::qc::b3_impossible_reflectance(scan).outcome,
        Some(spyder_core::qc::Outcome::Unusable)
    );
    // ordinary bright bone is a sample
    let r = vec![0.7; N];
    let spec = AsdSpec::default().with_reflectance(&r);
    assert_eq!(read_bytes(&spec.build()).unwrap().kind, ScanKind::Sample);
}

#[test]
fn header_join_not_at_reference_jump_is_logged() {
    let mut spec = AsdSpec::default();
    // move the white-reference DN step from 1800/1801 to 1810/1811 nm
    let base = spec.reference.clone();
    for i in idx(1801.0)..=idx(1810.0) {
        spec.reference[i] = base[i] / 0.6;
    }
    let scan = read_bytes(&spec.build()).unwrap();
    assert!(
        scan.warnings.iter().any(|w| w.contains("splice mismatch")),
        "{:?}",
        scan.warnings
    );
}

/// Deterministic xorshift.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn mutations_never_panic() {
    let base = AsdSpec::default().build();
    let mut rng = Rng(0xDEADBEEF);
    let mut outcomes = std::collections::BTreeMap::new();
    for _ in 0..4000 {
        let mut b = base.clone();
        let k = 1 + (rng.next() % 4) as usize;
        for _ in 0..k {
            // bias mutations towards the header and the reference header, where structure lives
            let pos = match rng.next() % 3 {
                0 => (rng.next() % 484) as usize,
                1 => 484 + 8 * N + (rng.next() % 20) as usize,
                _ => (rng.next() % b.len() as u64) as usize,
            };
            b[pos] = (rng.next() & 0xFF) as u8;
        }
        if rng.next().is_multiple_of(5) {
            let cut = (rng.next() % b.len() as u64) as usize;
            b.truncate(cut);
        }
        let tag = match read_bytes(&b) {
            Ok(_) => "ok",
            Err(e) => e.kind(),
        };
        *outcomes.entry(tag).or_insert(0) += 1;
    }
    // every outcome is a value, never a panic; and the mutations exercised several error paths
    assert!(outcomes.len() >= 4, "{outcomes:?}");
}

#[test]
fn json_serialisation_of_a_scan() {
    let scan = read_bytes(&AsdSpec::default().build()).unwrap();
    let v = serde_json::to_value(&scan.header).unwrap();
    assert_eq!(v["serial"], 12345);
    assert_eq!(v["splice2_nm"], 1800.0);
}

// ---------------------------------------------------------------------------------------------- other instruments
// DECISIONS 87-88: Trek-type as7, FieldSpec 3 legacy v1 reflectance, joins resolved per file.

use spyder_core::read::asd::{detect_joins, resolve_joins, JOIN_WINDOWS_NM};
use spyder_core::scan::FileFormat;

/// The default white reference with its SWIR1/SWIR2 step moved from 1800/1801 to `j2`/`j2 + 1` (j2 < 1800).
fn reference_with_upper_join(j2: f64) -> Vec<f64> {
    wl().iter()
        .zip(reference_dn())
        .map(|(&w, v)| if w > j2 && w <= 1800.0 { v * 0.6 } else { v })
        .collect()
}

#[test]
fn plausible_header_joins_are_used_as_they_are() {
    let mut spec = AsdSpec::default();
    spec.splices = [1000.0, 1830.0]; // the FieldSpec 3 manual split
    let scan = read_bytes(&spec.build()).expect("1830 lies in its window");
    assert_eq!(scan.splices_nm, vec![1000.0, 1830.0]);
    assert_eq!(scan.joins_source, "header");
    assert_eq!(scan.segments[1].last_nm, 1830.0);
}

#[test]
fn implausible_header_joins_are_found_in_the_white_reference() {
    for header in [
        [0.0f32, 0.0],
        [f32::NAN, 1800.0],
        [1000.0, 2000.0],
        [900.0, 1800.0],
    ] {
        let mut spec = AsdSpec::default();
        spec.splices = header;
        spec.reference = reference_with_upper_join(1785.0);
        let spec = spec.with_reflectance(&bone_reflectance());
        let scan = read_bytes(&spec.build()).expect("joins resolved, never rejected");
        assert_eq!(scan.joins_source, "reference_dn", "{header:?}");
        assert_eq!(scan.splices_nm, vec![1000.0, 1785.0], "{header:?}");
        assert_eq!(scan.splices_header_nm[0].is_nan(), header[0].is_nan());
        assert!(scan.warnings.iter().any(|w| w.contains("not usable")));
    }
}

#[test]
fn no_clear_step_falls_back_to_the_default_joins() {
    let mut spec = AsdSpec::default();
    spec.splices = [0.0, 0.0];
    spec.reference = wl().iter().map(|&w| 20_000.0 + w).collect(); // smooth: no detector steps
    let spec = spec.with_reflectance(&bone_reflectance());
    let scan = read_bytes(&spec.build()).expect("read");
    assert_eq!(scan.joins_source, "default");
    assert_eq!(scan.splices_nm, vec![1000.0, 1800.0]);
    // detection itself: the steps of the default reference sit at 1000 and 1800
    assert_eq!(
        detect_joins(&wl(), &reference_dn()),
        Some(vec![1000.0, 1800.0])
    );
    assert_eq!(
        resolve_joins(&[0.0, 0.0], &wl(), None),
        (vec![1000.0, 1800.0], "default")
    );
    assert_eq!(JOIN_WINDOWS_NM, [(950.0, 1050.0), (1700.0, 1900.0)]);
}

fn trek_spec() -> AsdSpec {
    let mut spec = AsdSpec::default();
    spec.dark_corrected = 0; // as the Trek-type files are
    spec.splices = [0.0, 0.0];
    spec.swir_gains = [390, 396];
    spec.reference = reference_with_upper_join(1785.0);
    spec.trailer = vec![1u8; 300];
    spec.with_reflectance(&bone_reflectance())
}

#[test]
fn reads_a_trek_type_as7_file() {
    let spec = trek_spec();
    let scan = read_bytes(&spec.build_as7(1)).expect("as7");
    assert_eq!(scan.format, FileFormat::As7);
    assert_eq!(scan.header.version, "as7");
    assert_eq!(scan.splices_nm, vec![1000.0, 1785.0]);
    assert_eq!(scan.joins_source, "reference_dn");
    let (s, r) = (
        scan.sample_dn.as_ref().unwrap(),
        scan.reference_dn.as_ref().unwrap(),
    );
    for i in 0..N {
        assert_eq!(scan.reflectance[i].to_bits(), (s[i] / r[i]).to_bits());
        assert_eq!(r[i].to_bits(), spec.reference[i].to_bits());
    }
    assert_eq!(
        scan.timestamps.spectrum_ole.to_bits(),
        spec.spectrum_ole.to_bits()
    );
    assert_eq!(scan.timestamps.utc_offset_minutes, Some(-240));
    assert_eq!(scan.kind, ScanKind::Sample);
    assert!(scan.warnings.iter().any(|w| w.contains("as7 trailer")));
}

#[test]
fn as7_reference_flag_and_truncation() {
    let spec = trek_spec();
    assert!(matches!(
        read_bytes(&spec.build_as7(0)),
        Err(ReadError::Unsupported { reason }) if reason.contains("reference flag")
    ));
    assert!(matches!(
        read_bytes(&spec.build_as7(2)),
        Err(ReadError::Invalid { .. })
    ));
    let mut no_trailer = spec.clone();
    no_trailer.trailer.clear();
    let bytes = no_trailer.build_as7(1);
    for len in 0..bytes.len() {
        assert!(
            matches!(read_bytes(&bytes[..len]), Err(ReadError::Truncated { .. })),
            "length {len}"
        );
    }
    assert!(read_bytes(&bytes).is_ok());
    // a white-reference save in as7 is recognised as in as8
    let mut save = spec.clone();
    save.sample = save.reference.clone();
    assert_eq!(
        read_bytes(&save.build_as7(1)).unwrap().kind,
        ScanKind::WhiteReferenceSave
    );
}

#[test]
fn reads_a_legacy_reflectance_file() {
    let r = bone_reflectance();
    let spec = AsdSpec::default();
    let bytes = spec.build_legacy(&r);
    assert_eq!(bytes.len(), 9088, "same size as the FieldSpec 3 files");
    let scan = read_bytes(&bytes).expect("legacy");
    assert_eq!(scan.format, FileFormat::Legacy);
    assert_eq!(scan.header.version, "ASD");
    assert!(scan.sample_dn.is_none() && scan.reference_dn.is_none());
    for i in 0..N {
        assert_eq!(scan.reflectance[i], f64::from(r[i] as f32));
    }
    assert_eq!(scan.splices_nm, vec![1000.0, 1800.0]);
    assert_eq!(scan.joins_source, "header");
    assert_eq!(scan.kind, ScanKind::Sample);
    assert!(scan.timestamps.spectrum_local.is_none());
    for len in 0..bytes.len() {
        assert!(
            matches!(read_bytes(&bytes[..len]), Err(ReadError::Truncated { .. })),
            "length {len}"
        );
    }
    // a ViewSpec .sco carries flag byte 0x04: noted, read the same way
    let mut sco = bytes.clone();
    sco[421] = 4;
    let s = read_bytes(&sco).unwrap();
    assert!(s.warnings.iter().any(|w| w.contains(".sco")));
    // raw counts without a white reference, or integer data, are outside the matrix
    let mut raw = bytes.clone();
    raw[186] = 0;
    assert!(matches!(
        read_bytes(&raw),
        Err(ReadError::Unsupported { reason }) if reason == "not a reflectance file"
    ));
    let mut int = bytes.clone();
    int[199] = 1;
    assert!(matches!(
        read_bytes(&int),
        Err(ReadError::Unsupported { .. })
    ));
    // implausible header joins on a legacy file: the default joins (no white reference to look at)
    let mut odd = spec.clone();
    odd.splices = [0.0, 0.0];
    let s = read_bytes(&odd.build_legacy(&r)).unwrap();
    assert_eq!(
        (s.splices_nm.clone(), s.joins_source),
        (vec![1000.0, 1800.0], "default")
    );
}

#[test]
fn other_formats_never_panic_under_mutation() {
    let r = bone_reflectance();
    let mut rng = Rng(0xC0FFEE);
    for base in [
        trek_spec().build_as7(1),
        AsdSpec::default().build_legacy(&r),
    ] {
        for _ in 0..3000 {
            let mut b = base.clone();
            for _ in 0..1 + (rng.next() % 4) as usize {
                let pos = match rng.next() % 3 {
                    0 => (rng.next() % 484) as usize,
                    1 => (484 + 8 * N + (rng.next() % 20) as usize).min(b.len() - 1),
                    _ => (rng.next() % b.len() as u64) as usize,
                };
                b[pos] = (rng.next() & 0xFF) as u8;
            }
            if rng.next().is_multiple_of(5) {
                let cut = (rng.next() % b.len() as u64) as usize;
                b.truncate(cut);
            }
            let _ = read_bytes(&b);
            let _ = spyder_core::read::asd::probe(&b);
        }
    }
}

#[test]
fn folder_listing_takes_all_scan_names_and_skips_shadowed_sco() {
    use spyder_core::read::{is_scan_name, list_asd_files};
    for (name, want) in [
        ("a.asd", true),
        ("A.ASD", true),
        ("spot.000", true),
        ("spot.000.sco", true),
        ("x.SCO", true),
        ("x.12", false),
        ("x.1234", false),
        ("x.00a", false),
        ("notes.txt", false),
        (".asd", false),
        ("asd", false),
    ] {
        assert_eq!(is_scan_name(name), want, "{name}");
    }
    let dir = std::env::temp_dir().join(format!("spyder_listing_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for f in [
        "a.000",
        "a.000.sco",
        "b.001.sco",
        "c.asd",
        "d.txt",
        "e.123",
        "._f.asd",
    ] {
        std::fs::write(dir.join(f), b"x").unwrap();
    }
    let got: Vec<String> = list_asd_files(&dir)
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(got, ["a.000", "b.001.sco", "c.asd", "e.123"]);
}
