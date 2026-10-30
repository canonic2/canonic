//! Validated UTC timestamps and explicit-clock retention arithmetic without host clock I/O.

use super::TaskError;
use serde::Serialize;
use std::cmp::Ordering;

/// A UTC RFC 3339 timestamp. Comparisons use instants, not textual precision.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Timestamp {
    text: String,
    #[serde(skip)]
    seconds: u64,
    #[serde(skip)]
    fraction: String,
}

impl Timestamp {
    /// Converts a consumer-supplied Unix millisecond clock reading to UTC.
    /// This function does not read the system clock.
    ///
    /// # Errors
    /// Returns an error for instants beyond the four-digit RFC 3339 year range.
    pub fn from_unix_millis(millis: u64) -> Result<Self, TaskError> {
        if millis > 253_402_300_799_999 {
            return Err(TaskError::Invalid("timestamp"));
        }
        // Gregorian civil-date conversion; days are bounded to years 1970..9999.
        let days = i64::try_from(millis / 86_400_000)
            .map_err(|_| TaskError::Invalid("timestamp"))?
            + 719_468;
        let era = days / 146_097;
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_prime = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
        let month = month_prime + if month_prime < 10 { 3 } else { -9 };
        year += i64::from(month <= 2);
        let seconds = millis / 1000;
        Self::parse(&format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
            seconds / 3600 % 24,
            seconds / 60 % 60,
            seconds % 60,
            millis % 1000
        ))
    }
    /// Parses UTC timestamps with calendar validation and optional fractional seconds.
    ///
    /// # Errors
    /// Refuses invalid dates/times, leap seconds, offsets and excessive length.
    pub fn parse(text: &str) -> Result<Self, TaskError> {
        let fail = || TaskError::Invalid("timestamp");
        if text.len() < 20 || text.len() > 64 || !text.is_ascii() {
            return Err(fail());
        }
        let number = |start, end| {
            text.get(start..end)
                .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(fail)
        };
        if &text[4..5] != "-"
            || &text[7..8] != "-"
            || &text[10..11] != "T"
            || &text[13..14] != ":"
            || &text[16..17] != ":"
            || !text.ends_with('Z')
        {
            return Err(fail());
        }
        let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
        let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
        let leap = |year: u64| {
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
        };
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap(year) {
                    29
                } else {
                    28
                }
            }
            _ => return Err(fail()),
        };
        if year == 0 || day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
            return Err(fail());
        }
        let mut fraction = String::new();
        if text.len() != 20 {
            if &text[19..20] != "." {
                return Err(fail());
            }
            let digits = &text[20..text.len() - 1];
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(fail());
            }
            fraction = digits.trim_end_matches('0').into();
        }
        let prior = year - 1;
        let before_year = prior * 365 + prior / 4 - prior / 100 + prior / 400;
        let before_month = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334]
            [usize::try_from(month - 1).map_err(|_| fail())?]
            + u64::from(month > 2 && leap(year));
        let seconds =
            ((before_year + before_month + day - 1) * 24 + hour) * 3600 + minute * 60 + second;
        Ok(Self {
            text: text.into(),
            seconds,
            fraction,
        })
    }

    /// Exact source representation, suitable for persistence.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whole elapsed milliseconds between explicit instants, without reading a clock.
    ///
    /// # Errors
    /// Returns a time-regression error if `earlier` is later than this instant.
    pub fn elapsed_millis_since(&self, earlier: &Self) -> Result<u128, TaskError> {
        if self < earlier {
            return Err(TaskError::Transition);
        }
        Ok(self.elapsed_ms(earlier))
    }

    pub(super) fn elapsed_ms(&self, earlier: &Self) -> u128 {
        let millis = |fraction: &str| {
            (0..3).fold(0_u128, |value, index| {
                value * 10
                    + u128::from(fraction.as_bytes().get(index).copied().unwrap_or(b'0') - b'0')
            })
        };
        let elapsed = (u128::from(self.seconds.saturating_sub(earlier.seconds)) * 1000
            + millis(&self.fraction))
        .saturating_sub(millis(&earlier.fraction));
        elapsed.saturating_sub(u128::from(
            self.fraction.get(3..).unwrap_or("") < earlier.fraction.get(3..).unwrap_or(""),
        ))
    }
}

impl PartialEq for Timestamp {
    fn eq(&self, other: &Self) -> bool {
        self.seconds == other.seconds && self.fraction == other.fraction
    }
}
impl Eq for Timestamp {}
impl PartialOrd for Timestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.seconds, &self.fraction).cmp(&(&other.seconds, &other.fraction))
    }
}
