//! One bounded path for browser, server and editor diagnostics. Records stay
//! small and textual, so prompts, DOM snapshots and image bytes never become
//! logs, and one noisy session stops logging at its byte budget after saying
//! so once. The host writes the records where its editor keeps logs.
use crate::host::Host;
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};

/// The default budget of one session's records, in bytes.
pub(crate) const SESSION_BYTES: usize = 2 * 1024 * 1024;
const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

/// A record as it is logged: a known level, a short event, at most twenty
/// details, each a number, boolean, null or text of at most 2000 characters.
pub(crate) fn record(level: &str, event: &str, details: &Value) -> Value {
    let level = if LEVELS.contains(&level) {
        level
    } else {
        "info"
    };
    let event: String = if event.is_empty() {
        "workbench".to_owned()
    } else {
        event.chars().take(120).collect()
    };
    let mut kept = Map::new();
    for (key, value) in details.as_object().into_iter().flatten().take(20) {
        let value = match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
            Value::String(text) => Value::String(text.chars().take(2000).collect()),
            other => Value::String(other.to_string().chars().take(2000).collect()),
        };
        kept.insert(key.clone(), value);
    }
    json!({ "level": level, "event": event, "details": kept })
}

struct Budget {
    used: usize,
    reported: bool,
}

#[derive(Clone)]
pub(crate) struct Diagnostics {
    host: Arc<Host>,
    limit: usize,
    budget: Arc<Mutex<Budget>>,
}

impl Diagnostics {
    pub(crate) fn new(host: Arc<Host>, limit: usize) -> Self {
        Self {
            host,
            limit,
            budget: Arc::new(Mutex::new(Budget {
                used: 0,
                reported: false,
            })),
        }
    }

    /// The record to log within the budget, the one-time notice that the
    /// budget is spent, or nothing.
    fn admit(&self, record: Value) -> Option<Value> {
        let size = record.to_string().len();
        let mut budget = self
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if budget.used + size > self.limit {
            if budget.reported {
                return None;
            }
            budget.reported = true;
            return Some(
                json!({ "level": "warn", "event": "log.limit.reached", "details": { "bytes": budget.used, "limit": self.limit } }),
            );
        }
        budget.used += size;
        Some(record)
    }

    pub(crate) async fn log(&self, level: &str, event: &str, details: Value) {
        if let Some(record) = self.admit(record(level, event, &details)) {
            self.host.log(record).await;
        }
    }

    pub(crate) async fn info(&self, event: &str, details: Value) {
        self.log("info", event, details).await;
    }

    pub(crate) async fn warn(&self, event: &str, details: Value) {
        self.log("warn", event, details).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_keep_known_levels_short_events_and_small_details() {
        let long = "x".repeat(3000);
        let details: Map<String, Value> = (0..25).map(|at| (format!("k{at}"), json!(at))).collect();
        let shaped = record("loud", &"e".repeat(200), &Value::Object(details));
        assert_eq!(shaped["level"], "info");
        assert_eq!(shaped["event"].as_str().unwrap().len(), 120);
        assert_eq!(shaped["details"].as_object().unwrap().len(), 20);
        let text = record(
            "warn",
            "a",
            &json!({ "message": long, "nested": { "a": 1 } }),
        );
        assert_eq!(text["details"]["message"].as_str().unwrap().len(), 2000);
        assert_eq!(text["details"]["nested"], "{\"a\":1}");
    }
}
