//! Browser-generated network console entries and exact native request identity.
use super::*;

impl Worker {
    pub(super) fn network_log(
        &self,
        session: &str,
        entry: &Value,
        setup_pending: bool,
        boundary: Option<&str>,
    ) -> Option<Value> {
        if entry["source"] != "network" {
            return None;
        }
        let (context, _page, worker) = self.index.diagnostic_scope(session)?;
        // Log.enable replays retained entries. Preserve explicitly labelled
        // startup history only for newly owned targets observed during collection.
        if !self.diagnostics.log_sessions.contains(session)
            && !self.startup_paused(session)
            && worker
                .is_none_or(|worker| self.diagnostics.new_workers.get(&worker) != Some(&context))
        {
            return None;
        }
        let source_url = entry["url"].as_str();
        let request = entry["networkRequestId"]
            .as_str()
            .and_then(|id| {
                self.diagnostics
                    .requests
                    .get(&(session.to_owned(), id.to_owned()))
            })
            .filter(|request| {
                request.context == context
                    && request
                        .source_url
                        .as_deref()
                        .is_some_and(|url| Some(url) == source_url)
            });
        let request_id = request.and_then(|request| {
            serde_json::from_value::<Uuid>(request.data["requestId"].clone()).ok()
        });
        let (text, truncated) =
            diagnostics::text(entry["text"].as_str().unwrap_or("Network console entry"));
        Some(
            json!({"level":entry["level"],"source":"network","text":text,"textTruncated":truncated,
            "location":location_summary(entry),"sourceTimestamp":entry["timestamp"],"worker":worker,
            "requestId":request_id,"requestIdentity":request_id.map(|_|"native-network-id-and-exact-url"),
            "argumentsRetained":false,"stackAvailable":entry["stackTrace"].is_object(),
            "stack":entry.get("stackTrace").filter(|stack|stack.is_object()).map(diagnostics::stacks::cdp),
            "attachmentSetupPending":setup_pending,"subscriptionBoundary":boundary}),
        )
    }
}
