//! Privacy projection and cursor progress for records larger than one read page.
use super::*;

pub(super) fn read(mut event: Value, query: &Query) -> Value {
    let data = event["data"].as_object_mut().expect("Diagnostic data");
    if !query.include_text {
        data.remove("text");
        data.remove("name");
    }
    if !query.include_stack {
        data.remove("stack");
    }
    if data.contains_key("stackAvailable") {
        data.insert(
            "stackIncluded".into(),
            json!(query.include_stack && data.get("stack").is_some_and(Value::is_object)),
        );
    }
    let original_bytes = serde_json::to_vec(&event).expect("Diagnostic event").len();
    if original_bytes <= 6000 {
        return event;
    }
    // A single oversized record must not pin nextCursor forever. Preserve
    // condition/correlation fields first and state the omitted detail explicitly.
    let mut original = std::mem::take(event["data"].as_object_mut().expect("Diagnostic data"));
    event["data"] = json!({"detailsTruncatedForRead":true,"requestedRecordBytes":original_bytes,"stackIncluded":false});
    for key in [
        "requestId",
        "requestIdentity",
        "source",
        "webSocketId",
        "method",
        "status",
        "worker",
        "initialEventMissing",
        "phase",
        "direction",
        "level",
        "ignored",
        "ignoreRules",
        "url",
        "requestCorrelation",
        "location",
        "sourceTimestamp",
        "stackAvailable",
        "argumentsRetained",
        "arguments",
    ] {
        if let Some(value) = original.remove(key) {
            event["data"][key] = value;
            if serde_json::to_vec(&event)
                .expect("Diagnostic projection")
                .len()
                > 6000
            {
                event["data"].as_object_mut().unwrap().remove(key);
            }
        }
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_large_private_record_advances_the_cursor_and_retains_exact_matching_fields() {
        let mut history = Diagnostics::default();
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let request = Uuid::new_v4();
        let group = Uuid::new_v4();
        history.push(context,None,DiagnosticKind::RequestFailed,json!({"requestId":request,"url":"https://example.com/acme","method":"POST","status":409,"text":"\0".repeat(2048),"stackAvailable":true,"stack":{"text":"Acme private stack"},"arguments":{"groupId":group,"retained":true,"count":1}}),0);
        let default = history.read(context, &Query::default()).unwrap();
        assert!(!default.to_string().contains("Acme private stack"));
        let read = history
            .read(
                context,
                &Query {
                    include_text: true,
                    include_stack: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(read["events"].as_array().unwrap().len(), 1);
        assert_eq!(read["nextCursor"], 1);
        assert_eq!(read["hasMore"], false);
        assert_eq!(read["events"][0]["data"]["requestId"], json!(request));
        assert_eq!(read["events"][0]["data"]["method"], "POST");
        assert_eq!(read["events"][0]["data"]["status"], 409);
        assert_eq!(
            read["events"][0]["data"]["arguments"]["groupId"],
            json!(group)
        );
        assert_eq!(read["events"][0]["data"]["detailsTruncatedForRead"], true);
    }

    #[test]
    fn large_console_projection_preserves_unproven_request_candidates_and_cursor_progress() {
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let request = Uuid::new_v4();
        let mut history = Diagnostics::default();
        history.push(context,None,DiagnosticKind::Response,json!({"requestId":request,"url":"https://example.com/api?token=Acme-private-token","method":"POST","status":409}),1000);
        history.push(context,None,DiagnosticKind::Console,json!({"level":"error","text":"\0".repeat(2048),"location":{"url":"https://example.com/api"}}),1001);
        let read = history
            .read(
                context,
                &Query {
                    sequence: Some(2),
                    include_text: true,
                    correlation: Some(Correlation::default()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(read["nextCursor"], 2);
        assert_eq!(read["events"][0]["data"]["detailsTruncatedForRead"], true);
        assert_eq!(
            read["events"][0]["data"]["requestCorrelation"]["candidates"][0]["requestId"],
            json!(request)
        );
        assert_eq!(
            read["events"][0]["data"]["requestCorrelation"]["confidence"],
            "candidate-only"
        );
        assert!(serde_json::to_vec(&read).unwrap().len() < 7000);
        assert!(!read.to_string().contains("Acme-private-token"));
    }
}
