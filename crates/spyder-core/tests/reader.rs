//! Public reader tests on synthetic as8 files built in-test: supported matrix, every truncation length,
//! corrupt magic, wrong channel count, other joins, mutations (never panic).

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
            splices: [1000.0, 1830.0],
            ..Default::default()
        },
        "joins",
    );
    expect_unsupported(
        AsdSpec {
            splices: [990.0, 1800.0],
            ..Default::default()
        },
        "joins",
    );
    expect_unsupported(
        AsdSpec {
            splices: [f32::NAN, 1800.0],
            ..Default::default()
        },
        "joins",
    );
    expect_unsupported(
        AsdSpec {
            magic: *b"as7",
            ..Default::default()
        },
        "version",
    );
    expect_unsupported(
        AsdSpec {
            data_type: 1,
            ..Default::default()
        },
        "data type",
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
        "dark-correction",
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
