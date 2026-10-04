//! ASD `as8` reader for the supported-input matrix (PLAN section 3 Step 1; 03 sections 3.2-3.4; DECISIONS 43).
//!
//! Layout (little-endian): 484-byte header; sample block (`channels` x f64); reference header
//! (2-byte BOOL flag, f64 OLE reference time, f64 OLE spectrum time, u16 description length, description);
//! white-reference block (`channels` x f64); v8 trailer (212 bytes, all zero, in every file in hand).
//!
//! Accepted (anything else is a typed error, never a guess):
//! * version `as8`; data type RAW (0) with the reference flag set; data format f64 (2);
//! * dark-correction flag set (both blocks already dark-corrected; never subtract again);
//! * grid 350 nm + 1 nm x 2151 channels; header joins exactly 1000 and 1800 nm;
//! * white-reference DN finite and > 0 in 1000-2450 nm;
//! * the whole structure present (a trailer, when present, of at least [`MIN_TRAILER_LEN`] bytes), so a file
//!   truncated at any length is an error.

use super::ReadError;
use crate::scan::{AsdHeader, Scan, ScanKind, Segment, Timestamps};
use crate::timefmt;

/// Spectrum header length.
pub const HEADER_LEN: usize = 484;
/// Reference header without the description: BOOL flag (2) + two f64 OLE dates (16) + u16 length (2).
pub const REFERENCE_HEADER_LEN: usize = 20;
/// The v8 trailer (classifier data, dependent variables, calibration header, audit log, signature) is
/// 212 bytes in all 267 files in hand (all zero). It is not parsed; it must be present, so a truncated
/// file is never accepted. Longer trailers (stored calibration spectra, audit log) are accepted.
pub const MIN_TRAILER_LEN: usize = 212;
/// Supported grid.
pub const CHANNELS: usize = 2151;
pub const FIRST_NM: f32 = 350.0;
pub const STEP_NM: f32 = 1.0;
/// Supported detector joins (DECISIONS 43).
pub const SUPPORTED_SPLICES_NM: [f32; 2] = [1000.0, 1800.0];
/// Region where the reference must be usable and where the acquisition checks look.
pub const SCORED_RANGE_NM: (f64, f64) = (1000.0, 2450.0);

// Reference-save recognition (Codex Phase 1 review, MEDIUM 2): the as8 header has no verified field that marks a
// white-reference or dark save, so brightness is NEVER used to classify a file (a very dark or flat near-white
// SAMPLE must reach B3/B4, which mark it Unusable). The only recognised save is the structural identity
// "sample DN block == white-reference DN block, bit for bit", which no measured sample can produce.
/// Half-width (channels) of the window searched for the largest white-reference DN jump at each join.
pub const SPLICE_CROSSCHECK_HALF_WIDTH: usize = 30;

struct Bytes<'a> {
    b: &'a [u8],
}

impl Bytes<'_> {
    fn u8(&self, off: usize) -> u8 {
        self.b[off]
    }
    fn arr<const N: usize>(&self, off: usize) -> [u8; N] {
        let mut a = [0u8; N];
        a.copy_from_slice(&self.b[off..off + N]);
        a
    }
    fn u16(&self, off: usize) -> u16 {
        u16::from_le_bytes(self.arr(off))
    }
    fn i16(&self, off: usize) -> i16 {
        i16::from_le_bytes(self.arr(off))
    }
    fn u32(&self, off: usize) -> u32 {
        u32::from_le_bytes(self.arr(off))
    }
    fn f32(&self, off: usize) -> f32 {
        f32::from_le_bytes(self.arr(off))
    }
    fn f64(&self, off: usize) -> f64 {
        f64::from_le_bytes(self.arr(off))
    }
    fn f64_block(&self, off: usize, n: usize) -> Vec<f64> {
        (0..n).map(|i| self.f64(off + 8 * i)).collect()
    }
}

