//! ASD reader for the supported-input matrix (PLAN section 3 Step 1; 03 sections 3.2-3.4; DECISIONS 43, 87).
//!
//! Three formats share the 484-byte header (little-endian):
//! * `as8` (LabSpec 4): sample block (`channels` x f64 DN); reference header (2-byte BOOL flag 0xFFFF, f64 OLE
//!   reference time, f64 OLE spectrum time, u16 description length, description); white-reference block
//!   (`channels` x f64); v8 trailer (212 bytes, all zero, in every file in hand).
//! * `as7` (Trek-type handheld; layout verified on one unit): sample block (f64 DN); reference header (u8 flag 1,
//!   f64 OLE reference time at +1, f64 OLE spectrum time at +9, u16 description length at +17, one reserved byte);
//!   the description and then the white-reference block (f64) from +20; a trailer that is not parsed.
//! * `ASD` (legacy v1: FieldSpec 3 `.NNN` and ViewSpec `.sco`): reflectance stored directly (data type 1, f32 or
//!   f64); no white reference and no raw DN.
//!
//! Accepted (anything else is a typed error, never a guess):
//! * `as8`/`as7`: data type RAW (0) with the reference flag set, data format f64 (2); `as8` also needs the
//!   dark-correction flag (both blocks already dark-corrected; never subtract again);
//! * `ASD`: data type REF (1), data format f32 (0) or f64 (2);
//! * grid 350 nm + 1 nm x 2151 channels;
//! * raw files: white-reference DN finite and > 0 in 1000-2450 nm;
//! * the whole structure present (an as8 trailer, when present, of at least [`MIN_TRAILER_LEN`] bytes), so a file
//!   truncated at any length is an error.
//!
//! Detector joins (DECISIONS 87) are never a reason to reject a file: the header's joins when each lies in its
//! window ([`JOIN_WINDOWS_NM`]), else the largest clear white-reference DN jump in each window, else 1000/1800.
//! [`Scan::joins_source`] says which.

use super::ReadError;
use crate::scan::{AsdHeader, FileFormat, Scan, ScanKind, Segment, Timestamps};
use crate::timefmt;

/// Spectrum header length.
pub const HEADER_LEN: usize = 484;
/// as8 reference header without the description: BOOL flag (2) + two f64 OLE dates (16) + u16 length (2).
pub const REFERENCE_HEADER_LEN: usize = 20;
/// as7 reference header without the description: u8 flag (1) + two f64 OLE dates (16) + u16 length (2) + one
/// reserved byte (1). Verified on 31 files of one unit (reflectance bit-equal to the vendor export).
pub const AS7_REFERENCE_HEADER_LEN: usize = 20;
/// The v8 trailer (classifier data, dependent variables, calibration header, audit log, signature) is
/// 212 bytes in all 267 files in hand (all zero). It is not parsed; it must be present, so a truncated
/// file is never accepted. Longer trailers (stored calibration spectra, audit log) are accepted.
pub const MIN_TRAILER_LEN: usize = 212;
/// Supported grid.
pub const CHANNELS: usize = 2151;
pub const FIRST_NM: f32 = 350.0;
pub const STEP_NM: f32 = 1.0;
/// Where each detector join may lie (VNIR/SWIR1, SWIR1/SWIR2). Seen: LabSpec 4 1000/1800, FieldSpec 3 header
/// 1000/1800 (manual: 1830), Trek-type handheld 1000/1785 (blank header).
pub const JOIN_WINDOWS_NM: [(f64, f64); 2] = [(950.0, 1050.0), (1700.0, 1900.0)];
/// Joins used when neither the header nor the white reference gives them.
pub const DEFAULT_JOINS_NM: [f64; 2] = [1000.0, 1800.0];
/// A detected join's relative DN jump must be at least this many times the median jump in its window.
pub const JOIN_JUMP_CLEAR: f64 = 3.0;
/// ... and at least this large (a window of mostly flat steps has a median of 0).
pub const JOIN_JUMP_MIN: f64 = 0.01;
/// Region where the reference must be usable and where the acquisition checks look.
pub const SCORED_RANGE_NM: (f64, f64) = (1000.0, 2450.0);

