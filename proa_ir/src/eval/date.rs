// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Date formatting for `Expr::FormatDate`.
//!
//! Dates in the IR are epoch milliseconds (`Expr::Now` is `Date.now()`).
//! Server formatting is UTC so renders are deterministic; supported tokens
//! are documented in `SEMANTICS.md`.

/// ECMAScript maximum time value: +/- 8.64e15 ms from the epoch.
const MAX_TIME_MS: f64 = 8.64e15;

const MS_PER_DAY: i64 = 86_400_000;

/// Format epoch milliseconds with a token pattern, in UTC.
///
/// Tokens: `YYYY` `YY` `MM` `M` `DD` `D` `HH` `H` `mm` `m` `ss` `s` `SSS`.
/// Non-finite or out-of-range input yields `"Invalid Date"` like JS.
pub fn format_date(ms: f64, format: &str) -> String {
    if !ms.is_finite() || ms.abs() > MAX_TIME_MS {
        return "Invalid Date".to_string();
    }
    let t = ms.trunc() as i64;
    let days = t.div_euclid(MS_PER_DAY);
    let ms_of_day = t.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = ms_of_day / 3_600_000;
    let minute = ms_of_day / 60_000 % 60;
    let second = ms_of_day / 1000 % 60;
    let millis = ms_of_day % 1000;

    let mut out = String::with_capacity(format.len() + 4);
    let mut rest = format;
    while !rest.is_empty() {
        let (text, consumed) = match () {
            _ if rest.starts_with("YYYY") => (format!("{year:04}"), 4),
            _ if rest.starts_with("SSS") => (format!("{millis:03}"), 3),
            _ if rest.starts_with("YY") => (format!("{:02}", year.rem_euclid(100)), 2),
            _ if rest.starts_with("MM") => (format!("{month:02}"), 2),
            _ if rest.starts_with("DD") => (format!("{day:02}"), 2),
            _ if rest.starts_with("HH") => (format!("{hour:02}"), 2),
            _ if rest.starts_with("mm") => (format!("{minute:02}"), 2),
            _ if rest.starts_with("ss") => (format!("{second:02}"), 2),
            _ if rest.starts_with('M') => (month.to_string(), 1),
            _ if rest.starts_with('D') => (day.to_string(), 1),
            _ if rest.starts_with('H') => (hour.to_string(), 1),
            _ if rest.starts_with('m') => (minute.to_string(), 1),
            _ if rest.starts_with('s') => (second.to_string(), 1),
            _ => {
                let c = rest.chars().next().expect("rest is non-empty");
                (c.to_string(), c.len_utf8())
            }
        };
        out.push_str(&text);
        rest = &rest[consumed..];
    }
    out
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970() {
        assert_eq!(
            format_date(0.0, "YYYY-MM-DD HH:mm:ss.SSS"),
            "1970-01-01 00:00:00.000"
        );
    }

    #[test]
    fn known_timestamp() {
        // 2026-07-03T12:34:56.789Z
        assert_eq!(
            format_date(1_783_082_096_789.0, "YYYY-MM-DD HH:mm:ss.SSS"),
            "2026-07-03 12:34:56.789"
        );
    }

    #[test]
    fn negative_timestamps_work() {
        // One millisecond before the epoch.
        assert_eq!(
            format_date(-1.0, "YYYY-MM-DD HH:mm:ss.SSS"),
            "1969-12-31 23:59:59.999"
        );
    }

    #[test]
    fn unpadded_tokens() {
        assert_eq!(format_date(0.0, "M/D/YY"), "1/1/70");
    }

    #[test]
    fn out_of_range_is_invalid_date() {
        assert_eq!(format_date(f64::NAN, "YYYY"), "Invalid Date");
        assert_eq!(format_date(8.7e15, "YYYY"), "Invalid Date");
    }
}
