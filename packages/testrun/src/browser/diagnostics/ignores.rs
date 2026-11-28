//! Read-scoped noise annotations preserve original events and failure states.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IgnoreRule {
    pub reason: String,
    pub kind: DiagnosticKind,
    #[serde(default)]
    pub page: Option<Reference>,
    #[serde(default)]
    pub worker: Option<Reference>,
    #[serde(default)]
    pub request_id: Option<Uuid>,
    #[serde(default)]
    pub web_socket_id: Option<Uuid>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
}
impl IgnoreRule {
    fn query(&self) -> Query {
        Query {
            kind: Some(self.kind),
            page: self.page,
            worker: self.worker,
            request_id: self.request_id,
            web_socket_id: self.web_socket_id,
            url: self.url.clone(),
            status: self.status,
            ..Default::default()
        }
    }
    pub(super) fn validate(&self) -> Result<(), &'static str> {
        if self.reason.is_empty()
            || self.reason.len() > 256
            || self.reason.chars().any(char::is_control)
            || (self.request_id.is_none() && self.web_socket_id.is_none() && self.url.is_none())
            || (self.web_socket_id.is_some() && self.kind != DiagnosticKind::WebSocket)
        {
            return Err("DIAGNOSTIC_IGNORE_BOUNDS");
        }
        self.query().validate()
    }
}
pub(super) fn annotate(event: &mut Value, context: Reference, rules: &[IgnoreRule]) {
    let matches = rules.iter().enumerate().filter(|(_,rule)|rule.query().matches(context,event))
        .map(|(index,rule)|json!({"ruleIndex":index,"reason":rule.reason,"scope":{"context":context,"page":rule.page,"worker":rule.worker},"lifetime":"this-read"})).collect::<Vec<_>>();
    if !matches.is_empty() {
        event["data"]["ignored"] = json!(true);
        event["data"]["ignoreRules"] = json!(matches);
    }
}
pub(super) fn schema(reference: &Value) -> Value {
    json!({"type":"array","maxItems":4,"default":[],"items":{"type":"object","properties":{
        "reason":{"type":"string","minLength":1,"maxLength":256},"kind":{"enum":["console","page_error","request","response","request_finished","request_failed","web_socket"]},
        "page":{"anyOf":[reference,{"type":"null"}]},"worker":{"anyOf":[reference,{"type":"null"}]},
        "requestId":{"type":["string","null"],"format":"uuid"},"webSocketId":{"type":["string","null"],"format":"uuid"},
        "url":{"type":["string","null"],"maxLength":2048},"status":{"type":["integer","null"],"minimum":100,"maximum":599}},
        "required":["reason","kind"],"additionalProperties":false},"description":"Read-scoped annotations; require an exact requestId, webSocketId or redacted URL. Events remain visible with their original failure states."})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignore_annotations_preserve_failures_scope_and_subsequent_unannotated_reads() {
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let page = Reference {
            id: Uuid::new_v4(),
            ..context
        };
        let other = Reference {
            id: Uuid::new_v4(),
            ..context
        };
        let id = Uuid::new_v4();
        let mut history = Diagnostics::default();
        for owner in [context, other] {
            history.push(owner,Some(page),DiagnosticKind::RequestFailed,json!({"requestId":id,"url":"https://example.com/api","cancelled":true,"text":"Acme private failure"}),0);
        }
        let rule = IgnoreRule {
            reason: "Expected cancellation after owned navigation".into(),
            kind: DiagnosticKind::RequestFailed,
            page: Some(page),
            worker: None,
            request_id: Some(id),
            web_socket_id: None,
            url: None,
            status: None,
        };
        let mut query = Query {
            ignore_rules: vec![rule.clone()],
            ..Default::default()
        };
        let read = history.read(context, &query).unwrap();
        assert_eq!(read["events"].as_array().unwrap().len(), 1);
        let event = &read["events"][0];
        assert_eq!(event["kind"], "request_failed");
        assert_eq!(event["data"]["cancelled"], true);
        assert_eq!(event["data"]["ignored"], true);
        assert_eq!(
            event["data"]["ignoreRules"][0]["scope"]["context"],
            json!(context)
        );
        assert!(!read.to_string().contains("Acme private failure"));
        assert_eq!(read["nextCursor"], 2);
        let default = history.read(context, &Query::default()).unwrap();
        assert!(default["events"][0]["data"].get("ignored").is_none());
        query.ignore_rules[0].request_id = Some(Uuid::new_v4());
        assert!(
            history.read(context, &query).unwrap()["events"][0]["data"]
                .get("ignored")
                .is_none()
        );
        query.ignore_rules[0].request_id = None;
        assert_eq!(
            history.read(context, &query).unwrap_err(),
            "DIAGNOSTIC_IGNORE_BOUNDS"
        );
        let worker = Reference {
            id: Uuid::new_v4(),
            ..context
        };
        history.push(context,Some(page),DiagnosticKind::RequestFailed,json!({"requestId":id,"worker":worker,"url":format!("https://example.com/{}","a".repeat(2000)),"text":"\0".repeat(2048)}),1);
        let mut bounded_rule = rule.clone();
        bounded_rule.reason = "\"".repeat(256);
        bounded_rule.worker = Some(worker);
        let bounded = history
            .read(
                context,
                &Query {
                    sequence: Some(3),
                    include_text: true,
                    ignore_rules: vec![bounded_rule; 4],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            bounded["events"][0]["data"]["ignoreRules"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(bounded["events"][0]["data"]["ignored"], true);
        assert_eq!(bounded["nextCursor"], 3);
        assert!(serde_json::to_vec(&bounded).unwrap().len() < 8000);
        query.ignore_rules = vec![rule; 5];
        assert!(history.read(context, &query).is_err());
    }
}
