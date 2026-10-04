//! Structural completeness of an ASD file held in memory (03_architecture section 4.3, step 4).
//!
//! This is NOT the reader (that is `spyder-core::read::asd`, which validates everything and decides
//! "unsupported"). It answers one question for the watcher: is the whole structure present yet, or should we
//! wait and read again? A file truncated anywhere fails the block-length check, and a preallocated file (full
//! length, zero-filled tail) fails the zero-fill check, so a half-written file is never delivered as a scan.
//!
//! Layout (little-endian, as in `spyder-core`): 484-byte header; sample block (`channels` values); reference
//! header (2-byte flag, two f64 OLE times, u16 description length, description); white-reference block
//! (`channels` values); `as8` trailer (212 bytes, all zero in every file in hand).
//!
//! Settled bytes that are not an ASD file this app knows are [`Shape::Unrecognised`] and are still delivered,
//! so the core can show a plain "unsupported" message instead of the file silently never appearing.

/// Header length.
pub const HEADER_LEN: usize = 484;
/// Reference header without the description.
pub const REFERENCE_HEADER_LEN: usize = 20;
/// Minimum `as8` trailer (matches `spyder-core::read::asd::MIN_TRAILER_LEN`).
pub const AS8_MIN_TRAILER_LEN: usize = 212;
/// Number of white-reference channels (ending at 2450 nm) that must not all be zero bytes.
const ZERO_FILL_WINDOW: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The whole structure is present.
    Complete { version: String, kind: KindHint },
    /// Not complete yet: wait and read again.
    Incomplete(&'static str),
    /// Settled bytes that are not an ASD structure this check knows; delivered so the core can say why.
    Unrecognised(String),
}

