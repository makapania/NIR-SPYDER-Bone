//! What a reader knows about one scan: facts only (PLAN section 3 Step 1; 03 section 3.5).
//!
//! Readers never decide the instrument class and never correct anything. They report the header,
//! the grid, the detector joins, the raw DN blocks and the reflectance R = sample DN / white-reference DN.

use serde::Serialize;

/// Detector segment on the grid. Indices are a half-open range `[start, end)`.
/// Convention (03 section 3.4, the research `splice_correct`): the join wavelength belongs to the LOWER segment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Segment {
    /// "VNIR", "SWIR1", "SWIR2".
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub first_nm: f64,
    pub last_nm: f64,
}

/// What the file is, as opposed to whether it can be scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanKind {
    /// An ordinary target scan: scored.
    Sample,
    /// The white reference (or the panel) saved as a scan: "reference scan (not scored)".
    WhiteReferenceSave,
    /// A dark-current save: "reference scan (not scored)".
    DarkSave,
}

impl ScanKind {
    pub fn is_scored(self) -> bool {
        matches!(self, ScanKind::Sample)
    }

    /// The plain label shown for the scan.
    pub fn label(self) -> &'static str {
        match self {
            ScanKind::Sample => "sample scan",
            ScanKind::WhiteReferenceSave | ScanKind::DarkSave => "reference scan (not scored)",
        }
    }
}

/// Every decoded header field the app may need (03 section 3.2 offsets).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AsdHeader {
    /// Bytes 0..3, e.g. "as8".
    pub version: String,
    /// Byte 178 (BCD-like: 0x65 = 6.5).
    pub program_version: u8,
    /// Byte 179 (0x80 = v8 in every file in hand).
    pub file_version: u8,
    /// Byte 181: 1 = both blocks already dark-corrected.
    pub dark_corrected: u8,
    /// Byte 186: 0 RAW, 1 REF, 2 RAD, ...
    pub data_type: u8,
    /// Byte 199: 0 f32, 1 i32, 2 f64.
    pub data_format: u8,
    /// Byte 191 (f32).
    pub first_wavelength_nm: f32,
    /// Byte 195 (f32).
    pub wavelength_step_nm: f32,
    /// Byte 204 (u16).
    pub channels: u16,
    /// Byte 390 (u32): VNIR integration time.
    pub integration_time_ms: u32,
    /// Byte 394 (i16).
    pub fore_optic: i16,
    /// Byte 398 (u16).
    pub calibration_series: u16,
    /// Byte 400 (u16): the instrument serial number, the only definitive instrument key.
    pub serial: u16,
    /// Byte 418 (u16): A/D bits.
    pub ad_bits: u16,
    /// Bytes 421..425: flag bytes (all zero in every file in hand; ASD documents saturation bits here).
    pub flags: [u8; 4],
    /// Bytes 425/427/429 (u16): dark, white-reference and sample averaging counts.
    pub dark_averages: u16,
    pub reference_averages: u16,
    pub sample_averages: u16,
    /// Byte 431 (u8): 4 = FSFR (full range).
    pub instrument_type: u8,
    /// Bytes 436/438 (u16).
    pub swir1_gain: u16,
    pub swir2_gain: u16,
    /// Bytes 440/442 (u16).
    pub swir1_offset: u16,
    pub swir2_offset: u16,
    /// Bytes 444/448 (f32): detector joins (splices), VNIR/SWIR1 and SWIR1/SWIR2.
    pub splice1_nm: f32,
    pub splice2_nm: f32,
}

/// Times stored in the file. Local times are naive (no zone); the UTC offset is derived.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Timestamps {
    /// Raw OLE automation date of the sample (days since 1899-12-30, local clock).
    pub spectrum_ole: f64,
    /// `spectrum_ole` as local time "YYYY-MM-DDTHH:MM:SS" (nearest second), if plausible.
    pub spectrum_local: Option<String>,
    /// Raw OLE automation date of the white reference (local clock).
    pub reference_ole: f64,
    pub reference_local: Option<String>,
    /// Byte 160 `struct tm` (local) acquisition time.
    pub acquired_tm_local: Option<String>,
    /// `struct tm` daylight-saving flag (tm_isdst).
    pub tm_isdst: i16,
    /// Byte 187 (u32 time_t, UTC): white-reference time.
    pub reference_time_t: u32,
    /// Byte 182 (u32 time_t, UTC): dark-current time.
    pub dark_time_t: u32,
    /// Local minus UTC in minutes = reference OLE (local) - reference time_t (UTC), rounded to a quarter hour.
    /// None when the two disagree with any whole quarter-hour offset by more than 120 s, or are absent.
    pub utc_offset_minutes: Option<i32>,
    /// Sample time minus white-reference time, seconds (both from the OLE dates, same clock).
    pub reference_age_s: Option<f64>,
}

/// One scan as read from a file. Reflectance is on `wavelengths_nm`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scan {
    /// Reader id, "asd".
    pub reader: &'static str,
    pub header: AsdHeader,
    pub timestamps: Timestamps,
    /// Reference-block description string (empty in every file in hand).
    pub reference_description: String,
    /// Explicit grid: first + step * i, computed in f64 (350 + i nm for every supported file).
    pub wavelengths_nm: Vec<f64>,
    /// Per-file detector joins from the header (f32 widened to f64).
    pub splices_nm: Vec<f64>,
    pub segments: Vec<Segment>,
    /// R = sample DN / white-reference DN (IEEE division; bitwise equal to numpy).
    pub reflectance: Vec<f64>,
    /// Dark-corrected sample DN.
    pub sample_dn: Vec<f64>,
    /// Dark-corrected white-reference DN.
    pub reference_dn: Vec<f64>,
    pub kind: ScanKind,
    /// Facts worth logging (e.g. a header join not matching the reference DN jump).
    pub warnings: Vec<String>,
}

impl Scan {
    /// Index of the channel at `nm` (|diff| <= 1e-6), if on the grid.
    pub fn index_of(&self, nm: f64) -> Option<usize> {
        crate::grid::index_of(&self.wavelengths_nm, nm)
    }
}
