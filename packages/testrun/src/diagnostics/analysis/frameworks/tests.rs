use super::*;
fn sample(runtime: &str) -> Value {
    json!({"schemaVersion":1,"runtime":runtime,"buildId":"build-a","symbolsBuildId":"build-a","architecture":"arm64","records":[{"name":"Acme shared operation","category":"cpu","identity":"operation-1","frames":["shared.Acme.load(Acme.kt:12)"],"value":12.0,"unit":"milliseconds","definition":"exported selected operation duration","actionId":"action-1"}]})
}
fn native() -> Value {
    json!({"schemaVersion":1,"appId":"com.example.acme","kind":"state","data":{"status":"ok","fields":{},"metrics":[]}})
}
fn bundle(framework: &str, runtime: &str) -> Value {
    json!({"schemaVersion":1,"framework":framework,"frameworkVersion":"fixture-1","target":{"id":"android:Acme","appId":"com.example.acme","class":"phone","os":"android","transport":"import"},"build":{"id":"build-a","architecture":"arm64","mode":"release","aot":true,"trimmed":true},"components":[{"id":"runtime","role":"runtime","processGeneration":"boot-1:42:birth-1","runtimeGeneration":"runtime-1","buildId":"build-a","architecture":"arm64","format":"runtime-samples-v1","payload":sample(runtime)},{"id":"native","role":"native","processGeneration":"boot-1:42:birth-1","runtimeGeneration":null,"buildId":"build-a","architecture":"arm64","format":"android-diagnostics-v1","payload":native()}],"links":[{"id":"action-1","kind":"bridge","from":{"componentId":"runtime","ordinal":0},"to":{"componentId":"native","ordinal":0}}]})
}
fn parse(v: &Value) -> Result<Index> {
    decode::decode("framework-evidence-v1", &serde_json::to_vec(v).unwrap())
}
#[test]
fn diagnostic_frameworks_composes_runtime_native_pairs_without_family_only_authority() {
    for (family, runtime) in [
        ("kotlin_multiplatform", "kotlin"),
        ("compose_multiplatform", "kotlin"),
        ("dotnet_maui", "dotnet"),
        ("unity", "unity"),
        ("unreal", "unreal"),
        ("custom_engine", "custom"),
        ("capacitor", "javascript"),
        ("cordova", "javascript"),
        ("ionic", "javascript"),
        ("electron", "javascript"),
        ("cef", "javascript"),
        ("tauri", "javascript"),
        ("webview2", "javascript"),
        ("hybrid", "javascript"),
        ("wkwebview", "webkit"),
    ] {
        let index = parse(&bundle(family, runtime)).unwrap();
        assert_eq!(index.rows.len(), 7);
        assert_eq!(
            index.rows[1].data["frames"][0],
            "shared.Acme.load(Acme.kt:12)"
        );
        assert_eq!(
            index.rows[1].data["frameworkEvidence"]["build"]["aot"],
            true
        );
        assert_eq!(index.rows[6].data["witnessOrdinals"], json!([1, 3]));
        assert_eq!(index.rows[6].correlations, vec!["action-1"]);
        assert_eq!(
            index.rows[1].correlations[0],
            "c0:runtime-object:operation-1"
        );
        assert!(!index.unsupported.is_empty());
    }
}
#[test]
fn diagnostic_frameworks_refuses_identity_spoofing_recursive_formats_and_bad_links() {
    let mut cases = vec![];
    let mut p = bundle("dotnet_maui", "dotnet");
    p["components"][0]["buildId"] = json!("other");
    cases.push(p);
    let mut p = bundle("dotnet_maui", "dotnet");
    p["components"][0]["payload"]["buildId"] = json!("other");
    cases.push(p);
    let mut p = bundle("dotnet_maui", "kotlin");
    cases.push(p.take());
    let mut p = bundle("unity", "unity");
    p["components"][0]["format"] = json!("framework-evidence-v1");
    cases.push(p);
    let mut p = bundle("unity", "unity");
    p["links"][0]["to"]["ordinal"] = json!(999);
    cases.push(p);
    let mut p = bundle("unity", "unity");
    p["components"][1]["payload"]["appId"] = json!("com.example.other");
    cases.push(p);
    let mut p = bundle("unity", "unity");
    p["components"][0]["payload"]["symbolsBuildId"] = json!("other");
    cases.push(p);
    let mut p = bundle("unity", "unity");
    p["components"][0]["path"] = json!("/must-not-open");
    cases.push(p);
    for p in cases {
        assert!(parse(&p).is_err(), "{p}");
    }
}
#[test]
fn diagnostic_frameworks_classes_require_transport_facts_and_never_grant_automation() {
    for class in ["tv", "watch", "automotive", "spatial", "extension"] {
        let mut p = bundle("custom_engine", "custom");
        p["target"]["class"] = json!(class);
        let index = parse(&p).unwrap();
        assert_eq!(index.rows[0].data["target"]["transport"], "import");
        p["target"]["transport"] = json!("apple");
        assert!(parse(&p).is_err());
    }
}
#[test]
fn diagnostic_frameworks_coroutine_recomposition_engine_metrics_and_missing_symbols_stay_distinct()
{
    let mut p = sample("kotlin");
    p["records"][0]["category"] = json!("coroutine");
    assert!(decode::decode("runtime-samples-v1", &serde_json::to_vec(&p).unwrap()).is_err());
    p["records"][0]["state"] = json!("suspended");
    p["symbolsBuildId"] = Value::Null;
    let index = decode::decode("runtime-samples-v1", &serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(index.rows[0].data["state"], "suspended");
    assert_eq!(index.rows[0].data["symbolication"], "unresolved");
    for category in [
        "recomposition",
        "gc",
        "scripting",
        "asset",
        "native",
        "cpu",
        "gpu",
        "bridge",
        "navigation",
        "managed_exception",
    ] {
        p["records"][0]["category"] = json!(category);
        let index = decode::decode("runtime-samples-v1", &serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(index.rows[0].value, Some(12.0));
        assert_eq!(
            index.rows[0].kind,
            if category == "managed_exception" {
                "failure"
            } else {
                "stack"
            }
        );
    }
}
#[test]
fn diagnostic_frameworks_dotnet_evented_speedscope_has_exclusive_intervals_and_separate_clocks() {
    let profile = json!({"$schema":"https://www.speedscope.app/file-format-schema.json","shared":{"frames":[{"name":"Acme.Main"},{"name":"Acme.Load"}]},"profiles":[{"type":"evented","name":"managed thread","unit":"milliseconds","startValue":0,"endValue":10,"events":[{"type":"O","at":0,"frame":0},{"type":"O","at":2,"frame":1},{"type":"C","at":7,"frame":1},{"type":"C","at":10,"frame":0}]}]});
    let mut p = bundle("dotnet_maui", "dotnet");
    p["components"][0]["format"] = json!("speedscope");
    p["components"][0]["payload"] = profile.clone();
    let index = parse(&p).unwrap();
    assert_eq!(index.rows[1].value, Some(2.0));
    assert_eq!(index.rows[2].value, Some(5.0));
    assert_eq!(index.rows[3].value, Some(3.0));
    assert_eq!(index.rows[2].data["frames"].as_array().unwrap().len(), 2);
    assert_eq!(index.clocks[0].id, "c0:speedscope:0");
    assert_eq!(index.clocks[1].id, "c1:android-epoch");
    let rows = index.rows.iter().enumerate().collect::<Vec<_>>();
    let tree = super::super::native_trace::stacks(&rows, "flamegraph", 0, 100).unwrap();
    assert_eq!(tree["matched"], 2);
    assert_eq!(
        tree["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["value"].as_f64().unwrap())
            .sum::<f64>(),
        10.0
    );
    for events in [
        json!([{"type":"C","at":1,"frame":0}]),
        json!([{"type":"O","at":0,"frame":0}]),
        json!([{"type":"O","at":11,"frame":0}]),
    ] {
        let mut v = profile.clone();
        v["profiles"][0]["events"] = events;
        assert!(decode::decode("speedscope", &serde_json::to_vec(&v).unwrap()).is_err());
    }
}
