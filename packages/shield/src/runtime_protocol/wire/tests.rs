use super::{Envelope, Error, MAX_INPUT, Operation};
use serde_json::{Value, json};

fn request() -> Value {
    json!({"protocol_version":1,"correlation_id":"callback:1","operation":"runtime.evaluate",
        "payload":{"input":{"kind":"file","action":"read","sources":["/project/note.txt"],
            "destination":null,"recursive":false,"original_input":{"file_path":"/project/note.txt"}},
            "context":{"cwd":"/project","environment":null},
            "provenance":{"session_id":"session:1","tool_use_id":"tool:1","event":"PreToolUse"},"retry_ref":null}})
}

fn refused(value: &Value) {
    assert!(matches!(
        Envelope::parse(&serde_json::to_vec(value).unwrap()),
        Err(Error::InvalidInput)
    ));
}

#[test]
fn closed_envelope_preserves_input_and_cannot_appoint_authority() {
    let value = request();
    let parsed = Envelope::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(parsed.correlation_id, "callback:1");
    let Operation::Evaluate(call) = parsed.operation else {
        panic!("expected evaluation")
    };
    assert!(call.retry_ref.is_none());
    assert_eq!(call.context.cwd, "/project");
    for key in [
        "project_id",
        "consumer_id",
        "activation_id",
        "fallback",
        "approval_id",
    ] {
        let mut forged = request();
        forged["payload"][key] = json!("caller-appointed");
        refused(&forged);
    }
    let mut omitted = request();
    omitted["payload"]["context"]
        .as_object_mut()
        .unwrap()
        .remove("environment");
    refused(&omitted);
    omitted["payload"]["context"]["environment"] = json!({"PATH":"caller ambient"});
    refused(&omitted);
}

#[test]
fn decoded_duplicates_nuls_versions_depth_and_size_are_checked() {
    let source = request().to_string();
    let duplicate = source.replacen(
        "\"protocol_version\":1",
        "\"protocol_version\":1,\"protocol_versi\\u006fn\":1",
        1,
    );
    assert!(matches!(
        Envelope::parse(duplicate.as_bytes()),
        Err(Error::InvalidInput)
    ));
    let mut value = request();
    value["protocol_version"] = json!(2);
    assert!(matches!(
        Envelope::parse(value.to_string().as_bytes()),
        Err(Error::UnsupportedVersion)
    ));
    value["protocol_version"] = json!("1");
    refused(&value);
    value = request();
    value["payload"]["input"]["original_input"]["escaped"] = json!("nul\0");
    refused(&value);
    let mut nested = json!(null);
    for _ in 0..64 {
        nested = json!([nested]);
    }
    value = request();
    value["payload"]["input"]["original_input"]["nested"] = nested;
    refused(&value);
    assert!(matches!(
        Envelope::parse(&vec![b' '; MAX_INPUT + 1]),
        Err(Error::InvalidInput)
    ));
}

#[test]
fn consumption_requires_private_reference_and_observation_has_no_authority_fields() {
    let mut value = request();
    value["operation"] = json!("runtime.consume");
    value["payload"]["request_id"] = json!("request:1");
    refused(&value);
    value["payload"]["retry_ref"] = json!("private-reference");
    assert!(matches!(
        Envelope::parse(value.to_string().as_bytes())
            .unwrap()
            .operation,
        Operation::Consume(_)
    ));
    value["operation"] = json!("history.observe");
    value["payload"]
        .as_object_mut()
        .unwrap()
        .remove("retry_ref");
    value["payload"]["delivery_id"] = json!("delivery:1");
    value["payload"]["observation"] = json!({"kind":"completed","occurred_at":null,"run_id":null,
        "exit_code":0,"signal":null,"host_approval":null});
    assert!(matches!(
        Envelope::parse(value.to_string().as_bytes())
            .unwrap()
            .operation,
        Operation::Observe(_)
    ));
    value["payload"]["observation"]["output"] = json!("never retained");
    refused(&value);
}