fn need(bytes: &[u8], needed: usize, what: &'static str) -> Result<(), ReadError> {
    if bytes.len() < needed {
        Err(ReadError::Truncated {
            what,
            needed,
            got: bytes.len(),
        })
    } else {
        Ok(())
    }
}

fn unsupported(reason: impl Into<String>) -> ReadError {
    ReadError::Unsupported {
        reason: reason.into(),
    }
}

fn parse_header(r: &Bytes) -> AsdHeader {
    AsdHeader {
        version: String::from_utf8_lossy(&r.b[0..3]).to_string(),
        program_version: r.u8(178),
        file_version: r.u8(179),
        dark_corrected: r.u8(181),
        data_type: r.u8(186),
        data_format: r.u8(199),
        first_wavelength_nm: r.f32(191),
        wavelength_step_nm: r.f32(195),
        channels: r.u16(204),
        integration_time_ms: r.u32(390),
        fore_optic: r.i16(394),
        calibration_series: r.u16(398),
        serial: r.u16(400),
        ad_bits: r.u16(418),
        flags: r.arr(421),
        dark_averages: r.u16(425),
        reference_averages: r.u16(427),
        sample_averages: r.u16(429),
        instrument_type: r.u8(431),
        swir1_gain: r.u16(436),
        swir2_gain: r.u16(438),
        swir1_offset: r.u16(440),
        swir2_offset: r.u16(442),
        splice1_nm: r.f32(444),
        splice2_nm: r.f32(448),
    }
}

/// Check the supported-input matrix on the header alone.
fn check_matrix(h: &AsdHeader) -> Result<(), ReadError> {
    if h.data_type != 0 {
        return Err(unsupported(format!(
            "data type {} (only RAW = 0, sample DN with a stored white reference, is supported)",
            h.data_type
        )));
    }
    if h.data_format != 2 {
        return Err(unsupported(format!(
            "data format {} (only float64 = 2 is supported)",
            h.data_format
        )));
    }
    if h.dark_corrected != 1 {
        return Err(unsupported(format!(
            "dark-correction flag {} (only dark-corrected files, flag 1, are supported)",
            h.dark_corrected
        )));
    }
    if usize::from(h.channels) != CHANNELS {
        return Err(unsupported(format!(
            "{} channels (only the 2151-channel grid is supported)",
            h.channels
        )));
    }
    if h.first_wavelength_nm != FIRST_NM || h.wavelength_step_nm != STEP_NM {
        return Err(unsupported(format!(
            "grid starts at {} nm with step {} nm (only 350 nm + 1 nm is supported)",
            h.first_wavelength_nm, h.wavelength_step_nm
        )));
    }
    if [h.splice1_nm, h.splice2_nm] != SUPPORTED_SPLICES_NM {
        // the oracle's B1 text (reference/oracle/asd.py `acceptance`), so exports agree word for word
        return Err(unsupported(format!(
            "detector joins [{:?}, {:?}] (only [1000.0, 1800.0] are supported)",
            f64::from(h.splice1_nm),
            f64::from(h.splice2_nm)
        )));
    }
    Ok(())
}

/// Detector segments from the joins: VNIR = wl <= j1, SWIR1 = j1 < wl <= j2, SWIR2 = wl > j2.
pub fn segments(wl: &[f64], joins: &[f64]) -> Vec<Segment> {
    let names = ["VNIR", "SWIR1", "SWIR2"];
    let ranges = crate::grid::segment_ranges(wl, joins);
    ranges
        .iter()
        .enumerate()
        .map(|(k, &(a, b))| Segment {
            name: names
                .get(k)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("SEG{}", k + 1)),
            start: a,
            end: b,
            first_nm: wl[a],
            last_nm: wl[b - 1],
        })
        .collect()
}

fn in_scored(wl: f64) -> bool {
    wl >= SCORED_RANGE_NM.0 && wl <= SCORED_RANGE_NM.1
}

