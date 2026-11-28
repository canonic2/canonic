//! Read durable browser summaries, skipping operation results written by older managers.
use crate::{
    error::{Error, Result},
    managed::storage,
};
use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};
use serde_json::{Map, Value};
use std::path::Path;

const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Default, Deserialize)]
struct Snapshot {
    browsers: Option<Vec<Row>>,
}

#[derive(Deserialize, Serialize)]
struct Row {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    browser: Option<Option<Browser>>,
    #[serde(flatten)]
    fields: Map<String, Value>,
}

#[derive(Deserialize, Serialize)]
struct Browser {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    operations: Option<Option<Vec<Operation>>>,
    #[serde(flatten)]
    fields: Map<String, Value>,
}

#[derive(Deserialize, Serialize)]
struct Operation {
    #[serde(
        default,
        deserialize_with = "omit_result",
        skip_serializing_if = "Option::is_none"
    )]
    result: Option<()>,
    #[serde(flatten)]
    fields: Map<String, Value>,
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

fn omit_result<'de, D: Deserializer<'de>>(input: D) -> std::result::Result<Option<()>, D::Error> {
    IgnoredAny::deserialize(input).map(|_| Some(()))
}

pub(super) fn read(path: &Path) -> Result<Vec<Value>> {
    let record = storage::read_streamed::<Snapshot>(path, MAX_SNAPSHOT_BYTES)?.unwrap_or_default();
    record
        .browsers
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            serde_json::to_value(row)
                .map_err(|_| Error::new("STATE_IO", "Cannot project browser status"))
        })
        .collect()
}

pub(super) fn compact(row: &mut Value, limit: usize) {
    if let Some(browser) = row["browser"].as_object_mut() {
        browser.retain(|key, _| {
            matches!(
                key.as_str(),
                "connectionId"
                    | "sessionId"
                    | "sessionGeneration"
                    | "backend"
                    | "engine"
                    | "phase"
                    | "identity"
                    | "selectedContext"
                    | "selectedPage"
                    | "failure"
                    | "operations"
            )
        });
        if let Some(operations) = browser.get_mut("operations").and_then(Value::as_array_mut) {
            let count = operations.len();
            // Older snapshots lack submission order. Preserve their order within each group.
            operations.sort_by_key(|operation| operation["settled"] != false);
            operations.truncate(limit);
            browser.insert("operationsTotal".into(), Value::from(count));
        }
        browser.insert("detailsOmitted".into(), Value::Bool(true));
    }
}
