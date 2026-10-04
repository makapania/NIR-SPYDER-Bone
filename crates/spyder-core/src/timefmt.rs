//! Small, dependency-free date helpers for the timestamps stored in ASD files.

/// OLE automation date of 1970-01-01 (days since 1899-12-30).
pub const OLE_UNIX_EPOCH_DAYS: f64 = 25569.0;

/// Plausible OLE range for a scan: 1900-01-01 .. 2199-12-31.
const OLE_MIN: f64 = 2.0;
const OLE_MAX: f64 = 109_574.0;

/// Gregorian civil date from days since 1970-01-01 (H. Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// "YYYY-MM-DDTHH:MM:SS" from seconds since 1970-01-01 on some clock (no zone attached).
pub fn format_unix_seconds(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Whether an OLE date is a plausible scan time.
pub fn ole_plausible(ole: f64) -> bool {
    ole.is_finite() && (OLE_MIN..=OLE_MAX).contains(&ole)
}

/// Seconds since 1970-01-01 on the OLE value's own clock, rounded to the nearest second.
pub fn ole_to_unix_seconds(ole: f64) -> Option<i64> {
    if !ole_plausible(ole) {
        return None;
    }
    Some(((ole - OLE_UNIX_EPOCH_DAYS) * 86_400.0).round() as i64)
}

/// OLE automation date as "YYYY-MM-DDTHH:MM:SS" (nearest second), if plausible.
pub fn ole_to_string(ole: f64) -> Option<String> {
    ole_to_unix_seconds(ole).map(format_unix_seconds)
}

/// Proleptic Gregorian leap year.
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` (1-12) of `year`; 0 for an invalid month.
pub fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// C `struct tm` (sec, min, hour, mday, mon 0-11, year since 1900, ...) as "YYYY-MM-DDTHH:MM:SS".
pub fn tm_to_string(tm: &[i16; 9]) -> Option<String> {
    let (sec, min, hour, mday, mon, year) = (tm[0], tm[1], tm[2], tm[3], tm[4], tm[5]);
    let year = i32::from(year) + 1900;
    let ok = (0..=60).contains(&sec)
        && (0..=59).contains(&min)
        && (0..=23).contains(&hour)
        && (1..=31).contains(&mday)
        && (0..=11).contains(&mon)
        && (1900..=2199).contains(&year);
    // Codex Phase 1 LOW 6: no impossible calendar dates (Feb 31, Feb 29 in a common year, Apr 31, ...)
    if !ok || i32::from(mday) > days_in_month(year, i32::from(mon) + 1) {
        return None;
    }
    Some(format!(
        "{year:04}-{:02}-{mday:02}T{hour:02}:{min:02}:{sec:02}",
        mon + 1
    ))
}

/// Local-minus-UTC offset in minutes from the same instant on the local OLE clock and as a UTC time_t.
/// Rounded to a quarter hour; None if the residual exceeds 120 s, the offset exceeds 14 h, or time_t is 0.
pub fn utc_offset_minutes(ole_local: f64, time_t_utc: u32) -> Option<i32> {
    if time_t_utc == 0 || !ole_plausible(ole_local) {
        return None;
    }
    let local = (ole_local - OLE_UNIX_EPOCH_DAYS) * 86_400.0;
    let diff = local - f64::from(time_t_utc);
    let quarters = (diff / 900.0).round();
    if (diff - quarters * 900.0).abs() > 120.0 || quarters.abs() > 56.0 {
        return None;
    }
    Some(quarters as i32 * 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tm_rejects_impossible_calendar_dates() {
        // sec, min, hour, mday, mon (0-11), year-1900, wday, yday, isdst
        let tm = |mday: i16, mon: i16, year: i16| [0i16, 0, 12, mday, mon, year, 0, 0, 0];
        assert_eq!(tm_to_string(&tm(31, 1, 124)), None); // 2024-02-31
        assert_eq!(tm_to_string(&tm(30, 1, 124)), None); // 2024-02-30
        assert_eq!(
            tm_to_string(&tm(29, 1, 124)).as_deref(),
            Some("2024-02-29T12:00:00")
        );
        assert_eq!(tm_to_string(&tm(29, 1, 123)), None); // 2023 is not a leap year
        assert_eq!(tm_to_string(&tm(29, 1, 200)), None); // 2100 is not a leap year
        assert!(tm_to_string(&tm(29, 1, 100)).is_some()); // 2000 is
        assert_eq!(tm_to_string(&tm(31, 3, 124)), None); // April 31
        assert!(tm_to_string(&tm(31, 11, 124)).is_some()); // December 31
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn ole_dates() {
        // 03 section 3.2: 46273.5707 is 2026-09-08 13:41:48 (rounded to the second here)
        assert_eq!(
            ole_to_string(46273.0).as_deref(),
            Some("2026-09-08T00:00:00")
        );
        assert_eq!(
            ole_to_string(25569.5).as_deref(),
            Some("1970-01-01T12:00:00")
        );
        assert_eq!(ole_to_string(f64::NAN), None);
        assert_eq!(ole_to_string(0.0), None);
        assert_eq!(ole_to_string(1e300), None);
    }

    #[test]
    fn offsets() {
        // local 12:00 on 1970-01-02, UTC 16:00 -> -240 minutes
        let ole = OLE_UNIX_EPOCH_DAYS + 1.5;
        assert_eq!(utc_offset_minutes(ole, 86_400 + 16 * 3600), Some(-240));
        assert_eq!(utc_offset_minutes(ole, 86_400 + 11 * 3600), Some(60));
        assert_eq!(utc_offset_minutes(ole, 86_400 + 11 * 3600 + 400), None);
        assert_eq!(utc_offset_minutes(ole, 0), None);
    }

    #[test]
    fn tm() {
        assert_eq!(
            tm_to_string(&[14, 53, 13, 8, 8, 126, 2, 250, 1]).as_deref(),
            Some("2026-09-08T13:53:14")
        );
        assert_eq!(tm_to_string(&[0, 0, 0, 0, 0, 126, 0, 0, 0]), None);
    }
}
