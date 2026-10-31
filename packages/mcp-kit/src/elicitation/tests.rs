use super::*;

fn form(schema: Value) -> Request {
    Request::form("Choose Acme report settings", Form::new(schema).unwrap()).unwrap()
}

fn report() -> Request {
    form(json!({"type":"object","properties":{
        "format":{"type":"string","oneOf":[{"const":"md","title":"Markdown"},{"const":"json","title":"JSON"}],"default":"md"},
        "sections":{"type":"array","items":{"type":"string","enum":["summary","details"]},"minItems":1,"maxItems":2},
        "count":{"type":"integer","minimum":1,"maximum":10},
        "note":{"type":"string","minLength":1,"maxLength":2},
        "enabled":{"type":"boolean","default":false}
    },"required":["format","sections","count","note"]}))
}

#[test]
fn submitted_values_are_exact_defaults_are_not_answers() {
    let question = report();
    let content = json!({"format":"md","sections":["details","summary"],"count":10,"note":"é🦀"});
    let response = question
        .answer(&json!({"action":"accept","content":content,"_meta":{"acme/tag":true}}))
        .unwrap();
    assert_eq!(response.action, Action::Accept);
    assert_eq!(response.content, Some(content.clone()));
    assert_eq!(response.meta, Some(json!({"acme/tag":true})));
    assert!(response.content.unwrap().get("enabled").is_none());
    for bad in [
        json!({}),
        json!({"format":"md","sections":["summary"],"count":"1","note":"a"}),
        json!({"format":"pdf","sections":["summary"],"count":1,"note":"a"}),
        json!({"format":"md","sections":["summary","summary"],"count":1,"note":"a"}),
        json!({"format":"md","sections":[],"count":1,"note":"a"}),
        json!({"format":"md","sections":["summary"],"count":11,"note":"a"}),
        json!({"format":"md","sections":["summary"],"count":1.5,"note":"a"}),
        json!({"format":"md","sections":["summary"],"count":1,"note":"abc"}),
        {
            let mut extra = content.clone();
            extra["authority"] = json!(true);
            extra
        },
    ] {
        assert!(
            question
                .answer(&json!({"action":"accept","content":bad}))
                .is_err()
        );
    }
    for action in ["decline", "cancel"] {
        let answer = question
            .answer(&json!({"action":action,"content":{"ignored":true}}))
            .unwrap();
        assert_eq!(answer.content, None);
    }
}

#[test]
fn invalid_definitions_and_defaults_never_publish_constraints() {
    for field in [
        json!({"type":"object"}),
        json!({"type":"array","items":{"type":"object"}}),
        json!({"type":"string","pattern":".*"}),
        json!({"type":"string","minLength":3,"maxLength":2}),
        json!({"type":"integer","minimum":2,"maximum":1}),
        json!({"type":"string","enum":["x","x"]}),
        json!({"type":"string","enum":["x"],"default":"y"}),
        json!({"type":"string","enum":["x"],"enumNames":[]}),
        json!({"type":"string","oneOf":[{"const":"x"}]}),
        json!({"type":"integer","default":9_007_199_254_740_992_u64}),
        json!({"type":"boolean","default":"false"}),
    ] {
        assert!(Form::new(json!({"type":"object","properties":{"field":field}})).is_err());
    }
    assert!(Form::new(json!({"type":"object","properties":{},"required":["unknown"]})).is_err());
}