/// What the structure alone says about the save. The core's `classify_kind` uses the same identity rule:
/// a sample block equal to the white-reference block, bit for bit, is a white-reference save (no measured
/// sample can produce it). Dark saves cannot be recognised from verified header fields, so they never are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindHint {
    Sample,
    WhiteReferenceSave,
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn f32_at(b: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Classify the bytes of a candidate file.
pub fn check(b: &[u8]) -> Shape {
    if b.len() < 3 {
        return Shape::Incomplete("shorter than the file signature");
    }
    let magic = &b[0..3];
    if magic == [0, 0, 0] {
        return Shape::Incomplete("file start is still zero-filled (preallocated file)");
    }
    let version = match magic {
        b"as6" | b"as7" | b"as8" => String::from_utf8_lossy(magic).into_owned(),
        _ => {
            return Shape::Unrecognised(format!(
                "not an ASD file (signature {:?})",
                String::from_utf8_lossy(magic)
            ))
        }
    };
    if b.len() < HEADER_LEN {
        return Shape::Incomplete("header incomplete");
    }
    let channels = u16_at(b, 204) as usize;
    if channels == 0 {
        // A preallocated file whose header is only partly written reads as 0 channels: wait. (A genuinely broken
        // header ends up reported as incomplete after the give-up time, never silently dropped.)
        return Shape::Incomplete("header not fully written (0 channels)");
    }
    let value_size = match b[199] {
        2 => 8,
        0 | 1 => 4,
        other => return Shape::Unrecognised(format!("unknown ASD data format {other}")),
    };
    let block = channels * value_size;
    let sample_end = HEADER_LEN + block;
    if b.len() < sample_end {
        return Shape::Incomplete("spectrum block incomplete");
    }
    if version == "as6" {
        // No reference block to check; the core rejects as6 as unsupported anyway.
        return Shape::Complete {
            version,
            kind: KindHint::Sample,
        };
    }
    if b.len() < sample_end + REFERENCE_HEADER_LEN {
        return Shape::Incomplete("reference header incomplete");
    }
    let desc_len = u16_at(b, sample_end + 18) as usize;
    let ref_start = sample_end + REFERENCE_HEADER_LEN + desc_len;
    let ref_end = ref_start + block;
    if b.len() < ref_end {
        return Shape::Incomplete("white-reference block incomplete");
    }
    // The core accepts an as8 file that ends exactly after the reference block (no trailer); a PARTIAL trailer
    // means the file is still being written (or was cut short).
    if version == "as8" && b.len() > ref_end && b.len() < ref_end + AS8_MIN_TRAILER_LEN {
        return Shape::Incomplete("file trailer incomplete");
    }
    // Preallocation: a writer that sets the final length first and fills it later leaves zeros. The white
    // reference is > 0 in 1000-2450 nm in every supported file, so all-zero bytes there mean "not written yet".
    let (lo, hi) = zero_fill_window(b, channels);
    let window = &b[ref_start + lo * value_size..ref_start + hi * value_size];
    let last = &b[ref_end - ZERO_FILL_WINDOW.min(channels) * value_size..ref_end];
    if window.iter().all(|&x| x == 0) || last.iter().all(|&x| x == 0) {
        return Shape::Incomplete("white-reference block is still zero-filled (preallocated file)");
    }
    let kind = if b[HEADER_LEN..sample_end] == b[ref_start..ref_end] {
        KindHint::WhiteReferenceSave
    } else {
        KindHint::Sample
    };
    Shape::Complete { version, kind }
}

/// Channel range [lo, hi) of the zero-fill check: the last channels up to 2450 nm when the header grid is
/// usable, else the last channels of the block.
fn zero_fill_window(b: &[u8], channels: usize) -> (usize, usize) {
    let first = f32_at(b, 191) as f64;
    let step = f32_at(b, 195) as f64;
    let mut hi = channels;
    if first.is_finite() && step.is_finite() && step > 0.0 {
        let idx = ((2450.0 - first) / step).floor();
        if idx >= 0.0 && (idx as usize) < channels {
            hi = idx as usize + 1;
        }
    }
    (hi.saturating_sub(ZERO_FILL_WINDOW), hi)
}

/// Builders of synthetic ASD-shaped bytes for tests (here and in the integration tests). The numbers are
/// meaningless; only the structure matches an `as8` RAW file (2151 channels from 350 nm, f64, 35 132 bytes
/// with an empty description, like every file in hand).
pub mod synthetic {
    use super::*;

    pub const CHANNELS: usize = 2151;

    /// A complete synthetic `as8` file. `seed` varies the sample block so different seeds give different bytes.
    pub fn as8(seed: u32) -> Vec<u8> {
        as8_with(seed, false)
    }

    /// A synthetic white-reference save (sample block identical to the reference block).
    pub fn as8_reference_save(seed: u32) -> Vec<u8> {
        as8_with(seed, true)
    }

    fn as8_with(seed: u32, reference_save: bool) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_LEN];
        h[0..3].copy_from_slice(b"as8");
        h[178] = 6; // program version
        h[179] = 8; // file version
        h[181] = 1; // dark corrected
        h[186] = 0; // RAW
        h[191..195].copy_from_slice(&350.0f32.to_le_bytes());
        h[195..199].copy_from_slice(&1.0f32.to_le_bytes());
        h[199] = 2; // f64
        h[204..206].copy_from_slice(&(CHANNELS as u16).to_le_bytes());
        h[400..402].copy_from_slice(&(28313u16).to_le_bytes());
        h[444..448].copy_from_slice(&1000.0f32.to_le_bytes());
        h[448..452].copy_from_slice(&1800.0f32.to_le_bytes());
        let reference: Vec<f64> = (0..CHANNELS).map(|i| 20_000.0 + (i as f64) * 3.0).collect();
        let sample: Vec<f64> = if reference_save {
            reference.clone()
        } else {
            (0..CHANNELS)
                .map(|i| {
                    reference[i]
                        * (0.3 + 0.2 * (((i as f64) + seed as f64 * 7.3) / 97.0).sin().abs())
                })
                .collect()
        };
        let mut out = h;
        for v in &sample {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&1u16.to_le_bytes()); // reference flag
        out.extend_from_slice(&45_000.5f64.to_le_bytes());
        out.extend_from_slice(&(45_000.5f64 + seed as f64 / 86_400.0).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // description length
        for v in &reference {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend(std::iter::repeat_n(0u8, AS8_MIN_TRAILER_LEN));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::synthetic::*;
    use super::*;

    #[test]
    fn synthetic_file_is_complete_and_the_real_size() {
        let b = as8(1);
        assert_eq!(b.len(), 35_132);
        assert_eq!(
            check(&b),
            Shape::Complete {
                version: "as8".into(),
                kind: KindHint::Sample
            }
        );
    }

    #[test]
    fn every_truncation_is_incomplete() {
        let b = as8(2);
        let ref_end = b.len() - AS8_MIN_TRAILER_LEN;
        for cut in (0..b.len())
            .step_by(97)
            .chain([b.len() - 1, 483, 484, 3, 2])
        {
            if cut == ref_end {
                continue; // a trailerless file is complete (tested below)
            }
            match check(&b[..cut]) {
                Shape::Incomplete(_) => {}
                other => panic!("cut {cut}: {other:?}"),
            }
        }
    }

    #[test]
    fn preallocated_zero_tail_and_zero_start_are_incomplete() {
        let b = as8(3);
        let mut z = b.clone();
        let half = z.len() / 2;
        z[half..].iter_mut().for_each(|x| *x = 0);
        assert!(matches!(check(&z), Shape::Incomplete(_)));
        let zeros = vec![0u8; b.len()];
        assert!(matches!(check(&zeros), Shape::Incomplete(_)));
    }

    #[test]
    fn partly_written_preallocated_files_are_incomplete() {
        let b = as8(5);
        // only the first 100 header bytes written
        let mut z = vec![0u8; b.len()];
        z[..100].copy_from_slice(&b[..100]);
        assert!(matches!(check(&z), Shape::Incomplete(_)), "{:?}", check(&z));
        // everything but the last 81 reference values
        let ref_end = b.len() - AS8_MIN_TRAILER_LEN;
        let mut z = b.clone();
        z[ref_end - 81 * 8..ref_end].iter_mut().for_each(|x| *x = 0);
        assert!(matches!(check(&z), Shape::Incomplete(_)), "{:?}", check(&z));
    }

    #[test]
    fn trailerless_file_is_complete_but_a_partial_trailer_is_not() {
        let b = as8(6);
        let ref_end = b.len() - AS8_MIN_TRAILER_LEN;
        assert!(matches!(check(&b[..ref_end]), Shape::Complete { .. }));
        assert!(matches!(check(&b[..ref_end + 5]), Shape::Incomplete(_)));
    }

    #[test]
    fn reference_save_is_recognised() {
        assert!(matches!(
            check(&as8_reference_save(4)),
            Shape::Complete {
                kind: KindHint::WhiteReferenceSave,
                ..
            }
        ));
    }

    #[test]
    fn other_bytes_are_unrecognised_not_incomplete() {
        assert!(matches!(
            check(b"hello world, not an asd"),
            Shape::Unrecognised(_)
        ));
    }
}
