//! Pinned Playwright coverage projection; offsets remain UTF-16 code units.
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A known drain exceeding the budget must not prevent collector release;
/// publish explicit incomplete evidence without rereading counters.
pub(crate) fn encode_coverage(
    value: &Value,
    format: &'static str,
) -> Result<(Vec<u8>, &'static str, bool), &'static str> {
    let bytes = serde_json::to_vec(value).map_err(|_| "BACKEND_PROTOCOL")?;
    let maximum = 8 * 1024 * 1024;
    if bytes.len() <= maximum {
        return Ok((bytes, format, true));
    }
    let status = json!({"complete":false,"reason":"coverage-result-byte-bound","requestedFormat":format,"resultBytes":bytes.len(),"maxResultBytes":maximum,"coverageDataAvailable":false,"replayed":false});
    Ok((
        serde_json::to_vec(&status).map_err(|_| "BACKEND_PROTOCOL")?,
        "native-coverage-capture-status-json",
        false,
    ))
}

pub(crate) fn javascript_entries(mut value: Value) -> Value {
    let mut entries = value["result"].take();
    if !entries.is_array() {
        entries = json!([]);
    }
    let mut result = json!({});
    result["entries"] = entries;
    result
}

/// The pinned CSS converter sweeps nested ranges, takes the innermost hit
/// count, joins adjacent used spans, then excludes spans of one code unit.
fn disjoint(ranges: &[Value]) -> Vec<Value> {
    let mut points = Vec::with_capacity(ranges.len() * 2);
    for range in ranges {
        let (Some(start), Some(end), Some(used)) = (
            range["startOffset"].as_u64(),
            range["endOffset"].as_u64(),
            range["used"].as_bool(),
        ) else {
            continue;
        };
        if end <= start {
            continue;
        }
        points.push((start, true, end - start, used));
        points.push((end, false, end - start, used));
    }
    points.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| if a.1 { b.2.cmp(&a.2) } else { a.2.cmp(&b.2) })
    });
    let mut hits = Vec::new();
    let mut result: Vec<(u64, u64)> = Vec::new();
    let mut previous = 0;
    for (offset, start, _, used) in points {
        if previous < offset && hits.last() == Some(&true) {
            if let Some(last) = result.last_mut().filter(|last| last.1 == previous) {
                last.1 = offset;
            } else {
                result.push((previous, offset));
            }
        }
        previous = offset;
        if start {
            hits.push(used);
        } else {
            hits.pop();
        }
    }
    result
        .into_iter()
        .filter(|(start, end)| end - start > 1)
        .map(|(start, end)| json!({"start":start,"end":end}))
        .collect()
}

/// `sheets` contains exact private URLs and optional source text. Unlike the
/// ordinary status metadata, report URLs must not redact queries.
pub(crate) fn css_result(
    value: &Value,
    mut sheets: BTreeMap<String, (String, Option<String>)>,
    order: &[String],
    sources: bool,
) -> Value {
    let mut grouped: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for range in value["ruleUsage"]
        .as_array()
        .or_else(|| value["coverage"].as_array())
        .into_iter()
        .flatten()
    {
        if let Some(id) = range["styleSheetId"].as_str() {
            grouped.entry(id).or_default().push(range.clone());
        }
    }
    let entries = order
        .iter()
        .filter_map(|id| sheets.remove(id).map(|entry| (id, entry)))
        .filter(|(_, (url, text))| !url.is_empty() && (!sources || text.is_some()))
        .map(|(id, (url, text))| {
            let mut entry =
                json!({"ranges":disjoint(grouped.get(id.as_str()).map_or(&[],Vec::as_slice))});
            entry["url"] = Value::String(url);
            if sources {
                entry["text"] = text.map_or(Value::Null, Value::String);
            }
            entry
        })
        .collect::<Vec<_>>();
    let mut result = json!({});
    result["entries"] = Value::Array(entries);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_coverage_publishes_explicit_incomplete_status_without_source_content() {
        let value = json!({"entries":[{"source":"Acme".repeat(1024*1024),"ranges":"range".repeat(1024*1024)}]});
        let (bytes, format, complete) =
            encode_coverage(&value, "playwright-javascript-coverage").unwrap();
        assert!(!complete);
        assert_eq!(format, "native-coverage-capture-status-json");
        let status: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status["reason"], "coverage-result-byte-bound");
        assert_eq!(status["replayed"], false);
        assert_eq!(status["coverageDataAvailable"], false);
        assert!(bytes.len() < 1024);
        assert!(!String::from_utf8(bytes).unwrap().contains("Acme"));
    }
    #[test]
    fn sdk_css_ranges_preserve_nested_holes_adjacency_and_utf16_offsets() {
        let ranges = vec![
            json!({"startOffset":0,"endOffset":20,"used":true}),
            json!({"startOffset":4,"endOffset":9,"used":false}),
            json!({"startOffset":12,"endOffset":14,"used":true}),
            json!({"startOffset":20,"endOffset":24,"used":true}),
            json!({"startOffset":30,"endOffset":31,"used":true}),
        ];
        assert_eq!(
            disjoint(&ranges),
            vec![json!({"start":0,"end":4}), json!({"start":9,"end":24})]
        );
        let sheets = [
            (
                "a".into(),
                (
                    "https://example.com/acme.css?token=Acme".into(),
                    Some("/* 😀 */ .acme {}".into()),
                ),
            ),
            (
                "b".into(),
                ("https://example.com/unused.css".into(), Some(String::new())),
            ),
            (
                "missing".into(),
                ("https://example.com/missing.css".into(), None),
            ),
        ]
        .into_iter()
        .collect();
        let result = css_result(
            &json!({"coverage":[{"styleSheetId":"a","startOffset":9,"endOffset":17,"used":true}]}),
            sheets,
            &["a".into(), "b".into(), "missing".into()],
            true,
        );
        assert_eq!(result["entries"].as_array().unwrap().len(), 2);
        assert_eq!(
            result["entries"][0]["ranges"],
            json!([{"start":9,"end":17}])
        );
        assert_eq!(
            result["entries"][0]["url"],
            "https://example.com/acme.css?token=Acme"
        );
        assert_eq!(result["entries"][1]["ranges"], json!([]));
    }
    #[test]
    fn sdk_javascript_preserves_functions_counts_and_optional_source() {
        let entry = json!({"scriptId":"1","url":"https://example.com/acme.js","source":"/* 😀 */","functions":[{"functionName":"acme","ranges":[{"startOffset":3,"endOffset":8,"count":2}],"isBlockCoverage":true}]});
        assert_eq!(
            javascript_entries(json!({"timestamp":3,"result":[entry.clone()]})),
            json!({"entries":[entry]})
        );
    }
}
