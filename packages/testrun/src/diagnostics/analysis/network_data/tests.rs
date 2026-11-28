//! Independent network, packet and scalar snapshot fixtures; no external service required.
use super::*;
fn decoded(format: &str, v: Value) -> Index {
    super::super::decode::decode(format, &serde_json::to_vec(&v).unwrap()).unwrap()
}
fn query_for(index: &Index, v: Value) -> Value {
    query(index, &serde_json::from_value(v).unwrap(), 0, 100).unwrap()
}
fn identity(scope: &str) -> Value {
    json!({"scope":scope,"requestId":"request1","traceId":"11111111111111111111111111111111","spanId":"2222222222222222"})
}
fn network(records: Value) -> Value {
    json!({"schemaVersion":1,"format":"network-evidence-v1","source":"Acme app/service export","coverage":"selected requests; sampled backend spans","records":records})
}
fn request(id: &str, scope: &str) -> Value {
    json!({"type":"request","id":id,"identity":identity(scope),"protocol":"grpc","method":"Acme.Read","address":"https://api.example.com/Read","status":200,"grpcStatus":14,"cancelled":false,"cache":"unknown","phases":{"dns":null,"connect":0,"total":12000}})
}
#[test]
fn diagnostic_network_data_har_keeps_zero_unknown_and_nested_tls_without_phase_sum() {
    let v = json!({"log":{"version":"1.2","entries":[{"startedDateTime":"2026-11-08T00:00:00Z","time":20,"request":{"method":"GET","url":"https://example.com/a","headers":[{"name":"TraceParent","value":"00-11111111111111111111111111111111-2222222222222222-01"}]},"response":{"status":200},"timings":{"dns":-1,"connect":10,"ssl":6,"send":0,"wait":5,"receive":5}}]}});
    let index = decoded("har", v);
    let result = query_for(&index, json!({"operation":"phases"}));
    let row = &result["rows"][0]["observation"];
    assert_eq!(row["phasesMillis"]["dns"], Value::Null);
    assert_eq!(row["phasesMillis"]["send"], 0.0);
    assert_eq!(row["phasesMillis"]["connect"], 10.0);
    assert_eq!(row["phasesMillis"]["ssl"], 6.0);
    assert_eq!(
        row["identity"]["traceId"],
        "11111111111111111111111111111111"
    );
    assert_eq!(row["traceSampled"], true);
    assert!(row.get("phaseSum").is_none());
    assert!(traceparent("00-00000000000000000000000000000000-2222222222222222-00").is_none());
    assert!(traceparent("00-11111111111111111111111111111111-0000000000000000-01").is_none());
    assert!(traceparent("01-11111111111111111111111111111111-2222222222222222-01").is_none());
}
#[test]
fn diagnostic_network_data_exact_correlations_keep_scope_sampling_and_protocol_messages() {
    let index = decoded(
        "network-evidence-v1",
        network(
            json!([request("r1","client"),{"type":"span","id":"span1","identity":identity("service"),"service":"Acme API","operation":"Read","durationMicros":8000,"sampled":true},{"type":"log","id":"log1","identity":identity("service"),"message":"upstream unavailable"},{"type":"message","id":"message1","request":"r1","direction":"received","sequence":0,"bytes":64,"encoding":"protobuf"}]),
        ),
    );
    let exact = query_for(
        &index,
        json!({"operation":"correlate","requestId":"request1","identityScope":"client"}),
    );
    assert_eq!(exact["matched"], 1);
    let trace = query_for(
        &index,
        json!({"operation":"correlate","traceId":"11111111111111111111111111111111"}),
    );
    assert_eq!(trace["matched"], 3);
    assert_eq!(
        query_for(&index, json!({"operation":"messages"}))["rows"][0]["observation"]["bytes"],
        64
    );
    assert!(
        query(
            &index,
            &serde_json::from_value(json!({"operation":"correlate","requestId":"request1"}))
                .unwrap(),
            0,
            100
        )
        .is_err()
    );
    assert_eq!(
        query_for(&index, json!({"operation":"requests"}))["rows"][0]["observation"]["grpcStatus"],
        14
    );
}
#[test]
fn diagnostic_network_data_bad_retries_missing_messages_and_unknown_controls_refuse() {
    let mut a = request("a", "client");
    a["retryOf"] = json!("b");
    let mut b = request("b", "client");
    b["retryOf"] = json!("a");
    for records in [
        json!([a, b]),
        json!([request("a", "client"), request("a", "client")]),
        json!([{"type":"message","id":"x","request":"missing","direction":"sent","sequence":1,"bytes":3,"encoding":"binary"}]),
    ] {
        assert!(
            super::super::decode::decode(
                "network-evidence-v1",
                &serde_json::to_vec(&network(records)).unwrap()
            )
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<Query>(
            json!({"operation":"requests","decrypt":true,"evaluate":"must-not-run"})
        )
        .is_err()
    );
}
fn snapshot() -> Value {
    json!({"schemaVersion":1,"format":"data-snapshot-v1","backend":"sqlite","dataset":"orders","scope":"Acme user fixture","account":"local","project":"Acme","role":"authoritative","snapshotId":"snapshot1","consistency":"atomic","sourceOwner":"app-owned read-only export","coverage":"selected fixture keys","collections":[{"name":"orders","indexes":["state"],"records":[{"key":"a","fields":{"state":null,"total":10}},{"key":"b","fields":{"total":20}},{"key":"c","fields":{"state":"ready","total":30}}]}]})
}
fn plan() -> Value {
    json!({"preset":"order-by-id","readOnly":true,"indexed":true,"indexName":"orders_primary","timeoutMs":2000,"maxRows":10,"maxScanned":20,"estimatedScanned":3,"actualScanned":3,"costUnit":"read_units","maxCost":10,"estimatedCost":1,"actualCost":1})
}
#[test]
fn diagnostic_network_data_materialized_index_distinguishes_null_missing_and_unindexed_predicates()
{
    let index = decoded("data-snapshot-v1", snapshot());
    let result = query_for(
        &index,
        json!({"operation":"data","collection":"orders","equals":{"field":"state","value":null}}),
    );
    assert_eq!(result["matched"], 1);
    assert_eq!(result["rows"][0]["record"]["key"], "a");
    assert_eq!(
        result["selection"]["method"],
        "materialized_equality_postings"
    );
    assert_eq!(
        query_for(
            &index,
            json!({"operation":"data","collection":"orders","key":"b"})
        )["rows"][0]["record"]["fields"]["total"],
        20
    );
    assert_eq!(query(&index,&serde_json::from_value(json!({"operation":"data","collection":"orders","equals":{"field":"total","value":20}})).unwrap(),0,100).unwrap_err().code,"DATA_INDEX_REQUIRED");
}
#[test]
fn diagnostic_network_data_remote_exports_require_readonly_named_indexed_bounded_plan() {
    let mut v = snapshot();
    v["backend"] = json!("remote_sql");
    assert_eq!(
        super::super::decode::decode("data-snapshot-v1", &serde_json::to_vec(&v).unwrap())
            .err()
            .unwrap()
            .code,
        "DATA_READ_PLAN_REQUIRED"
    );
    v["plan"] = plan();
    assert_eq!(
        query_for(
            &decoded("data-snapshot-v1", v.clone()),
            json!({"operation":"query_plan"})
        )["rows"][0]["schema"]["plan"]["preset"],
        "order-by-id"
    );
    for (field, value) in [
        ("readOnly", json!(false)),
        ("indexed", json!(false)),
        ("actualCost", json!(11)),
        ("estimatedScanned", json!(21)),
        ("timeoutMs", json!(10001)),
    ] {
        let mut bad = v.clone();
        bad["plan"][field] = value;
        assert_eq!(
            super::super::decode::decode("data-snapshot-v1", &serde_json::to_vec(&bad).unwrap())
                .err()
                .unwrap()
                .code,
            "DATA_READ_BUDGET"
        );
    }
    let mut command = v;
    command["plan"]["sql"] = json!("DELETE FROM orders");
    assert!(
        super::super::decode::decode("data-snapshot-v1", &serde_json::to_vec(&command).unwrap())
            .is_err()
    );
}
#[test]
fn diagnostic_network_data_browser_body_reuses_settled_private_json_and_never_replays_http() {
    let mut packet = json!({"schemaVersion":1,"targetId":"browser:11111111-1111-4111-8111-111111111111","engine":"chrome","request":{"operation":"http_evidence_read","format":"json"},"operation":{"id":"22222222-2222-4222-8222-222222222222","settled":true,"outcome":"succeeded","result":{"format":"json","value":snapshot()}}});
    let mut value = snapshot();
    value["backend"] = json!("remote_search");
    value["plan"] = plan();
    packet["operation"]["result"]["value"] = value;
    let index = decoded("browser-operation-v1", packet.clone());
    let result = query_for(
        &index,
        json!({"operation":"data","collection":"orders","key":"a"}),
    );
    assert_eq!(result["matched"], 1);
    assert!(
        result["rows"][0]["source"]
            .as_str()
            .unwrap()
            .starts_with("/operation/result/value/collections")
    );
    packet["request"]["operation"] = json!("http_fetch");
    let index = decoded("browser-operation-v1", packet);
    assert!(
        query(
            &index,
            &serde_json::from_value(json!({"operation":"schema"})).unwrap(),
            0,
            10
        )
        .is_err()
    );
}
fn receipt() -> super::super::model::Receipt {
    serde_json::from_value(json!({"version":1,"id":"analysis-a","collectorId":"collector-a","signature":"s","owner":"test","phase":"complete","rawSha256":"00","rawBytes":1,"format":"data-snapshot-v1","capturePhase":"complete","captureReason":null,"binding":{"targetId":"android:Acme","appId":"com.example.acme"},"metadata":{},"indexSha256":"00","reason":null})).unwrap()
}
#[test]
fn diagnostic_network_data_source_cache_differences_preserve_roles_consistency_and_no_repair() {
    let a = decoded("data-snapshot-v1", snapshot());
    let mut changed = snapshot();
    changed["role"] = json!("cache");
    changed["snapshotId"] = json!("snapshot2");
    changed["consistency"] = json!("best_effort");
    changed["collections"][0]["records"][0]["fields"]["state"] = json!("ready");
    let b = decoded("data-snapshot-v1", changed.clone());
    let result = compare(&receipt(), &a, &receipt(), &b);
    assert_eq!(result["changedRecords"], 1);
    assert_eq!(result["afterRole"], "cache");
    assert_eq!(result["repairPerformed"], false);
    changed["account"] = json!("neighbor");
    assert_eq!(
        compare(
            &receipt(),
            &a,
            &receipt(),
            &decoded("data-snapshot-v1", changed)
        )["compatible"],
        false
    );
}
fn pcap_bytes(little: bool, nanos: bool) -> Vec<u8> {
    let mut v = match (little, nanos) {
        (true, false) => vec![0xd4, 0xc3, 0xb2, 0xa1],
        (false, false) => vec![0xa1, 0xb2, 0xc3, 0xd4],
        (true, true) => vec![0x4d, 0x3c, 0xb2, 0xa1],
        (false, true) => vec![0xa1, 0xb2, 0x3c, 0x4d],
    };
    for n in [2u16, 4] {
        v.extend(if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        });
    }
    let word = |v: &mut Vec<u8>, n: u32| {
        v.extend(if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        })
    };
    for n in [0, 0, 65535, 101, 1, 100, 40, 40] {
        word(&mut v, n);
    }
    let mut ip = vec![0u8; 40];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&40u16.to_be_bytes());
    ip[9] = 6;
    ip[12..16].copy_from_slice(&[192, 0, 2, 1]);
    ip[16..20].copy_from_slice(&[198, 51, 100, 2]);
    ip[20..22].copy_from_slice(&1234u16.to_be_bytes());
    ip[22..24].copy_from_slice(&443u16.to_be_bytes());
    ip[32] = 0x50;
    ip[33] = 2;
    v.extend(ip);
    v
}
#[test]
fn diagnostic_network_data_pcap_both_endian_and_precision_keep_headers_without_tls_guesses() {
    for little in [true, false] {
        for nanos in [true, false] {
            let bytes = pcap_bytes(little, nanos);
            let index = super::super::decode::decode("pcap", &bytes).unwrap();
            let result = query_for(&index, json!({"operation":"packets"}));
            let p = &result["rows"][0]["observation"];
            assert_eq!(p["source"], "192.0.2.1");
            assert_eq!(p["destinationPort"], 443);
            assert_eq!(p["transport"], "tcp");
            assert!(p.get("tls").is_none());
            assert_eq!(result["rows"][0]["source"], "binary:offset:24");
            assert!(super::super::decode::decode("pcap", &bytes[..bytes.len() - 1]).is_err());
        }
    }
    let mut fragmented = pcap_bytes(true, false);
    fragmented[40 + 6] = 0x20;
    let index = super::super::decode::decode("pcap", &fragmented).unwrap();
    assert!(index.rows[0].data["networkData"]["transportUnavailable"].is_string());
    let mut ipv6 = pcap_bytes(true, false);
    ipv6.truncate(40);
    ipv6[32..36].copy_from_slice(&48u32.to_le_bytes());
    ipv6[36..40].copy_from_slice(&48u32.to_le_bytes());
    let mut ip = vec![0u8; 48];
    ip[0] = 0x60;
    ip[4..6].copy_from_slice(&8u16.to_be_bytes());
    ip[6] = 17;
    ip[23] = 1;
    ip[39] = 2;
    ip[40..42].copy_from_slice(&53u16.to_be_bytes());
    ip[42..44].copy_from_slice(&1234u16.to_be_bytes());
    ip[44..46].copy_from_slice(&8u16.to_be_bytes());
    ipv6.extend(ip);
    let index = super::super::decode::decode("pcap", &ipv6).unwrap();
    assert_eq!(index.rows[0].data["networkData"]["source"], "::1");
    assert_eq!(index.rows[0].data["networkData"]["transport"], "udp");
}
#[test]
fn diagnostic_network_data_posting_chunks_preserve_all_matches_and_public_page_budget() {
    let mut v = snapshot();
    v["collections"][0]["records"]=json!((0..600).map(|i|json!({"key":format!("k{i}"),"fields":{"state":"ready","body":"x".repeat(2048)}})).collect::<Vec<_>>());
    let index = decoded("data-snapshot-v1", v);
    let q:Query=serde_json::from_value(json!({"operation":"data","collection":"orders","equals":{"field":"state","value":"ready"}})).unwrap();
    let mut keys = vec![];
    for offset in [0, 128, 256, 384, 512] {
        let page = query(&index, &q, offset, 128).unwrap();
        assert_eq!(page["matched"], 600);
        keys.extend(
            page["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["record"]["key"].as_str().unwrap().to_string()),
        );
    }
    assert_eq!(keys.len(), 600);
    assert_eq!(keys.iter().collect::<BTreeSet<_>>().len(), 600);
    let options = |limit| {
        serde_json::from_value::<super::super::query::Query>(json!({"investigationId":"diag-a","analysisId":"analysis-a","view":"network_data","networkData":{"operation":"data"},"limit":limit})).unwrap()
    };
    assert!(super::super::query::query(&receipt(), &index, &options(1)).is_ok());
    assert_eq!(
        super::super::query::query(&receipt(), &index, &options(200))
            .unwrap_err()
            .code,
        "invalid_arguments"
    );
}
#[test]
fn diagnostic_network_data_runtime_epoch_and_schema_keep_private_access_boundaries() {
    let mut index = decoded("network-evidence-v1", network(json!([])));
    for generation in ["before", "after"] {
        let r = super::super::decode::row(
            "event",
            "request",
            "/runtime".into(),
            json!({"targetId":"android:Acme","pageId":"one","runtimeGeneration":generation,"observation":{"event":{"method":"Network.loadingFailed","params":{"requestId":"same","canceled":true,"timestamp":1}}}}),
        );
        index.rows.push(r);
    }
    runtime_rows(&mut index).unwrap();
    let result = query_for(&index, json!({"operation":"requests"}));
    assert_eq!(result["matched"], 2);
    assert_ne!(
        result["rows"][0]["observation"]["identity"]["scope"],
        result["rows"][1]["observation"]["identity"]["scope"]
    );
    let schema = &crate::diagnostics::surface::definition("diagnostics_query").unwrap()["inputSchema"]
        ["properties"]["networkData"];
    assert_eq!(schema["additionalProperties"], false);
}