// Reference-save recognition (Codex Phase 1 review, MEDIUM 2): the header has no verified field that marks a
// white-reference or dark save, so brightness is NEVER used to classify a file (a very dark or flat near-white
// SAMPLE must reach B3/B4, which mark it Unusable). The only recognised save is the structural identity
// "sample DN block == white-reference DN block, bit for bit", which no measured sample can produce. Legacy
// reflectance files have no DN blocks: a saved panel reaches B4 (white panel, Rescan).
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
    fn f32_block(&self, off: usize, n: usize) -> Vec<f64> {
        (0..n).map(|i| f64::from(self.f32(off + 4 * i))).collect()
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

/// The format a file's magic names, if it is one this reader handles.
pub fn file_format(magic: &[u8]) -> Option<FileFormat> {
    match magic {
        b"as8" => Some(FileFormat::As8),
        b"as7" => Some(FileFormat::As7),
        b"ASD" => Some(FileFormat::Legacy),
        _ => None,
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

/// Check the supported-input matrix on the header alone. Texts that the oracle's acceptance also produces are
/// word for word the same (data type, dark correction).
fn check_matrix(h: &AsdHeader, fmt: FileFormat) -> Result<(), ReadError> {
    match fmt {
        FileFormat::As8 | FileFormat::As7 => {
            if h.data_type != 0 {
                return Err(unsupported("not a raw (DN + white reference) file"));
            }
            if h.data_format != 2 {
                return Err(unsupported(format!(
                    "data format {} (only float64 = 2 is supported)",
                    h.data_format
                )));
            }
        }
        FileFormat::Legacy => {
            if h.data_type != 1 {
                return Err(unsupported("not a reflectance file"));
            }
            if h.data_format != 0 && h.data_format != 2 {
                return Err(unsupported(format!(
                    "data format {} (legacy reflectance files must be float32 = 0 or float64 = 2)",
                    h.data_format
                )));
            }
        }
    }
    if fmt == FileFormat::As8 && h.dark_corrected != 1 {
        return Err(unsupported("dark correction flag not set"));
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

// ---------------------------------------------------------------------------------------------- joins
/// Each join finite, on the 1-nm grid (a whole number) and inside its window (two joins). An off-grid join would put
/// the B8 fits between channels.
pub fn joins_plausible(joins: &[f64]) -> bool {
    joins.len() == JOIN_WINDOWS_NM.len()
        && joins
            .iter()
            .zip(JOIN_WINDOWS_NM)
            .all(|(&j, (lo, hi))| j.is_finite() && j.fract() == 0.0 && lo <= j && j <= hi)
}

/// numpy's median of a non-empty slice (the mean of the two middle values for an even count).
fn np_median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// The last channel before the largest clear relative white-reference DN jump in each join window
/// (oracle `asd.detect_joins`). Candidate steps k -> k+1 have both channels inside the window; a jump is
/// |ref[k+1] - ref[k]| / |ref[k]|, non-finite jumps are skipped, the first of equal maxima wins, and the jump
/// must be at least [`JOIN_JUMP_CLEAR`] times the median finite jump in the window. `None` if any window fails.
pub fn detect_joins(wl: &[f64], reference: &[f64]) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for (lo, hi) in JOIN_WINDOWS_NM {
        let ks: Vec<usize> = (0..wl.len().saturating_sub(1))
            .filter(|&k| wl[k] >= lo && wl[k + 1] <= hi)
            .collect();
        if ks.len() < 3 {
            return None;
        }
        let jumps: Vec<f64> = ks
            .iter()
            .map(|&k| (reference[k + 1] - reference[k]).abs() / reference[k].abs())
            .collect();
        let mut finite: Vec<f64> = jumps.iter().copied().filter(|j| j.is_finite()).collect();
        if finite.len() < 3 {
            return None;
        }
        let mut best: Option<(usize, f64)> = None;
        for (i, &j) in jumps.iter().enumerate() {
            if j.is_finite() && best.is_none_or(|(_, b)| j > b) {
                best = Some((i, j));
            }
        }
        let (i, j) = best?;
        // j and the median are finite here, so "<" is the oracle's "not >="
        if j < JOIN_JUMP_CLEAR * np_median(&mut finite) || j < JOIN_JUMP_MIN {
            return None;
        }
        out.push(wl[ks[i]]);
    }
    Some(out)
}

/// Joins and where they came from: "header" (plausible header joins), "reference_dn" (detected from the white
/// reference), or "default" (1000/1800).
pub fn resolve_joins(
    header_joins: &[f64],
    wl: &[f64],
    reference: Option<&[f64]>,
) -> (Vec<f64>, &'static str) {
    if joins_plausible(header_joins) {
        return (header_joins.to_vec(), "header");
    }
    if let Some(r) = reference {
        if let Some(d) = detect_joins(wl, r) {
            return (d, "reference_dn");
        }
    }
    (DEFAULT_JOINS_NM.to_vec(), "default")
}

/// What the oracle's tolerant reader (`asd.read`) sees in a file, for the record's input block: the format, the
/// two OLE dates (if the reference header is there) and the resolved joins. Mirrors the oracle exactly, including
/// its treatment of an unknown magic as `as8`-like. `None` when the file is shorter than the header, has an unknown
/// data format, or ends inside the sample block (the oracle raises then).
pub struct Probe {
    /// "as8" | "as7" | "legacy", or `None` for an unknown magic.
    pub format: Option<&'static str>,
    pub ole_reference: Option<f64>,
    pub ole_spectrum: Option<f64>,
    pub joins: Vec<f64>,
    pub joins_source: &'static str,
}

pub fn probe(b: &[u8]) -> Option<Probe> {
    if b.len() < HEADER_LEN {
        return None;
    }
    let r = Bytes { b };
    let fmt = file_format(&b[0..3]);
    let n = usize::from(r.u16(204));
    let sz = match b[199] {
        0 | 1 => 4,
        2 => 8,
        _ => return None,
    };
    let off = HEADER_LEN + n * sz;
    if b.len() < off {
        return None;
    }
    let first = f64::from(r.f32(191));
    let step = f64::from(r.f32(195));
    let wl: Vec<f64> = (0..n).map(|i| first + step * i as f64).collect();
    let header_joins = [f64::from(r.f32(444)), f64::from(r.f32(448))];
    let (mut ole_ref, mut ole_spec, mut reference) = (None, None, None);
    match fmt {
        Some(FileFormat::Legacy) => {}
        Some(FileFormat::As7) => {
            if b.len() >= off + AS7_REFERENCE_HEADER_LEN {
                ole_ref = Some(r.f64(off + 1));
                ole_spec = Some(r.f64(off + 9));
                let o2 = off + AS7_REFERENCE_HEADER_LEN + usize::from(r.u16(off + 17));
                if b.len() >= o2 + n * 8 {
                    reference = Some(r.f64_block(o2, n));
                }
            }
        }
        _ => {
            if b.len() >= off + REFERENCE_HEADER_LEN {
                ole_ref = Some(r.f64(off + 2));
                ole_spec = Some(r.f64(off + 10));
                let o2 = off + REFERENCE_HEADER_LEN + usize::from(r.u16(off + 18));
                if b.len() >= o2 + n * 8 {
                    reference = Some(r.f64_block(o2, n));
                }
            }
        }
    }
    let (joins, joins_source) = resolve_joins(&header_joins, &wl, reference.as_deref());
    Some(Probe {
        format: fmt.map(FileFormat::as_str),
        ole_reference: ole_ref,
        ole_spectrum: ole_spec,
        joins,
        joins_source,
    })
}

// ---------------------------------------------------------------------------------------------- helpers
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

fn grid(h: &AsdHeader) -> Vec<f64> {
    (0..usize::from(h.channels))
        .map(|i| f64::from(h.first_wavelength_nm) + f64::from(h.wavelength_step_nm) * i as f64)
        .collect()
}

fn acquired_tm(r: &Bytes) -> [i16; 9] {
    let mut tm = [0i16; 9];
    for (k, v) in tm.iter_mut().enumerate() {
        *v = r.i16(160 + 2 * k);
    }
    tm
}

fn timestamps(r: &Bytes, spectrum_ole: f64, reference_ole: f64) -> Timestamps {
    let tm = acquired_tm(r);
    let reference_time_t = r.u32(187);
    let reference_age_s =
        if timefmt::ole_plausible(spectrum_ole) && timefmt::ole_plausible(reference_ole) {
            Some((spectrum_ole - reference_ole) * 86_400.0)
        } else {
            None
        };
    Timestamps {
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
    }
}

fn header_joins(h: &AsdHeader) -> Vec<f64> {
    vec![f64::from(h.splice1_nm), f64::from(h.splice2_nm)]
}

fn flag_warning(h: &AsdHeader) -> Option<String> {
    (h.flags != [0, 0, 0, 0]).then(|| {
        format!(
            "header flag bytes 421-424 are {:02x?} (meaning unverified; see check B2)",
            h.flags
        )
    })
}

// ---------------------------------------------------------------------------------------------- readers
/// Read a scan from bytes: `as8`, `as7` or legacy `ASD` (dispatch on the magic). Never panics.
pub fn read_any(bytes: &[u8]) -> Result<Scan, ReadError> {
    need(bytes, 3, "magic")?;
    let magic = &bytes[0..3];
    match file_format(magic) {
        Some(FileFormat::Legacy) => read_legacy(bytes),
        Some(fmt) => read_raw(bytes, fmt),
        None => {
            let m = String::from_utf8_lossy(magic).to_string();
            Err(match magic {
                b"as6" | b"as5" | b"as4" | b"as3" | b"as2" | b"as1" | b"asd" => {
                    unsupported(format!(
                        "ASD file version {m:?} (only as8, as7 and legacy ASD files are supported)"
                    ))
                }
                _ => ReadError::NotAsd { magic: m },
            })
        }
    }
}

/// Read an `as8` file from bytes (any other format is an error). Never panics.
pub fn read_as8(bytes: &[u8]) -> Result<Scan, ReadError> {
    need(bytes, 3, "magic")?;
    match file_format(&bytes[0..3]) {
        Some(FileFormat::As8) => read_raw(bytes, FileFormat::As8),
        Some(other) => Err(unsupported(format!(
            "ASD file version {:?} (expected as8)",
            other.as_str()
        ))),
        None => read_any(bytes),
    }
}

/// `as8` / `as7`: sample DN with a stored white reference.
fn read_raw(bytes: &[u8], fmt: FileFormat) -> Result<Scan, ReadError> {
    need(bytes, HEADER_LEN, "header")?;
    let r = Bytes { b: bytes };
    let header = parse_header(&r);
    check_matrix(&header, fmt)?;

    let n = usize::from(header.channels);
    let sample_off = HEADER_LEN;
    let refhdr_off = sample_off + 8 * n;
    need(bytes, refhdr_off, "sample block")?;
    let (reference_ole, spectrum_ole, desc_off, desc_len) = match fmt {
        FileFormat::As7 => {
            need(
                bytes,
                refhdr_off + AS7_REFERENCE_HEADER_LEN,
                "reference header",
            )?;
            match r.u8(refhdr_off) {
                1 => {}
                0 => {
                    return Err(unsupported(
                        "reference flag not set (no stored white reference)",
                    ))
                }
                other => {
                    return Err(ReadError::Invalid {
                        reason: format!("as7 reference flag byte {other:#04x} (expected 0x01)"),
                    })
                }
            }
            (
                r.f64(refhdr_off + 1),
                r.f64(refhdr_off + 9),
                refhdr_off + AS7_REFERENCE_HEADER_LEN,
                usize::from(r.u16(refhdr_off + 17)),
            )
        }
        _ => {
            need(bytes, refhdr_off + REFERENCE_HEADER_LEN, "reference header")?;
            match r.u16(refhdr_off) {
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
            (
                r.f64(refhdr_off + 2),
                r.f64(refhdr_off + 10),
                refhdr_off + REFERENCE_HEADER_LEN,
                usize::from(r.u16(refhdr_off + 18)),
            )
        }
    };
    let ref_off = desc_off + desc_len;
    need(bytes, ref_off, "reference description")?;
    let reference_description: String = bytes[desc_off..ref_off]
        .iter()
        .map(|&c| char::from(c))
        .collect();
    let trailer_off = ref_off + 8 * n;
    need(bytes, trailer_off, "reference block")?;
    // The trailer is not parsed. An as8 file that ends exactly after the reference block (no trailer at all, as the
    // oracle's golden writer produces) has every block the reader needs: accepted with a log warning. A PARTIAL as8
    // trailer means the file was cut short: rejected (Phase 3; keeps parity with the oracle reader). The as7 trailer
    // size is not known, so any as7 trailer is accepted.
    let has_trailer = bytes.len() > trailer_off;
    if has_trailer && fmt == FileFormat::As8 {
        need(bytes, trailer_off + MIN_TRAILER_LEN, "as8 trailer")?;
    }

    let wavelengths_nm = grid(&header);
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

    let splices_header_nm = header_joins(&header);
    let (splices_nm, joins_source) =
        resolve_joins(&splices_header_nm, &wavelengths_nm, Some(&reference_dn));
    let segments = segments(&wavelengths_nm, &splices_nm);

    let mut warnings = if joins_source == "header" {
        splice_crosscheck(&wavelengths_nm, &reference_dn, &splices_nm)
    } else {
        vec![format!(
            "header joins {splices_header_nm:?} not usable; joins {splices_nm:?} ({joins_source})"
        )]
    };
    match fmt {
        FileFormat::As8 => {
            if !has_trailer {
                warnings.push(
                    "no as8 trailer (the file ends after the white-reference block)".to_string(),
                );
            }
            if bytes[trailer_off..].iter().any(|&b| b != 0) {
                warnings.push(format!(
                    "as8 trailer holds {} bytes with data (not parsed)",
                    bytes.len() - trailer_off
                ));
            }
        }
        _ => {
            if has_trailer {
                warnings.push(format!(
                    "{} trailer of {} bytes (not parsed)",
                    fmt.as_str(),
                    bytes.len() - trailer_off
                ));
            }
        }
    }
    warnings.extend(flag_warning(&header));

    let timestamps = timestamps(&r, spectrum_ole, reference_ole);
    let kind = classify_kind(&sample_dn, &reference_dn);

    Ok(Scan {
        reader: "asd",
        format: fmt,
        header,
        timestamps,
        reference_description,
        wavelengths_nm,
        splices_header_nm,
        splices_nm,
        joins_source,
        segments,
        reflectance,
        sample_dn: Some(sample_dn),
        reference_dn: Some(reference_dn),
        kind,
        warnings,
    })
}

/// Legacy v1 (`ASD`): the stored reflectance; no white reference, no raw DN, no OLE dates.
fn read_legacy(bytes: &[u8]) -> Result<Scan, ReadError> {
    need(bytes, HEADER_LEN, "header")?;
    let r = Bytes { b: bytes };
    let header = parse_header(&r);
    check_matrix(&header, FileFormat::Legacy)?;
    let n = usize::from(header.channels);
    let wavelengths_nm = grid(&header);
    let reflectance = if header.data_format == 0 {
        need(bytes, HEADER_LEN + 4 * n, "sample block")?;
        r.f32_block(HEADER_LEN, n)
    } else {
        need(bytes, HEADER_LEN + 8 * n, "sample block")?;
        r.f64_block(HEADER_LEN, n)
    };
    let splices_header_nm = header_joins(&header);
    let (splices_nm, joins_source) = resolve_joins(&splices_header_nm, &wavelengths_nm, None);
    let segments = segments(&wavelengths_nm, &splices_nm);
    let mut warnings = vec![
        "legacy reflectance file: no white reference or raw DN stored (saturation not checked)"
            .to_string(),
    ];
    if joins_source != "header" {
        warnings.push(format!(
            "header joins {splices_header_nm:?} not usable; joins {splices_nm:?} ({joins_source})"
        ));
    }
    if header.flags[0] & 0x04 != 0 {
        warnings.push(
            "header flag byte 421 has bit 0x04 set (seen on ViewSpec splice-corrected .sco files)"
                .to_string(),
        );
    }
    let timestamps = timestamps(&r, f64::NAN, f64::NAN);
    Ok(Scan {
        reader: "asd",
        format: FileFormat::Legacy,
        header,
        timestamps,
        reference_description: String::new(),
        wavelengths_nm,
        splices_header_nm,
        splices_nm,
        joins_source,
        segments,
        reflectance,
        sample_dn: None,
        reference_dn: None,
        kind: ScanKind::Sample,
        warnings,
    })
}
