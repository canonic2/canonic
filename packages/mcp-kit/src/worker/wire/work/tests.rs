//! Wire-size admission stays exact, bounded and independent of owned projections.
use super::*;
use crate::{
    json,
    task::{TaskId, TaskRequest},
    worker::wire::{Work, request_bytes},
};
use serde_json::json;

#[test]
fn borrowed_work_matches_owned_wire_for_every_supported_request() {
    let mut cases = vec![
        (
            Request::Tool(crate::ToolCall {
                name: "acme_\"probe".into(),
                arguments: json!({"nested":["é", "\n"]}),
                progress_token: Some(json!("token")),
            }),
            json!({"kind":"tool", "params":{"name":"acme_\"probe", "arguments":{"nested":["é", "\n"]}, "_meta":{"progressToken":"token"}}}),
        ),
        (
            Request::Prompt(crate::prompt::PromptCall {
                name: "acme_prompt".into(),
                arguments: [("key".into(), "value".into())].into(),
                progress_token: Some(json!(7)),
            }),
            json!({"kind":"prompt", "params":{"name":"acme_prompt", "arguments":{"key":"value"}, "_meta":{"progressToken":7}}}),
        ),
        (
            Request::Completion(crate::CompleteRequest {
                reference: crate::completion::CompletionReference::Prompt {
                    name: "acme_prompt".into(),
                    title: None,
                },
                argument: crate::completion::CompletionArgument {
                    name: "key".into(),
                    value: "v".into(),
                },
                context: [("other".into(), "known".into())].into(),
                progress_token: None,
            }),
            json!({"kind":"completion", "params":{"ref":{"type":"ref/prompt", "name":"acme_prompt"},
            "argument":{"name":"key", "value":"v"}, "context":{"arguments":{"other":"known"}}}}),
        ),
        (
            Request::Method {
                method: "acme/read".into(),
                params: json!({"value":[1,2,3]}),
            },
            json!({"kind":"method", "params":{"method":"acme/read", "params":{"value":[1,2,3]}}}),
        ),
        (
            Request::Notification {
                method: "acme/changed".into(),
                params: json!({}),
            },
            json!({"kind":"notification", "params":{"method":"acme/changed", "params":{}}}),
        ),
    ];
    let task_id = TaskId::new("acme-task").unwrap();
    cases.extend([
        (Request::Task(TaskRequest::Get { task_id:task_id.clone() }),
            json!({"kind":"tasks/get", "params":{"taskId":"acme-task"}})),
        (Request::Task(TaskRequest::Cancel { task_id:task_id.clone() }),
            json!({"kind":"tasks/cancel", "params":{"taskId":"acme-task"}})),
        (Request::Task(TaskRequest::Update { task_id,
            responses:crate::task::InputResponses::new(json!({"step":{"action":"accept"}})).unwrap() }),
            json!({"kind":"tasks/update", "params":{"taskId":"acme-task", "inputResponses":{"step":{"action":"accept"}}}})),
    ]);
    for (request, expected) in cases {
        let owned = Work::new(&request).unwrap();
        assert_eq!(serde_json::to_value(&owned).unwrap(), expected);
        let bytes = json::encode(&owned, 65536).unwrap();
        crate::test_work::reset();
        assert_eq!(request_bytes(&request, bytes.len()).unwrap(), bytes.len());
        let counts = crate::test_work::counts();
        assert_eq!(counts.measurements, 1);
        assert_eq!(counts.encodings, 0);
        assert_eq!(counts.preparations, 0);
        assert_eq!(
            request_bytes(&request, bytes.len() - 1),
            Err(json::EncodeError::TooLarge {
                limit: bytes.len() - 1
            })
        );
    }
}

#[test]
fn oversized_input_stops_measurement_at_the_budget() {
    let request = Request::Method {
        method: "acme/read".into(),
        params: json!({"unrelated":vec!["x"; 10000]}),
    };
    crate::test_work::reset();
    assert_eq!(
        request_bytes(&request, 128),
        Err(json::EncodeError::TooLarge { limit: 128 })
    );
    assert!(crate::test_work::counts().measured_bytes <= 128);
}
