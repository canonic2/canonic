//! JavaScript's value rules for text the browser composed: truthiness,
//! `String(value)`, `Math.round` and `Date#toISOString`, so prompts and
//! reports read the same as the canvas and the Node server wrote them.

use serde_json::Value;

/// Whether JavaScript would treat the value as true; `None` is `undefined`.
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `String(value)`; `None` is `undefined`.
pub(crate) fn text(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number_text(number.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => text(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// A number as JavaScript prints it.
pub(crate) fn number_text(number: f64) -> String {
    if number.is_nan() {
        return "NaN".into();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .into();
    }
    if number == 0.0 {
        return "0".into();
    }
    if number.fract() == 0.0 && number.abs() < 1e21 {
        return format!("{number:.0}");
    }
    format!("{number}")
}

/// `Math.round(Number(value))`; `None` is `undefined`, which rounds to `NaN`.
pub(crate) fn round(value: Option<&Value>) -> f64 {
    let number = match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    (number + 0.5).floor()
}

/// The length JavaScript reports for a string: its UTF-16 code units.
pub(crate) fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Whether JavaScript's `trim()` leaves anything.
pub(crate) fn blank(text: &str) -> bool {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
        .is_empty()
}

/// `new Date(ms).toISOString()`, for milliseconds since the Unix epoch.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a date's milliseconds fit an i64"
)]
pub(crate) fn iso_time(ms: f64) -> String {
    let ms = ms.floor() as i64;
    let days = ms.div_euclid(86_400_000);
    let of_day = ms.rem_euclid(86_400_000);
    // Howard Hinnant's days-to-civil conversion.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1000 % 60,
        of_day % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_print_and_test_as_javascript_does() {
        assert_eq!(text(Some(&json!(393))), "393");
        assert_eq!(text(Some(&json!(1.5))), "1.5");
        assert_eq!(text(None), "undefined");
        assert_eq!(text(Some(&json!(null))), "null");
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(Some(&json!(0))));
        assert!(truthy(Some(&json!([]))));
        assert!((round(Some(&json!(2.5))) - 3.0).abs() < f64::EPSILON);
        assert!((round(Some(&json!(-2.5))) + 2.0).abs() < f64::EPSILON);
        assert_eq!(number_text(round(None)), "NaN");
    }

    #[test]
    fn dates_print_as_iso_strings() {
        // Date.UTC(2026, 9, 3, 12) + 3000
        assert_eq!(iso_time(1_791_028_803_000.0), "2026-10-03T12:00:03.000Z");
        assert_eq!(iso_time(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(-1.0), "1969-12-31T23:59:59.999Z");
    }
}