#[test]
fn formats_validate_without_normalizing_or_fetching() {
    for (format, good, bad) in [
        ("email", "acme+test@example.com", "acme@-example.com"),
        ("date", "2024-02-29", "2025-02-29"),
        (
            "date-time",
            "2024-02-29T12:30:15.123+02:30",
            "2024-02-29T25:00:00Z",
        ),
        ("uri", "acme://reports/one", "relative/path"),
    ] {
        let question = form(
            json!({"type":"object","properties":{"field":{"type":"string","format":format}},"required":["field"]}),
        );
        assert!(
            question
                .answer(&json!({"action":"accept","content":{"field":good}}))
                .is_ok()
        );
        assert!(
            question
                .answer(&json!({"action":"accept","content":{"field":bad}}))
                .is_err()
        );
    }
    for url in [
        "http://example.com",
        "file:///report",
        "https://user@example.com",
        "https://example.com/#token",
        "https://:443",
        "https://example.com:999999",
    ] {
        assert!(Request::url("Connect", url, None).is_err());
    }
    let request = Request::url(
        "Connect Acme",
        "https://example.com/connect",
        Some("connection-1"),
    )
    .unwrap();
    assert!(
        request
            .answer(&json!({"action":"accept"}))
            .unwrap()
            .content
            .is_none()
    );
    assert!(
        request
            .answer(&json!({"action":"accept","content":{}}))
            .is_err()
    );
    assert!(
        request
            .legacy_params("2025-06-18", &json!({"elicitation":{"url":{}}}))
            .is_err()
    );
    assert_eq!(
        request
            .legacy_params("2025-11-25", &json!({"elicitation":{"url":{}}}))
            .unwrap()["elicitationId"],
        "connection-1"
    );
    assert!(request.as_value()["params"].get("elicitationId").is_none());
}

#[test]
fn questions_are_bound_to_their_exact_modes_and_schemas() {
    let round = Requests::new(vec![
        ("report".into(), report()),
        (
            "connect".into(),
            Request::url("Connect", "https://example.com/connect", None).unwrap(),
        ),
    ])
    .unwrap();
    assert!(
        round
            .require_capabilities(&json!({"elicitation":{}}))
            .is_err()
    );
    assert!(
        round
            .require_capabilities(&json!({"elicitation":{"form":{},"url":{}}}))
            .is_ok()
    );
    let answers = round
        .answers(&json!({"connect":{"action":"cancel"},"unknown":["opaque"]}))
        .unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers["connect"].action, Action::Cancel);
    assert!(
        round
            .answers(
                &json!({"connect":{"action":"accept"},"report":{"action":"accept","content":{}}})
            )
            .is_err()
    );
    assert_eq!(Requests::restore(&round.as_value()).unwrap(), round);
    let mut corrupt = round.as_value();
    corrupt["connect"]["params"]["extra"] = json!(true);
    assert!(Requests::restore(&corrupt).is_err());
    let empty = form(json!({"type":"object","properties":{}}));
    assert!(Requests::new(vec![("key".into(), empty.clone()), ("key".into(), empty)]).is_err());
}

#[test]
fn bounds_include_counts_escaped_bytes_and_combined_maps() {
    let empty = form(json!({"type":"object","properties":{}}));
    assert!(Requests::new((0..8).map(|i| (format!("q{i}"), empty.clone()))).is_ok());
    assert!(Requests::new((0..9).map(|i| (format!("q{i}"), empty.clone()))).is_err());
    assert!(Requests::new(vec![("k".repeat(128), empty.clone())]).is_ok());
    assert!(Requests::new(vec![("k".repeat(129), empty.clone())]).is_err());
    let properties = |count| {
        Value::Object(
            (0..count)
                .map(|i| (format!("f{i}"), json!({"type":"boolean"})))
                .collect(),
        )
    };
    assert!(Form::new(json!({"type":"object","properties":properties(64)})).is_ok());
    assert!(Form::new(json!({"type":"object","properties":properties(65)})).is_err());
    let choices = |count| json!({"type":"object","properties":{"choice":{"type":"string","enum":(0..count).map(|i| format!("c{i}")).collect::<Vec<_>>()}}});
    assert!(Form::new(choices(128)).is_ok());
    assert!(Form::new(choices(129)).is_err());
    assert!(
        Request::form(
            &"m".repeat(16384),
            Form::new(json!({"type":"object","properties":{}})).unwrap()
        )
        .is_ok()
    );
    assert!(
        Request::form(
            &"m".repeat(16385),
            Form::new(json!({"type":"object","properties":{}})).unwrap()
        )
        .is_err()
    );
    let large = Request::form(
        &"\"".repeat(16384),
        Form::new(json!({"type":"object","properties":{}})).unwrap(),
    )
    .unwrap();
    assert!(
        Requests::new(vec![
            ("first".into(), large.clone()),
            ("second".into(), large)
        ])
        .is_err()
    );
    let response = json!({"unknown":"x"});
    let overhead = serde_json::to_vec(&response).unwrap().len() - 1;
    assert!(
        empty
            .answer(&json!({"action":"accept","_meta":{"extra":"x".repeat(16384)}}))
            .is_err()
    );
    assert!(
        validate_responses(&json!({"unknown":"x".repeat(MAX_RESPONSE_BYTES - overhead)})).is_ok()
    );
    assert!(
        validate_responses(&json!({"unknown":"x".repeat(MAX_RESPONSE_BYTES - overhead + 1)}))
            .is_err()
    );
}

