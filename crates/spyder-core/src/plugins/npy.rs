//! A minimal `.npy` reader for plug-in sidecars: little-endian float64 (`'<f8'`), C order, 1-D or 2-D only.
//! Anything else (object arrays, pickles, Fortran order, other dtypes, 0-D or 3-D shapes, short or long data)
//! is an error. Never panics on malformed input.

use serde_json::Value;

/// A decoded float64 array (row-major).
#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    pub shape: Vec<usize>,
    pub data: Vec<f64>,
}

impl Array {
    /// As JSON: a list (1-D) or a list of lists (2-D).
    pub fn to_json(&self) -> Value {
        let num = |x: f64| serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number);
        match self.shape.as_slice() {
            [_] => Value::Array(self.data.iter().map(|&x| num(x)).collect()),
            [r, c] => Value::Array(
                (0..*r)
                    .map(|i| {
                        Value::Array(
                            self.data[i * c..(i + 1) * c]
                                .iter()
                                .map(|&x| num(x))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
            _ => Value::Null,
        }
    }

    /// Row `i` of a 2-D array.
    pub fn row(&self, i: usize) -> Option<&[f64]> {
        match self.shape.as_slice() {
            [r, c] if i < *r => Some(&self.data[i * c..(i + 1) * c]),
            _ => None,
        }
    }
}

const MAGIC: &[u8] = b"\x93NUMPY";

/// Value of `'key': <value>` in the header dict text, up to the next top-level comma or closing brace.
fn header_field<'a>(h: &'a str, key: &str) -> Option<&'a str> {
    let pat_sq = format!("'{key}'");
    let start = h.find(&pat_sq)? + pat_sq.len();
    let rest = h[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    // value ends at the first ',' or '}' outside parentheses
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' | '}' if depth == 0 => return Some(rest[..i].trim()),
            _ => {}
        }
    }
    None
}

/// Parse `.npy` bytes.
pub fn parse(b: &[u8]) -> Result<Array, String> {
    if b.len() < 10 || &b[..6] != MAGIC {
        return Err("not a .npy file (bad magic)".into());
    }
    let major = b[6];
    let (hlen, hstart) = match major {
        1 => (u16::from_le_bytes([b[8], b[9]]) as usize, 10usize),
        2 | 3 => {
            if b.len() < 12 {
                return Err("truncated .npy header".into());
            }
            (
                u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize,
                12usize,
            )
        }
        v => return Err(format!(".npy format version {v} is not supported")),
    };
    let hend = hstart
        .checked_add(hlen)
        .filter(|&e| e <= b.len())
        .ok_or("truncated .npy header")?;
    let h = std::str::from_utf8(&b[hstart..hend]).map_err(|_| "the .npy header is not text")?;
    let descr = header_field(h, "descr").ok_or("the .npy header has no 'descr'")?;
    if descr != "'<f8'" {
        return Err(format!(
            "dtype {descr} is not little-endian float64 ('<f8'); object arrays and pickles are never loaded"
        ));
    }
    let fo = header_field(h, "fortran_order").ok_or("the .npy header has no 'fortran_order'")?;
    if fo != "False" {
        return Err("Fortran-order arrays are not accepted (C order only)".into());
    }
    let shape_s = header_field(h, "shape").ok_or("the .npy header has no 'shape'")?;
    let inner = shape_s
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .ok_or("malformed shape")?;
    let shape: Vec<usize> = inner
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| format!("malformed shape {shape_s}"))
        })
        .collect::<Result<_, _>>()?;
    if shape.is_empty() || shape.len() > 2 {
        return Err(format!(
            "shape {shape_s}: only 1-D or 2-D arrays are accepted"
        ));
    }
    let count = shape
        .iter()
        .try_fold(1usize, |a, &d| a.checked_mul(d))
        .ok_or("shape too large")?;
    let nbytes = count.checked_mul(8).ok_or("shape too large")?;
    let data = &b[hend..];
    if data.len() != nbytes {
        return Err(format!(
            "data holds {} bytes; shape {shape_s} needs {nbytes}",
            data.len()
        ));
    }
    let values = data
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect();
    Ok(Array {
        shape,
        data: values,
    })
}

/// Encode a 1-D or 2-D float64 array as `.npy` version 1.0 bytes (used by tests and tools).
pub fn encode(shape: &[usize], data: &[f64]) -> Vec<u8> {
    let shape_s = match shape {
        [n] => format!("({n},)"),
        [r, c] => format!("({r}, {c})"),
        _ => "()".to_string(),
    };
    let mut h = format!("{{'descr': '<f8', 'fortran_order': False, 'shape': {shape_s}, }}");
    // pad so that the data starts on a 64-byte boundary, header ends with '\n'
    while (10 + h.len() + 1) % 64 != 0 {
        h.push(' ');
    }
    h.push('\n');
    let mut out = Vec::with_capacity(10 + h.len() + data.len() * 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&[1, 0]);
    out.extend_from_slice(&(h.len() as u16).to_le_bytes());
    out.extend_from_slice(h.as_bytes());
    for v in data {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_rejections() {
        let a = parse(&encode(&[2, 3], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.5])).unwrap();
        assert_eq!(a.shape, vec![2, 3]);
        assert_eq!(a.row(1).unwrap(), &[4.0, 5.0, 6.5]);
        let a = parse(&encode(&[3], &[1.0, -2.0, 3.0])).unwrap();
        assert_eq!(a.shape, vec![3]);
        // big-endian, object, Fortran order, truncated, extra bytes
        let good = encode(&[3], &[1.0, 2.0, 3.0]);
        let s = String::from_utf8_lossy(&good).to_string();
        for (from, to) in [("'<f8'", "'>f8'"), ("'<f8'", "'|O' "), ("False", "True ")] {
            let bad = s.replacen(from, to, 1).into_bytes();
            assert!(parse(&bad).is_err(), "{from} -> {to}");
        }
        assert!(parse(&good[..good.len() - 1]).is_err());
        let mut long = good.clone();
        long.push(0);
        assert!(parse(&long).is_err());
        assert!(parse(b"\x93NUMPY").is_err());
        assert!(parse(b"PK\x03\x04 pickle").is_err());
    }
}
