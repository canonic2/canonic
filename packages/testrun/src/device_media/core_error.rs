//! Readable summaries of CoreDevice error envelopes.
//!
//! The idevice crate reports a device's `CoreDevice.error` as the debug text of
//! its `plist::Value` dictionary: `domain`, `code` and
//! `userInfoWithNSSecureCoding`, a keyed archive printed as a byte list. This module reads the domain, code and
//! localized description from that text so errors show a message, not bytes.

/// The parts of a CoreDevice error a person can act on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    pub domain: Option<String>,
    pub code: Option<i64>,
    pub description: Option<String>,
}

impl Summary {
    /// A one-line message, or `None` when the text held nothing readable.
    pub(crate) fn message(&self) -> Option<String> {
        let origin = match (&self.domain, self.code) {
            (Some(domain), Some(code)) => Some(format!("{domain} error {code}")),
            (Some(domain), None) => Some(format!("{domain} error")),
            (None, Some(code)) => Some(format!("error {code}")),
            (None, None) => None,
        };
        match (origin, &self.description) {
            (Some(origin), Some(description)) => Some(format!("{description} ({origin})")),
            (None, Some(description)) => Some(description.clone()),
            (origin, None) => origin,
        }
    }
}

/// Reads a CoreDevice error from idevice's debug text of the plist dictionary.
pub(crate) fn summarize(text: &str) -> Summary {
    Summary {
        domain: between(text, "\"domain\": String(\"", "\")").map(str::to_owned),
        code: between(text, "\"code\": Integer(", ")").and_then(|digits| digits.parse().ok()),
        description: between(text, "\"userInfoWithNSSecureCoding\": Data([", "])")
            .and_then(bytes)
            .and_then(|archive| description(&archive)),
    }
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let rest = &text[text.find(start)? + start.len()..];
    Some(&rest[..rest.find(end)?])
}

fn bytes(list: &str) -> Option<Vec<u8>> {
    list.split(',')
        .map(|byte| byte.trim().parse().ok())
        .collect()
}

/// Finds `NSLocalizedDescription`, then `NSLocalizedFailureReason`, in an
/// `NSKeyedArchiver` user-info dictionary.
fn description(archive: &[u8]) -> Option<String> {
    let root = plist::Value::from_reader(std::io::Cursor::new(archive)).ok()?;
    let objects = root.as_dictionary()?.get("$objects")?.as_array()?;
    fn resolve<'a>(
        objects: &'a [plist::Value],
        value: &'a plist::Value,
    ) -> Option<&'a plist::Value> {
        match value {
            plist::Value::Uid(uid) => usize::try_from(uid.get())
                .ok()
                .and_then(|index| objects.get(index)),
            other => Some(other),
        }
    }
    let resolve = |value| resolve(objects, value);
    let entries: Vec<(&str, &str)> = objects
        .iter()
        .filter_map(plist::Value::as_dictionary)
        .filter_map(|dictionary| {
            let keys = dictionary.get("NS.keys")?.as_array()?;
            let values = dictionary.get("NS.objects")?.as_array()?;
            Some(keys.iter().zip(values).filter_map(|(key, value)| {
                Some((resolve(key)?.as_string()?, resolve(value)?.as_string()?))
            }))
        })
        .flatten()
        .collect();
    ["NSLocalizedDescription", "NSLocalizedFailureReason"]
        .iter()
        .find_map(|wanted| entries.iter().find(|(key, _)| key == wanted))
        .map(|(_, value)| value.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut objects = vec![plist::Value::String("$null".into())];
        let mut keys = Vec::new();
        let mut values = Vec::new();
        for (key, value) in entries {
            objects.push(plist::Value::String((*key).into()));
            keys.push(plist::Value::Uid(plist::Uid::new(objects.len() as u64 - 1)));
            objects.push(plist::Value::String((*value).into()));
            values.push(plist::Value::Uid(plist::Uid::new(objects.len() as u64 - 1)));
        }
        objects.push(plist::Value::Dictionary(plist::Dictionary::from_iter([
            ("NS.keys".to_owned(), plist::Value::Array(keys)),
            ("NS.objects".to_owned(), plist::Value::Array(values)),
        ])));
        let root = plist::Value::Dictionary(plist::Dictionary::from_iter([(
            "$objects".to_owned(),
            plist::Value::Array(objects),
        )]));
        let mut bytes = Vec::new();
        root.to_writer_binary(&mut bytes).unwrap();
        bytes
    }

    /// The text idevice produces: the debug form of the device's error value.
    fn envelope(archive: &[u8]) -> String {
        let error = plist::Value::Dictionary(plist::Dictionary::from_iter([
            ("code".to_owned(), plist::Value::Integer((-4).into())),
            (
                "domain".to_owned(),
                plist::Value::String("com.example.CoreDeviceError".into()),
            ),
            (
                "userInfoWithNSSecureCoding".to_owned(),
                plist::Value::Data(archive.to_vec()),
            ),
        ]));
        format!("{error:?}")
    }

    #[test]
    fn envelope_becomes_a_readable_message_without_bytes() {
        let text = envelope(&archive(&[
            ("NSLocalizedFailureReason", "The service is not ready."),
            (
                "NSLocalizedDescription",
                "The operation could not be completed.",
            ),
        ]));
        let summary = summarize(&text);
        assert_eq!(
            summary.domain.as_deref(),
            Some("com.example.CoreDeviceError")
        );
        assert_eq!(summary.code, Some(-4));
        assert_eq!(
            summary.message().unwrap(),
            "The operation could not be completed. (com.example.CoreDeviceError error -4)"
        );
    }

    #[test]
    fn failure_reason_stands_in_for_a_missing_description() {
        let text = envelope(&archive(&[("NSLocalizedFailureReason", "Not ready.")]));
        assert_eq!(summarize(&text).description.as_deref(), Some("Not ready."));
    }

    #[test]
    fn unreadable_archive_keeps_domain_and_code() {
        let text = envelope(&[98, 112, 1, 2]);
        assert_eq!(
            summarize(&text).message().unwrap(),
            "com.example.CoreDeviceError error -4"
        );
        assert_eq!(summarize("Dictionary({})").message(), None);
    }
}