#[test]
fn legacy_conversion_preserves_labels_and_refuses_unrepresentable_fields() {
    let question = form(
        json!({"type":"object","properties":{"color":{"type":"string","oneOf":[{"const":"red","title":"Red"},{"const":"blue","title":"Blue"}]}}}),
    );
    let legacy = question
        .legacy_params("2025-06-18", &json!({"elicitation":{}}))
        .unwrap();
    assert!(legacy.get("mode").is_none());
    assert_eq!(
        legacy["requestedSchema"]["properties"]["color"]["enum"],
        json!(["red", "blue"])
    );
    assert_eq!(
        legacy["requestedSchema"]["properties"]["color"]["enumNames"],
        json!(["Red", "Blue"])
    );
    assert!(
        report()
            .legacy_params("2025-06-18", &json!({"elicitation":{}}))
            .is_err()
    );
    assert!(
        question
            .answer(&json!({"action":"accept","content":{"color":"red"},"_meta":null}))
            .is_err()
    );
    assert!(question.answer(&json!({"action":"other"})).is_err());
}

#[test]
fn metadata_url_and_aggregate_choice_budgets_are_enforced_before_publication() {
    let schema = |description: String| json!({"type":"object","properties":{"field":{"type":"boolean","description":description}}});
    assert!(Form::new(schema("x".repeat(4096))).is_ok());
    assert!(Form::new(schema("x".repeat(4097))).is_err());
    let choices = |fields| {
        Value::Object((0..fields).map(|index| (format!("field{index}"),json!({"type":"string","enum":(0..128).map(|n|format!("choice{n}")).collect::<Vec<_>>()}))).collect())
    };
    assert!(Form::new(json!({"type":"object","properties":choices(4)})).is_ok());
    assert!(Form::new(json!({"type":"object","properties":choices(5)})).is_err());
    let prefix = "https://example.com/";
    assert!(
        Request::url(
            "Connect",
            &format!("{prefix}{}", "x".repeat(4096 - prefix.len())),
            Some(&"k".repeat(128))
        )
        .is_ok()
    );
    assert!(
        Request::url(
            "Connect",
            &format!("{prefix}{}", "x".repeat(4097 - prefix.len())),
            None
        )
        .is_err()
    );
    assert!(Request::url("Connect", prefix, Some(&"k".repeat(129))).is_err());
    let nested = |count| (0..count).fold(Value::Null, |inner, _| json!({"nested":inner}));
    assert!(validate_responses(&json!({"unknown":nested(31)})).is_ok());
    assert!(validate_responses(&json!({"unknown":nested(32)})).is_err());
    let form = Form::new(
        json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"]}),
    )
    .unwrap();
    let question = Request::form("Count", form).unwrap();
    assert!(
        question
            .answer(&json!({"action":"accept","content":{"count":9_007_199_254_740_991_u64}}))
            .is_ok()
    );
    assert!(
        question
            .answer(&json!({"action":"accept","content":{"count":9_007_199_254_740_992_u64}}))
            .is_err()
    );
}
