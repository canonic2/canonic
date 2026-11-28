use super::*;
fn parse(format: &str, v: Value) -> Result<Index> {
    decode::decode(format, &serde_json::to_vec(&v).unwrap())
}
fn span() -> Value {
    json!({"traceId":"11111111111111111111111111111111","spanId":"2222222222222222","parentSpanId":"3333333333333333","name":"checkout","startTimeUnixNano":"1000000000","endTimeUnixNano":"1000000100","status":{"code":2}})
}
#[test]
fn diagnostic_observability_otlp_preserves_resources_and_joins_exact_ids() {
    let s = span();
    let id = s["traceId"].clone();
    let v = json!({"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"api"}}]},"scopeSpans":[{"spans":[s]}]}],"resourceLogs":[{"scopeLogs":[{"logRecords":[{"traceId":id,"severityNumber":17,"body":{"stringValue":"failed"}}]}]}],"resourceMetrics":[{"scopeMetrics":[{"metrics":[{"name":"requests","unit":"1","sum":{"aggregationTemporality":2,"isMonotonic":true,"dataPoints":[{"asInt":"4","exemplars":[{"traceId":id}]}]}}]}]}]});
    let i = parse("otlp-json", v).unwrap();
    assert_eq!(i.rows[0].value, Some(100.0));
    assert_eq!(i.rows[0].data["serviceName"], "api");
    let rows = i.rows.iter().enumerate().collect::<Vec<_>>();
    let j = joins(&rows, 0, 10).unwrap();
    assert_eq!(j["traces"][0]["spanObservations"], 1);
    assert_eq!(j["traces"][0]["metricExemplarObservations"], 1);
    assert_eq!(
        j["traces"][0]["missingParents"][0]["parentSpanId"],
        "3333333333333333"
    );
    assert_eq!(j["causalityEstablished"], false);
}
#[test]
fn diagnostic_observability_rejects_bad_ids_and_ambiguous_anyvalues() {
    let mut s = span();
    s["traceId"] = json!("bad");
    assert!(
        parse(
            "otlp-json",
            json!({"resourceSpans":[{"scopeSpans":[{"spans":[s]}]}]})
        )
        .is_err()
    );
    assert!(parse("otlp-json",json!({"resourceLogs":[{"scopeLogs":[{"logRecords":[{"body":{"stringValue":"x","intValue":"1"}}]}]}]})).is_err());
}
#[test]
fn diagnostic_observability_multiservice_duplicates_and_inexact_metrics_stay_distinct() {
    let mut server = span();
    server["spanId"] = json!("3333333333333333");
    server["parentSpanId"] = Value::Null;
    server["status"] = json!({"code":1});
    let resources=[("client",span()),("api",server)].into_iter().map(|(service,s)|json!({"resource":{"attributes":[{"key":"service.name","value":{"stringValue":service}},{"key":"service.version","value":{"stringValue":format!("{service}-v1")}}]},"scopeSpans":[{"spans":[s.clone(),s]}]})).collect::<Vec<_>>();
    let i=parse("otlp-json",json!({"resourceSpans":resources,"resourceMetrics":[{"scopeMetrics":[{"metrics":[{"name":"large","sum":{"dataPoints":[{"asInt":"9007199254740993"}]}},{"name":"histogram","unit":"ms","histogram":{"aggregationTemporality":2,"dataPoints":[{"count":"2","bucketCounts":["1","1"],"explicitBounds":[5]}]}}]}]}]})).unwrap();
    let rows = i.rows.iter().enumerate().collect::<Vec<_>>();
    let j = joins(&rows, 0, 10).unwrap();
    assert_eq!(j["traces"][0]["distinctSpanIds"], 2);
    assert_eq!(j["traces"][0]["duplicateSpanIds"], true);
    assert_eq!(j["traces"][0]["missingParents"], json!([]));
    assert!(
        i.rows
            .iter()
            .any(|r| r.correlations.contains(&"release:client:client-v1".into()))
    );
    assert!(
        i.rows
            .iter()
            .any(|r| r.correlations.contains(&"release:api:api-v1".into()))
    );
    let large = i.rows.iter().find(|r| r.name == "large").unwrap();
    assert_eq!(large.value, None);
    assert_eq!(large.data["point"]["asInt"], "9007199254740993");
    let h = i.rows.iter().find(|r| r.name == "histogram").unwrap();
    assert_eq!(h.value, None);
    assert_eq!(h.data["point"]["bucketCounts"], json!(["1", "1"]));
}
#[test]
fn diagnostic_observability_provider_shapes_keep_native_values_and_partial_gaps() {
    for (format, v, count) in [
        (
            "prometheus-json",
            json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":{"__name__":"count"},"values":[[1,"NaN"],[2,"3"]]}]}}),
            2,
        ),
        (
            "loki-json",
            json!({"status":"success","data":{"resultType":"streams","result":[{"stream":{"service":"api"},"values":[["1000000000","message"]]}]}}),
            1,
        ),
        (
            "sentry-json",
            json!({"eventID":"event","level":"error","dateCreated":"2026-01-01T00:00:00Z"}),
            1,
        ),
        (
            "cloudwatch-json",
            json!({"results":[[{"field":"@message","value":"x"}]],"status":"Running","nextToken":"more"}),
            1,
        ),
        (
            "crashlytics-json",
            json!([{"event_id":"event","event_type":"FATAL"}]),
            1,
        ),
        (
            "play-vitals-json",
            json!({"rows":[{"startTime":{"timeZone":{"id":"America/Los_Angeles"}},"metrics":[{"metric":"crashRate","decimalValue":{"value":"0.1"}}]}]}),
            1,
        ),
        (
            "jaeger-json",
            json!({"data":[{"traceID":"11111111111111111111111111111111","spans":[{"traceID":"11111111111111111111111111111111","spanID":"2222222222222222","operationName":"api","startTime":1000000,"duration":5}]}]}),
            1,
        ),
        (
            "deployment-evidence-v1",
            json!({"schemaVersion":1,"records":[{"kind":"deployment","id":"d1","resource":"api","release":"v1","time":"2026-01-01T00:00:00Z","imageId":"sha256:declared"}]}),
            1,
        ),
    ] {
        let i = parse(format, v).unwrap();
        assert_eq!(i.rows.len(), count, "{format}");
        assert!(!i.unsupported.is_empty());
    }
}
#[test]
fn diagnostic_observability_envelope_fences_provider_budgets_and_sanitizes_links() {
    let mut e = json!({"schemaVersion":1,"provider":"otlp","sourceId":"incident","scope":{"account":"acme","project":"api","environment":"staging","region":"eu","resource":"api","release":"v1"},"window":{"from":"2026-01-01T00:00:00Z","to":"2026-01-01T00:01:00Z"},"authorizationOwner":"external","request":{"method":"export"},"format":"otlp-json","pages":[{"payload":{"resourceSpans":[]}}],"coverage":"partial","limits":{"maxRecords":10,"maxBytes":1024,"timeoutMs":1000},"scan":{"bytes":100,"maxBytes":200,"cost":null,"meaning":"reported"},"links":["https://example.com/incident?token=secret#private"]});
    let i = parse("observability-evidence-v1", e.clone()).unwrap();
    assert_eq!(
        i.rows[0].data["providerLinks"][0],
        "https://example.com/incident"
    );
    e["scan"]["maxBytes"] = json!(1);
    assert!(parse("observability-evidence-v1", e.clone()).is_err());
    e["provider"] = json!("unknown");
    assert!(parse("observability-evidence-v1", e).is_err());
}