/// Mean and population SD of the values whose wavelength is in 1000-2450 nm.
pub(crate) fn scored_mean_sd(wl: &[f64], x: &[f64]) -> (f64, f64) {
    let v: Vec<f64> = wl
        .iter()
        .zip(x)
        .filter(|(w, _)| in_scored(**w))
        .map(|(_, &x)| x)
        .collect();
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    (mean, var.sqrt())
}

/// Recognise a white-reference save: the sample DN block equals the white-reference DN block bit for bit.
/// No brightness rule: a dark or panel-like sample stays a Sample and B3/B4 decide (Unusable).
/// Dark saves cannot be recognised from verified header metadata, so they are never inferred.
pub fn classify_kind(sample: &[f64], reference: &[f64]) -> ScanKind {
    let identical = sample.len() == reference.len()
        && !sample.is_empty()
        && sample
            .iter()
            .zip(reference)
            .all(|(a, b)| a.to_bits() == b.to_bits());
    if identical {
        ScanKind::WhiteReferenceSave
    } else {
        ScanKind::Sample
    }
}

/// Where the white-reference DN jumps most within +/- 30 channels of each header join; a warning when that
/// is not at the join (03 section 3.4: informational, never blocks).
fn splice_crosscheck(wl: &[f64], reference: &[f64], joins: &[f64]) -> Vec<String> {
    let mut out = Vec::new();
    for &j in joins {
        let Some(i) = crate::grid::last_le(wl, j) else {
            continue;
        };
        let lo = i.saturating_sub(SPLICE_CROSSCHECK_HALF_WIDTH);
        let hi = (i + SPLICE_CROSSCHECK_HALF_WIDTH).min(wl.len().saturating_sub(2));
        let mut best: Option<(usize, f64)> = None;
        for k in lo..=hi {
            let den = reference[k].abs();
            if den <= 0.0 || !den.is_finite() {
                continue;
            }
            let jump = (reference[k + 1] - reference[k]).abs() / den;
            if jump.is_finite() && best.is_none_or(|(_, b)| jump > b) {
                best = Some((k, jump));
            }
        }
        if let Some((k, _)) = best {
            if k != i {
                out.push(format!(
                    "splice mismatch: header join {j} nm, largest white-reference DN jump after {} nm",
                    wl[k]
                ));
            }
        }
    }
    out
}

