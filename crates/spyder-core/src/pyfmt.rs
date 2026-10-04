//! Python-compatible text for exports that must equal the frozen oracle's word for word (PLAN Phase 3 gate:
//! labels exactly): `repr(float)`, `str()` of a JSON number, and `json.dumps` (default separators, ASCII).

/// Python `repr(x)` of a float: the shortest round-trip digits; exponent form when the decimal exponent is
/// < -4 or >= 16 (`1e-05`, `1e+16`), else positional with at least one decimal (`450.0`, `0.001`).
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // Rust's LowerExp gives the shortest round-trip digits: "d.ddde-5"
    let e = format!("{x:e}");
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let sign = if neg { "-" } else { "" };
    if !(-4..16).contains(&exp) {
        let m = if digits.len() == 1 {
            digits.clone()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        let es = if exp < 0 { "-" } else { "+" };
        return format!("{sign}{m}e{es}{:02}", exp.abs());
    }
    let n = digits.len() as i32;
    if exp >= 0 {
        let int_len = exp + 1;
        if n <= int_len {
            format!("{sign}{}{}.0", digits, "0".repeat((int_len - n) as usize))
        } else {
            let (a, b) = digits.split_at(int_len as usize);
            format!("{sign}{a}.{b}")
        }
    } else {
        format!("{sign}0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    }
}

/// Python `str()` of a JSON number as `json.loads` would produce it (int stays int, float uses `repr`).
pub fn json_number_str(v: &serde_json::Value) -> String {
    if let Some(i) = v.as_i64() {
        return i.to_string();
    }
    if let Some(u) = v.as_u64() {
        return u.to_string();
    }
    v.as_f64().map(float_repr).unwrap_or_default()
}

/// Python `json.dumps(s)` of a string (ensure_ascii).
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{08}' => o.push_str("\\b"),
            '\u{0c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    o.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_matches_python() {
        let cases: &[(f64, &str)] = &[
            (450.0, "450.0"),
            (0.1, "0.1"),
            (1e-5, "1e-05"),
            (1.5e-5, "1.5e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (1.2345e16, "1.2345e+16"),
            (1e15, "1000000000000000.0"),
            (-0.0006789449929507491, "-0.0006789449929507491"),
            (1.1999999999997168, "1.1999999999997168"),
            (166529.16866978622, "166529.16866978622"),
            (-3.471468164711178, "-3.471468164711178"),
            (2.0, "2.0"),
            (-0.0, "-0.0"),
            (123456789012345.6, "123456789012345.6"),
        ];
        for (x, want) in cases {
            assert_eq!(float_repr(*x), *want, "{x:e}");
        }
        assert_eq!(json_str("a\"b\u{b1}"), "\"a\\\"b\\u00b1\"");
    }
}
