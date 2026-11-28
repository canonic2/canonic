//! Latest SDK call states, with bounded incremental private log retention.
use serde_json::{Value, json};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct Calls {
    items: VecDeque<Call>,
    evicted: u64,
    ignored: u64,
}
struct Call {
    wire: String,
    id: Uuid,
    status: String,
    details: Value,
    logs: VecDeque<String>,
    truncated: bool,
    dropped_logs: u64,
}
fn short(value: &Value, truncated: &mut bool) -> String {
    let original = value.as_str().unwrap_or("");
    let result = original.chars().take(256).collect::<String>();
    *truncated |= result.len() < original.len();
    result
}
impl Calls {
    pub(super) fn update(&mut self, params: &Value) {
        let Some(updates) = params["apiCalls"].as_array() else {
            return;
        };
        self.ignored = self
            .ignored
            .saturating_add(updates.len().saturating_sub(512) as u64);
        for update in updates.iter().take(512) {
            let Some(wire) = update["id"]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 256)
            else {
                self.ignored = self.ignored.saturating_add(1);
                continue;
            };
            let Some(status) = update["status"]
                .as_str()
                .filter(|s| matches!(*s, "running" | "success" | "error"))
            else {
                self.ignored = self.ignored.saturating_add(1);
                continue;
            };
            let position = self.items.iter().position(|call| call.wire == wire);
            let mut call = position
                .and_then(|position| self.items.remove(position))
                .unwrap_or_else(|| Call {
                    wire: wire.into(),
                    id: Uuid::new_v4(),
                    status: String::new(),
                    details: Value::Null,
                    logs: VecDeque::new(),
                    truncated: false,
                    dropped_logs: 0,
                });
            call.status = status.into();
            let location = update["location"].is_object().then(||json!({
                "file":short(&update["location"]["file"],&mut call.truncated),
                "line":update["location"]["line"].as_i64(),"column":update["location"]["column"].as_i64()
            }));
            let error = update["error"]
                .is_string()
                .then(|| short(&update["error"], &mut call.truncated));
            call.details = json!({"title":short(&update["title"],&mut call.truncated),"location":location,"error":error});
            if let Some(logs) = update["newLogEntries"].as_array() {
                let skipped = logs.len().saturating_sub(8);
                call.dropped_logs = call.dropped_logs.saturating_add(skipped as u64);
                for log in logs.iter().skip(skipped) {
                    if call.logs.len() == 8 {
                        call.logs.pop_front();
                        call.dropped_logs = call.dropped_logs.saturating_add(1);
                    }
                    call.logs.push_back(short(log, &mut call.truncated));
                }
            }
            if self.items.len() == 32 {
                self.items.pop_front();
                self.evicted = self.evicted.saturating_add(1);
            }
            self.items.push_back(call);
        }
    }
    pub(super) fn read(&self, offset: u16, limit: u16, private: bool) -> Value {
        let items = self.items.iter().rev().skip(usize::from(offset)).take(usize::from(limit)).map(|call| {
            let mut value=json!({"callId":call.id,"status":call.status,"truncated":call.truncated,"droppedLogEntries":call.dropped_logs});
            if private {
                value["details"] = call.details.clone();
                value["logs"] = json!(call.logs);
            }
            value
        }).collect::<Vec<_>>();
        let next = usize::from(offset) + items.len();
        json!({"calls":items,"hasMore":next<self.items.len(),"nextOffset":next,"detailsIncluded":private,
            "retainedCalls":self.items.len(),"evictedCalls":self.evicted,"ignoredUpdates":self.ignored,
            "maxCalls":32,"maxLogsPerCall":8,"maxStringCharacters":256,"snapshot":false,
            "order":"most-recent-update-first","history":"latest-retained-call-states"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_update_batches_report_skipped_records_and_preserve_absent_metadata() {
        let mut calls = Calls::default();
        calls.update(&json!({"apiCalls":(0..600).map(|id|json!({"id":id.to_string(),"title":"Acme","status":"running","newLogEntries":[]})).collect::<Vec<_>>()}));
        let result = calls.read(0, 1, true);
        assert_eq!(result["ignoredUpdates"], 88);
        assert_eq!(result["evictedCalls"], 480);
        assert_eq!(result["calls"][0]["details"]["location"], Value::Null);
        assert_eq!(result["calls"][0]["details"]["error"], Value::Null);
    }
    #[test]
    fn updates_preserve_identity_and_incremental_logs_without_metadata_disclosure() {
        let mut calls = Calls::default();
        calls.update(&json!({"apiCalls":[{"id":"Acme-private-id","title":"Acme-private-title","location":{"file":"Acme-private-path","line":7},"status":"running","newLogEntries":["Acme-private-first"]}]}));
        let first = calls.read(0, 16, false);
        let id = first["calls"][0]["callId"].clone();
        assert!(!first.to_string().contains("Acme-private"));
        calls.update(&json!({"apiCalls":[{"id":"Acme-private-id","title":"Acme-private-title","location":{"file":"Acme-private-path","line":7},"status":"error","error":"Acme-private-error","newLogEntries":["Acme-private-second"]}]}));
        let result = calls.read(0, 16, true);
        assert_eq!(result["retainedCalls"], 1);
        assert_eq!(result["calls"][0]["callId"], id);
        assert_eq!(result["calls"][0]["status"], "error");
        assert_eq!(
            result["calls"][0]["logs"],
            json!(["Acme-private-first", "Acme-private-second"])
        );
        assert_eq!(result["calls"][0]["details"]["location"]["line"], 7);
    }
    #[test]
    fn retention_is_bounded_and_pagination_never_claims_a_stable_snapshot() {
        let mut calls = Calls::default();
        for id in 0..40 {
            calls.update(&json!({"apiCalls":[{"id":id.to_string(),"title":"東京".repeat(300),"status":"success","newLogEntries":vec!["x".repeat(300);12]}]}));
        }
        let result = calls.read(0, 1, true);
        assert_eq!(result["retainedCalls"], 32);
        assert_eq!(result["evictedCalls"], 8);
        assert_eq!(result["calls"][0]["logs"].as_array().unwrap().len(), 8);
        assert_eq!(result["calls"][0]["droppedLogEntries"], 4);
        assert_eq!(
            result["calls"][0]["details"]["title"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            256
        );
        assert_eq!(result["calls"][0]["truncated"], true);
        assert_eq!(result["hasMore"], true);
        assert_eq!(result["snapshot"], false);
        assert_eq!(calls.read(32, 1, false)["calls"], json!([]));
    }
}
