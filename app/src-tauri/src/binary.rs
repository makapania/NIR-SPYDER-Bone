//! Binary display arrays for IPC: little-endian float32, no header.
//!
//! Computation stays float64 in the core; only display arrays are narrowed to float32 at the
//! boundary (PLAN §2.1). Metadata (which array is which, grid, ids) travels as JSON in a separate
//! `invoke`, so the byte buffer is always a plain `n × k` float32 block that JS views without copying.

/// The ASD LabSpec 4 grid: 350–2500 nm at 1 nm.
pub const GRID_N: usize = 2151;

/// Narrows to float32 and writes little-endian bytes (explicitly LE, so the layout does not depend
/// on the host; every supported target is LE anyway).
pub fn f32_le_bytes(values: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for &v in values {
        out.extend_from_slice(&(v as f32).to_le_bytes());
    }
    out
}

/// The IPC self-test signal. The UI evaluates the same formula (`ui/src/lib/f32.ts`,
/// `ipcTestSignal`), so any byte-order, offset or length error shows up as a mismatch.
/// Value i: sin(i / 37) * 10 + i / 1000, with NaN at i = 0 to check the gap convention.
pub fn test_signal(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| {
            if i == 0 {
                f64::NAN
            } else {
                (i as f64 / 37.0).sin() * 10.0 + i as f64 / 1000.0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_little_endian_f32() {
        let b = f32_le_bytes(&[1.0, -2.5]);
        assert_eq!(b.len(), 8);
        assert_eq!(&b[0..4], &1.0f32.to_le_bytes());
        assert_eq!(f32::from_le_bytes([b[4], b[5], b[6], b[7]]), -2.5);
    }

    #[test]
    fn test_signal_has_gap_and_known_values() {
        let s = test_signal(GRID_N);
        assert_eq!(s.len(), GRID_N);
        assert!(s[0].is_nan());
        let expected = (100.0f64 / 37.0).sin() * 10.0 + 0.1;
        assert!((s[100] - expected).abs() < 1e-12);
    }
}
