//! Zero disables the HTTP-local budget, never the enclosing owner deadline.
use super::*;
use crate::http_fixtures::defaults as wire;

fn client(contexts: &mut Contexts, url: &str, timeout_ms: u64) -> Reference {
    let connection = contexts.connection.unwrap_or_else(Uuid::new_v4);
    serde_json::from_value(
        contexts
            .create(
                connection,
                Options {
                    base_url: Some(url.into()),
                    timeout_ms,
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap()
}

#[tokio::test]
async fn http_zero_timeout_defaults_and_overrides_wait_for_the_actual_response() {
    let site = wire::Site::start(None).await;
    let mut contexts = Contexts::default();
    for (context_timeout, request_timeout) in [(0, None), (1, Some(0))] {
        let context = client(&mut contexts, &site.url, context_timeout);
        let mut request = fetch("/delay/40");
        request.timeout_ms = request_timeout;
        let reply = body(&mut contexts, context, request).await;
        let record: Value = serde_json::from_slice(&reply).unwrap();
        assert_eq!(record["path"], "/delay/40");
        contexts.remove(context).unwrap();
    }
    assert_eq!(site.records().len(), 2);
    contexts.close().unwrap();
}

#[tokio::test]
async fn http_zero_timeout_preserves_finite_overrides_and_the_original_owner_deadline() {
    let site = wire::Site::start(None).await;
    let mut contexts = Contexts::default();
    for (request_timeout, owner_budget) in [
        (Some(1000), Duration::from_secs(5)),
        (Some(0), Duration::from_millis(1000)),
    ] {
        let context = client(&mut contexts, &site.url, 0);
        let mut request = fetch("/delay/1500");
        request.timeout_ms = request_timeout;
        let before = site.records().len();
        let id = contexts
            .start(context, request, Instant::now() + owner_budget)
            .unwrap();
        let error = match ready(&mut contexts, id).await {
            Err(error) => error,
            _ => panic!("Held response exceeded its finite deadline"),
        };
        assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
        assert_eq!(error.partial.unwrap()["requestMayHaveBeenSent"], true);
        assert_eq!(
            site.records().len(),
            before + 1,
            "One dispatch; no replay after timeout"
        );
        contexts.remove(context).unwrap();
    }
    contexts.close().unwrap();
}

#[test]
fn http_zero_timeout_schema_and_validation_keep_the_upper_bound() {
    let mut options = Options {
        timeout_ms: 0,
        ..Default::default()
    };
    options.validate().unwrap();
    options.timeout_ms = 120001;
    assert_eq!(options.validate().unwrap_err().code, "HTTP_OPTIONS_BOUNDS");
    let mut request = fetch("http://127.0.0.1:8081/Acme");
    request.timeout_ms = Some(0);
    request.validate().unwrap();
    request.timeout_ms = Some(120001);
    assert_eq!(request.validate().unwrap_err().code, "HTTP_OPTIONS_BOUNDS");
    for schema in schemas(&json!({"type":"object"})) {
        let operation = schema["properties"]["operation"]["const"].as_str().unwrap();
        let timeout = match operation {
            "http_context_create" => &schema["properties"]["options"]["properties"]["timeoutMs"],
            "http_fetch" => &schema["properties"]["request"]["properties"]["timeoutMs"],
            "route_fetch" => &schema["properties"]["timeoutMs"],
            _ => continue,
        };
        assert_eq!(timeout["minimum"], 0, "{operation}");
        assert_eq!(timeout["maximum"], 120000, "{operation}");
    }
}