/// Read an `as8` file from bytes. Never panics: every structure is length-checked first.
pub fn read_as8(bytes: &[u8]) -> Result<Scan, ReadError> {
    need(bytes, 3, "magic")?;
    let magic = &bytes[0..3];
    if magic != b"as8" {
        let m = String::from_utf8_lossy(magic).to_string();
        return Err(match magic {
            b"as6" | b"as7" | b"as5" | b"as4" | b"as3" | b"as2" | b"as1" | b"ASD" | b"asd" => {
                unsupported(format!("ASD file version {m:?} (only as8 is supported)"))
            }
            _ => ReadError::NotAsd { magic: m },
        });
    }
    need(bytes, HEADER_LEN, "header")?;
    let r = Bytes { b: bytes };
    let header = parse_header(&r);
    check_matrix(&header)?;

    let n = usize::from(header.channels);
    let sample_off = HEADER_LEN;
    let refhdr_off = sample_off + 8 * n;
    need(bytes, refhdr_off, "sample block")?;
    need(bytes, refhdr_off + REFERENCE_HEADER_LEN, "reference header")?;
    let flag = r.u16(refhdr_off);
    match flag {
        0xFFFF => {}
        0 => {
            return Err(unsupported(
                "reference flag not set (no stored white reference)",
            ))
        }
        other => {
            return Err(ReadError::Invalid {
                reason: format!("reference flag bytes {other:#06x} (expected 0xffff)"),
            })
        }
    }
    let reference_ole = r.f64(refhdr_off + 2);
    let spectrum_ole = r.f64(refhdr_off + 10);
    let desc_len = usize::from(r.u16(refhdr_off + 18));
    let desc_off = refhdr_off + REFERENCE_HEADER_LEN;
    let ref_off = desc_off + desc_len;
    need(bytes, ref_off, "reference description")?;
    let reference_description: String = bytes[desc_off..ref_off]
        .iter()
        .map(|&c| char::from(c))
        .collect();
    let trailer_off = ref_off + 8 * n;
    need(bytes, trailer_off, "reference block")?;
    // The trailer is not parsed. A file that ends exactly after the reference block (no trailer at all, as the
    // oracle's golden writer produces) has every block the reader needs: accepted with a log warning. A PARTIAL
    // trailer means the file was cut short: rejected (Phase 3; keeps parity with the oracle reader).
    let has_trailer = bytes.len() > trailer_off;
    if has_trailer {
        need(bytes, trailer_off + MIN_TRAILER_LEN, "as8 trailer")?;
    }

    let wavelengths_nm: Vec<f64> = (0..n)
        .map(|i| {
            f64::from(header.first_wavelength_nm) + f64::from(header.wavelength_step_nm) * i as f64
        })
        .collect();
    let sample_dn = r.f64_block(sample_off, n);
    let reference_dn = r.f64_block(ref_off, n);

    for (w, &v) in wavelengths_nm.iter().zip(&reference_dn) {
        if in_scored(*w) && !(v.is_finite() && v > 0.0) {
            return Err(ReadError::Invalid {
                reason: format!(
                    "white reference is not finite and > 0 at {w} nm (value {v}); reflectance cannot be computed"
                ),
            });
        }
    }

    // R = sample / reference, element-wise IEEE division (bitwise equal to numpy `tgt / ref`).
    let reflectance: Vec<f64> = sample_dn
        .iter()
        .zip(&reference_dn)
        .map(|(s, w)| s / w)
        .collect();

    let splices_nm = vec![f64::from(header.splice1_nm), f64::from(header.splice2_nm)];
    let segments = segments(&wavelengths_nm, &splices_nm);

    let mut warnings = splice_crosscheck(&wavelengths_nm, &reference_dn, &splices_nm);
    if !has_trailer {
        warnings.push("no as8 trailer (the file ends after the white-reference block)".to_string());
    }
    if bytes[trailer_off..].iter().any(|&b| b != 0) {
        warnings.push(format!(
            "as8 trailer holds {} bytes with data (not parsed)",
            bytes.len() - trailer_off
        ));
    }
    if header.flags != [0, 0, 0, 0] {
        warnings.push(format!(
            "header flag bytes 421-424 are {:02x?} (meaning unverified; see check B2)",
            header.flags
        ));
    }

    let mut tm = [0i16; 9];
    for (k, v) in tm.iter_mut().enumerate() {
        *v = r.i16(160 + 2 * k);
    }
    let reference_time_t = r.u32(187);
    let reference_age_s =
        if timefmt::ole_plausible(spectrum_ole) && timefmt::ole_plausible(reference_ole) {
            Some((spectrum_ole - reference_ole) * 86_400.0)
        } else {
            None
        };
    let timestamps = Timestamps {
        spectrum_ole,
        spectrum_local: timefmt::ole_to_string(spectrum_ole),
        reference_ole,
        reference_local: timefmt::ole_to_string(reference_ole),
        acquired_tm_local: timefmt::tm_to_string(&tm),
        tm_isdst: tm[8],
        reference_time_t,
        dark_time_t: r.u32(182),
        utc_offset_minutes: timefmt::utc_offset_minutes(reference_ole, reference_time_t),
        reference_age_s,
    };

    let kind = classify_kind(&sample_dn, &reference_dn);

    Ok(Scan {
        reader: "asd",
        header,
        timestamps,
        reference_description,
        wavelengths_nm,
        splices_nm,
        segments,
        reflectance,
        sample_dn,
        reference_dn,
        kind,
        warnings,
    })
}
