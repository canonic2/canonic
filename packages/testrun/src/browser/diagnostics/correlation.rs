//! Exact source identities and explicitly unproven retained request candidates.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Correlation {
    #[serde(default = "window")]
    pub window_ms: u64,
    #[serde(default = "limit")]
    pub limit: usize,
}
fn window() -> u64 {
    5000
}
fn limit() -> usize {
    4
}
impl Default for Correlation {
    fn default() -> Self {
        Self {
            window_ms: window(),
            limit: limit(),
        }
    }
}
struct Candidate {
    value: Value,
    url: String,
    delta: u64,
    failed: bool,
}
fn clipped(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}
impl Diagnostics {
    pub(super) fn correlate(
        &self,
        event: &Value,
        options: &Correlation,
        bounds: (Option<u64>, Option<u64>),
    ) -> Value {
        let exact = (event["data"]["requestIdentity"] == "native-network-id-and-exact-url")
            .then(|| serde_json::from_value::<Uuid>(event["data"]["requestId"].clone()).ok())
            .flatten();
        let observed = event["observedAtUnixMs"].as_u64();
        let mut candidates = BTreeMap::<Uuid, Candidate>::new();
        for record in &self.events {
            let sequence = record["sequence"].as_u64().expect("Diagnostic sequence");
            if bounds.0.is_some_and(|after| sequence <= after)
                || bounds.1.is_some_and(|through| sequence > through)
            {
                continue;
            }
            if record["context"] != event["context"]
                || (!event["page"].is_null() && record["page"] != event["page"])
                || (!event["data"]["worker"].is_null()
                    && record["data"]["worker"] != event["data"]["worker"])
                || (event["data"]["worker"].is_null()
                    && !event["page"].is_null()
                    && !record["data"]["worker"].is_null())
                || !matches!(
                    record["kind"].as_str(),
                    Some("request" | "response" | "request_finished" | "request_failed")
                )
            {
                continue;
            }
            let (Some(observed), Some(time)) = (observed, record["observedAtUnixMs"].as_u64())
            else {
                continue;
            };
            let delta = observed.abs_diff(time);
            if delta > options.window_ms {
                continue;
            }
            let Some(id) = serde_json::from_value::<Uuid>(record["data"]["requestId"].clone()).ok()
            else {
                continue;
            };
            if exact.is_some_and(|exact| exact != id) {
                continue;
            }
            let source_url = url(record["data"]["url"].as_str().unwrap_or(""));
            let candidate=candidates.entry(id).or_insert_with(||Candidate{
                value:json!({"requestId":id,"url":clipped(&source_url,256),"urlTruncated":source_url.len()>256,"method":clipped(record["data"]["method"].as_str().unwrap_or(""),32),"page":record["page"],"worker":record["data"]["worker"],"firstSequence":record["sequence"],"status":null}),
                url:source_url,delta,failed:false,
            });
            candidate.delta = candidate.delta.min(delta);
            candidate.failed |= record["kind"] == "request_failed";
            if record["data"]["status"].is_number() {
                candidate.value["status"] = record["data"]["status"].clone();
            }
            candidate.value["lastSequence"] = record["sequence"].clone();
            candidate.value["failed"] = json!(candidate.failed);
            if record["data"]["cancelled"].is_boolean() {
                candidate.value["cancelled"] = record["data"]["cancelled"].clone();
            }
            if record["data"]["initialEventMissing"].is_boolean() {
                candidate.value["initialEventMissing"] =
                    record["data"]["initialEventMissing"].clone();
            }
            candidate.value["deltaMs"] = json!(candidate.delta);
        }
        candidates.retain(|_, candidate| {
            exact.is_some()
                || candidate.failed
                || candidate.value["status"]
                    .as_f64()
                    .is_some_and(|status| (400.0..=599.0).contains(&status))
        });
        let source_url = url(event["data"]["location"]["url"].as_str().unwrap_or(""));
        let url_match = source_url.starts_with("http://") || source_url.starts_with("https://");
        let mut basis = "scope-and-observation-time";
        if exact.is_some() {
            basis = "native-network-id-and-exact-url";
        } else if event["data"]["source"] == "network"
            && (source_url.starts_with("ws://") || source_url.starts_with("wss://"))
        {
            candidates.clear();
            basis = "non-http-network-source";
        } else if url_match
            && (event["data"]["source"] == "network"
                || candidates
                    .values()
                    .any(|candidate| candidate.url == source_url))
        {
            candidates.retain(|_, candidate| candidate.url == source_url);
            basis = "redacted-url-scope-and-observation-time";
        }
        let count = candidates.len();
        let mut candidates = candidates.into_values().collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| {
            (
                candidate.delta,
                candidate.value["firstSequence"].as_u64().unwrap_or(0),
            )
        });
        json!({"basis":basis,"exactRequestId":exact,"confidence":if exact.is_some(){"source-identity"}else{"candidate-only"},"causalProof":false,
            "ambiguous":exact.is_none()&&count>1,"candidateCount":count,"candidates":candidates.into_iter().take(options.limit).map(|candidate|candidate.value).collect::<Vec<_>>(),
            "truncated":count>options.limit,"windowMs":options.window_ms,"history":"bounded-retained-events","oldestCursor":self.evicted_through,"historyLossObserved":self.dropped>0})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(connection_id: Uuid) -> Reference {
        Reference {
            connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "One bounded correlation history verifies ambiguity, privacy and source identity under retention pressure"
    )]
    fn ambiguous_candidates_preserve_scope_privacy_limits_and_source_identity() {
        let connection = Uuid::new_v4();
        let context = reference(connection);
        let page = reference(connection);
        let other_context = reference(connection);
        let other_page = reference(connection);
        let foreign_page = reference(connection);
        let worker = reference(connection);
        let mut history = Diagnostics::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let foreign = Uuid::new_v4();
        for (owner, page, id, time) in [
            (context, page, first, 1000),
            (context, page, second, 1002),
            (other_context, foreign_page, foreign, 1000),
            (context, other_page, Uuid::new_v4(), 1000),
            (context, page, Uuid::new_v4(), 10000),
        ] {
            history.push(owner,Some(page),DiagnosticKind::Response,json!({"requestId":id,"url":"https://user:Acme-private-token@example.com/api?token=Acme-private-token","method":"POST","status":409}),time);
        }
        history.push(context,Some(page),DiagnosticKind::Response,json!({"requestId":Uuid::new_v4(),"url":"https://example.com/api","method":"POST","status":409,"worker":worker}),1000);
        history.push(context,Some(page),DiagnosticKind::Console,json!({"level":"error","text":"Acme private console 409","location":{"url":"https://example.com/api"}}),1001);
        let sequence = history.sequence;
        let mut query = Query {
            sequence: Some(sequence),
            correlation: Some(Correlation {
                window_ms: 5000,
                limit: 1,
            }),
            ..Default::default()
        };
        let read = history.read(context, &query).unwrap();
        let result = &read["events"][0]["data"]["requestCorrelation"];
        assert_eq!(result["candidateCount"], 2);
        assert_eq!(result["ambiguous"], true);
        assert_eq!(result["truncated"], true);
        assert_eq!(result["candidates"].as_array().unwrap().len(), 1);
        assert!(result["exactRequestId"].is_null());
        assert_eq!(result["causalProof"], false);
        assert_eq!(result["confidence"], "candidate-only");
        assert!(!read.to_string().contains("Acme-private-token"));
        assert!(!read.to_string().contains("Acme private console"));
        history.push(
            context,
            None,
            DiagnosticKind::Console,
            json!({"level":"error","location":{"url":"https://example.com/api"}}),
            1001,
        );
        let scoped = history
            .read(
                context,
                &Query {
                    sequence: Some(history.sequence),
                    correlation: Some(Correlation::default()),
                    ..Default::default()
                },
            )
            .unwrap();
        let candidates = &scoped["events"][0]["data"]["requestCorrelation"];
        assert_eq!(candidates["candidateCount"], 4);
        assert!(
            !candidates["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| candidate["requestId"] == json!(foreign))
        );
        history.push(context,Some(page),DiagnosticKind::Console,json!({"source":"network","level":"error","location":{"url":"wss://example.com/socket"}}),1001);
        let read = history
            .read(
                context,
                &Query {
                    sequence: Some(history.sequence),
                    correlation: Some(Correlation::default()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            read["events"][0]["data"]["requestCorrelation"]["candidateCount"],
            0
        );
        assert_eq!(
            read["events"][0]["data"]["requestCorrelation"]["basis"],
            "non-http-network-source"
        );
        history.push(context,Some(page),DiagnosticKind::Console,json!({"level":"error","requestId":second,"requestIdentity":"native-network-id-and-exact-url","location":{"url":"https://example.com/api"}}),1003);
        query.sequence = Some(history.sequence);
        let read = history.read(context, &query).unwrap();
        let result = &read["events"][0]["data"]["requestCorrelation"];
        assert_eq!(result["candidateCount"], 1);
        assert_eq!(result["exactRequestId"], json!(second));
        assert_eq!(result["ambiguous"], false);
        assert_eq!(result["causalProof"], false);
        history.push(context,Some(page),DiagnosticKind::Response,json!({"requestId":Uuid::new_v4(),"url":"https://example.com/api","method":"POST","status":409}),1001);
        let ranged = history
            .read(
                context,
                &Query {
                    after: Some(1),
                    through: Some(sequence),
                    kind: Some(DiagnosticKind::Console),
                    page: Some(page),
                    correlation: Some(Correlation::default()),
                    ..Default::default()
                },
            )
            .unwrap();
        let candidates = &ranged["events"][0]["data"]["requestCorrelation"];
        assert_eq!(candidates["candidateCount"], 1);
        assert_eq!(candidates["candidates"][0]["requestId"], json!(second));
        history.source_gap(1);
        history.push(
            context,
            Some(page),
            DiagnosticKind::Console,
            json!({"level":"error","location":{"url":"https://example.com/api"}}),
            1004,
        );
        query.sequence = Some(history.sequence);
        let read = history.read(context, &query).unwrap();
        let result = &read["events"][0]["data"]["requestCorrelation"];
        assert_eq!(result["candidateCount"], 0);
        assert_eq!(result["historyLossObserved"], true);
        query.correlation.as_mut().unwrap().limit = 5;
        assert_eq!(
            history.read(context, &query).unwrap_err(),
            "DIAGNOSTIC_QUERY_BOUNDS"
        );
    }
}
