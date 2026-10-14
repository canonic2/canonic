//! Pure UTC calendar parsing and formatting for signed catalog metadata.

use std::time::{SystemTime, UNIX_EPOCH};

/// Calendar parts: year, month, day, hour, minute, second and nanoseconds.
pub type Parts = (u32, u32, u32, u32, u32, u32, u32);

/// Parses `YYYY-MM-DDTHH:MM:SSZ` with an optional 1–9 digit fraction.
///
/// Months, leap days and clock fields are validated. Parts compare in
/// chronological order.
#[must_use]
pub fn parse(value: &str) -> Option<Parts> {
    let b = value.as_bytes();
    if b.len() < 20
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
        || b.last() != Some(&b'Z')
    {
        return None;
    }
    let number = |start, end| {
        value
            .get(start..end)
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u32>().ok())
    };
    let (year, month, day, hour, minute, second) = (
        number(0, 4)?,
        number(5, 7)?,
        number(8, 10)?,
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
    );
    let fraction = if b.len() == 20 {
        0
    } else {
        if b[19] != b'.' || !(22..=30).contains(&b.len()) {
            return None;
        }
        let count = b.len() - 21;
        number(20, b.len() - 1)? * 10_u32.pow(u32::try_from(9 - count).ok()?)
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    (day > 0 && day <= days && hour < 24 && minute < 60 && second < 60)
        .then_some((year, month, day, hour, minute, second, fraction))
}

/// Seconds since the Unix epoch for validated calendar parts, ignoring the
/// fraction. Times before the epoch are negative.
#[must_use]
pub fn unix_seconds(parts: Parts) -> i64 {
    let (year, month, day, hour, minute, second, _) = parts;
    // Days from the civil calendar, counting years from March so leap days end a year.
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second)
}

/// Whether the whole-second UTC `expiry` is at or before `now`.
///
/// Comparison uses epoch seconds, so it holds for any `now`, including times
/// before the epoch or beyond year 9999. A malformed expiry returns `None`.
#[must_use]
pub fn is_expired(expiry: &str, now: SystemTime) -> Option<bool> {
    if !is_utc_seconds(expiry) {
        return None;
    }
    let expiry = i128::from(unix_seconds(parse(expiry)?));
    let now = match now.duration_since(UNIX_EPOCH) {
        Ok(after) => i128::from(after.as_secs()),
        // Floor towards the past, matching whole-second precision after the epoch.
        Err(before) => {
            let before = before.duration();
            -i128::from(before.as_secs()) - i128::from(before.subsec_nanos() > 0)
        }
    };
    Some(expiry <= now)
}

/// Whether `value` is a whole-second UTC timestamp, the catalog expiry format.
#[must_use]
pub fn is_utc_seconds(value: &str) -> bool {
    value.len() == 20 && parse(value).is_some()
}

/// Formats seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// ```
/// assert_eq!(canonic_registry::timestamp::format_utc(0), "1970-01-01T00:00:00Z");
/// ```
#[must_use]
pub fn format_utc(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds % 86400 / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn utc_calendar_preserves_leap_boundaries_and_fraction_order() {
        assert!(parse("2000-02-29T00:00:00Z").is_some());
        assert!(parse("0000-02-29T00:00:00Z").is_some());
        assert!(parse("1900-02-29T00:00:00Z").is_none());
        assert!(parse("2024-04-31T00:00:00Z").is_none());
        assert!(parse("2024-01-01T24:00:00Z").is_none());
        assert!(parse("2024-01-01T00:00:60Z").is_none());
        assert!(parse("2024-01-01T00:00:00.0000000001Z").is_none());
        assert_eq!(
            parse("2024-01-01T00:00:00.1Z"),
            parse("2024-01-01T00:00:00.100000000Z")
        );
        assert!(parse("2024-01-01T00:00:00.1Z") < parse("2024-01-01T00:00:00.2Z"));
    }

    #[test]
    fn catalog_expiry_requires_whole_seconds() {
        assert!(is_utc_seconds("2000-02-29T12:00:00Z"));
        assert!(!is_utc_seconds("1900-02-29T12:00:00Z"));
        assert!(!is_utc_seconds("2024-01-01T12:00:00.1Z"));
    }

    #[test]
    fn formatting_preserves_epoch_and_day_rollovers() {
        for (seconds, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (86_399, "1970-01-01T23:59:59Z"),
            (86_400, "1970-01-02T00:00:00Z"),
            (946_684_800, "2000-01-01T00:00:00Z"),
        ] {
            assert_eq!(format_utc(seconds), expected);
            assert!(parse(&format_utc(seconds)).is_some());
        }
    }

    #[test]
    fn expiry_is_decided_in_epoch_seconds_for_any_clock() {
        use std::time::Duration;
        let at = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(is_expired("1970-01-01T00:00:10Z", at(9)), Some(false));
        assert_eq!(is_expired("1970-01-01T00:00:10Z", at(10)), Some(true));
        assert_eq!(
            is_expired("1970-01-01T00:00:10Z", at(9) + Duration::from_millis(999)),
            Some(false)
        );
        // A clock beyond year 9999 still expires every catalog.
        assert_eq!(
            is_expired("9999-12-31T23:59:59Z", at(253_402_300_800 * 2)),
            Some(true)
        );
        // A clock before the epoch leaves later expiries valid.
        let before = UNIX_EPOCH - Duration::from_secs(5);
        assert_eq!(is_expired("1970-01-01T00:00:00Z", before), Some(false));
        assert_eq!(is_expired("1969-12-31T23:59:55Z", before), Some(true));
        assert_eq!(
            is_expired("1969-12-31T23:59:55Z", before + Duration::from_millis(500)),
            Some(true)
        );
        assert_eq!(is_expired("not a time", at(0)), None);
        assert_eq!(is_expired("1970-01-01T00:00:00.5Z", at(0)), None);
    }

    proptest! {
        #[test]
        fn epoch_seconds_invert_formatting(seconds in 0u64..253_402_300_799) {
            let parts = parse(&format_utc(seconds)).expect("formatted time parses");
            prop_assert_eq!(unix_seconds(parts), i64::try_from(seconds).unwrap());
        }

        #[test]
        fn formatted_times_parse_and_sort_chronologically(a in 0u64..253_402_300_799, b in 0u64..253_402_300_799) {
            let (left, right) = (format_utc(a), format_utc(b));
            prop_assert!(is_utc_seconds(&left));
            prop_assert_eq!(a.cmp(&b), parse(&left).cmp(&parse(&right)));
            prop_assert_eq!(a.cmp(&b), left.cmp(&right));
        }
    }
}
