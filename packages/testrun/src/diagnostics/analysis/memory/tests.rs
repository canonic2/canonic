use super::*;
fn decode(format: &str, bytes: &[u8]) -> Index {
    super::super::decode::decode(format, bytes).unwrap()
}
fn record(tag: u8, data: Vec<u8>, out: &mut Vec<u8>) {
    out.push(tag);
    out.extend(0u32.to_be_bytes());
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(data);
}
fn put(out: &mut Vec<u8>, value: u64, width: usize) {
    out.extend(&value.to_be_bytes()[8 - width..]);
}
pub(crate) fn hprof(width: usize) -> Vec<u8> {
    let names = [
        (1, "java/lang/Object"),
        (2, "android/app/Activity"),
        (3, "com/example/MainActivity"),
        (4, "android/view/View"),
        (5, "java/lang/ref/Reference"),
        (6, "java/lang/ref/WeakReference"),
        (7, "com/example/Holder"),
        (8, "[Landroid/view/View;"),
        (20, "mDestroyed"),
        (21, "mView"),
        (22, "mContext"),
        (23, "referent"),
        (24, "activity"),
        (25, "weak"),
        (26, "array"),
        (27, "buffer"),
        (28, "app"),
        (29, "onCreate"),
        (30, "MainActivity.kt"),
    ];
    let mut out = b"JAVA PROFILE 1.0.3\0".to_vec();
    out.extend((width as u32).to_be_bytes());
    out.extend(1234u64.to_be_bytes());
    for (key, name) in names {
        let mut data = vec![];
        put(&mut data, key, width);
        data.extend(name.as_bytes());
        record(1, data, &mut out);
    }
    for i in 0..8 {
        let mut data = vec![];
        put(&mut data, 100 + i, 4);
        put(&mut data, 0x100 + i, width);
        put(&mut data, 0, 4);
        put(&mut data, i + 1, width);
        record(2, data, &mut out);
    }
    let mut frame = vec![];
    put(&mut frame, 0x500, width);
    put(&mut frame, 29, width);
    put(&mut frame, 0, width);
    put(&mut frame, 30, width);
    put(&mut frame, 102, 4);
    put(&mut frame, 10, 4);
    record(4, frame, &mut out);
    let mut trace = vec![];
    for n in [1, 0, 1] {
        put(&mut trace, n, 4);
    }
    put(&mut trace, 0x500, width);
    record(5, trace, &mut out);
    let mut heap = vec![0xfe];
    put(&mut heap, 0x11223344, 4);
    put(&mut heap, 28, width);
    append_hprof_classes(width, &mut heap);
    for (object, class, value, boolean) in [
        (0x200, 0x102, 0x201, true),
        (0x201, 0x103, 0x200, false),
        (0x202, 0x105, 0x203, false),
        (0x203, 0x103, 0, false),
    ] {
        heap.push(0x21);
        put(&mut heap, object, width);
        put(&mut heap, if object == 0x200 { 1 } else { 0 }, 4);
        put(&mut heap, class, width);
        put(&mut heap, (width + usize::from(boolean)) as u64, 4);
        if boolean {
            heap.push(0);
        }
        put(&mut heap, value, width);
    }
    heap.push(0x22);
    put(&mut heap, 0x204, width);
    put(&mut heap, 0, 4);
    put(&mut heap, 2, 4);
    put(&mut heap, 0x107, width);
    put(&mut heap, 0x200, width);
    put(&mut heap, 0, width);
    heap.push(0x23);
    put(&mut heap, 0x205, width);
    put(&mut heap, 0, 4);
    put(&mut heap, 4, 4);
    heap.push(8);
    heap.extend([1, 2, 3, 4]);
    heap.push(5);
    put(&mut heap, 0x106, width);
    heap.push(0x90);
    put(&mut heap, 0x203, width);
    record(0x1c, heap, &mut out);
    record(0x2c, vec![], &mut out);
    out
}
fn receipt() -> Receipt {
    serde_json::from_value(json!({"version":1,"id":"analysis-owned","collectorId":"collector-owned","signature":"owned","owner":"owned","phase":"complete","rawSha256":"owned","rawBytes":1,"format":"java-hprof","capturePhase":"complete","binding":{},"metadata":{}})).unwrap()
}
fn q(operation: &str, object: Option<&str>) -> Query {
    Query {
        operation: operation.into(),
        object_id: object.map(str::to_owned),
        class_name: None,
        max_depth: None,
    }
}
#[test]
fn diagnostic_memory_hprof_both_widths_keep_activity_paths_weak_edges_and_modeled_dominators() {
    for width in [4, 8] {
        let index = decode("java-hprof", &hprof(width));
        let result = query(&receipt(), &index, &q("paths", Some("0x201")), 0, 100).unwrap();
        assert_eq!(result["rows"][0]["underNotDestroyedActivity"], true);
        assert_eq!(
            result["rows"][0]["nodes"][1]["className"],
            "com.example.MainActivity"
        );
        let object = query(&receipt(), &index, &q("object", Some("0x200")), 0, 100).unwrap();
        assert_eq!(object["rows"][0]["modeledRetainedBytes"], 48);
        assert_eq!(
            object["rows"][0]["allocationStack"][0],
            "com.example.MainActivity::onCreate (MainActivity.kt:10)"
        );
        let weak = query(&receipt(), &index, &q("object", Some("0x203")), 0, 100).unwrap();
        assert_eq!(weak["rows"][0]["reachable"], false);
        assert!(weak["rows"][0]["modeledRetainedBytes"].is_null());
        let retainers = query(&receipt(), &index, &q("retainers", Some("0x203")), 0, 100).unwrap();
        assert_eq!(retainers["rows"][0]["edge"]["strength"], "weak");
        assert!(query(&receipt(), &index, &q("object", Some("expired")), 0, 100).is_err());
    }
}
fn graph() -> Value {
    json!({"schemaVersion":1,"runtime":"producer_native","sizeDefinition":"producer logical buffer payload bytes; not allocator/RSS","graphComplete":true,"objects":[{"id":"root","className":"Owner","kind":"object","selfBytes":1,"roots":["app owner"],"edges":[{"to":"left","name":"left","strength":"strong"},{"to":"right","name":"right","strength":"strong"}]},{"id":"left","className":"Buffer","kind":"object","selfBytes":10,"edges":[{"to":"leaf","name":"shared","strength":"strong"}]},{"id":"right","className":"Buffer","kind":"object","selfBytes":20,"edges":[{"to":"leaf","name":"shared","strength":"strong"}]},{"id":"leaf","className":"Buffer","kind":"object","selfBytes":30}]})
}
#[test]
fn diagnostic_memory_graph_diamond_dominators_and_missing_edges_never_invent_ownership() {
    let v = graph();
    let index = decode("memory-graph-v1", v.to_string().as_bytes());
    let leaf = query(&receipt(), &index, &q("object", Some("leaf")), 0, 100).unwrap();
    assert_eq!(leaf["rows"][0]["immediateDominator"], "root");
    let root = query(&receipt(), &index, &q("object", Some("root")), 0, 100).unwrap();
    assert_eq!(root["rows"][0]["modeledRetainedBytes"], 61);
    let mut missing = v.clone();
    missing["objects"][0]["edges"][0]["to"] = json!("missing");
    let index = decode("memory-graph-v1", missing.to_string().as_bytes());
    let root = query(&receipt(), &index, &q("object", Some("root")), 0, 100).unwrap();
    assert!(root["rows"][0]["modeledRetainedBytes"].is_null());
    assert_eq!(root["summary"]["graphComplete"], false);
    let mut unknown = v;
    unknown["objects"][0]["edges"][0]["strength"] = json!("unknown");
    let index = decode("memory-graph-v1", unknown.to_string().as_bytes());
    assert!(
        query(&receipt(), &index, &q("dominators", None), 0, 100).unwrap()["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn diagnostic_memory_large_reference_arrays_are_paged_without_losing_retention_edges() {
    let mut v = graph();
    let mut root = v["objects"][0].clone();
    root["edges"] = json!(
        (0..1000)
            .map(
                |i| json!({"to":format!("object-{i}"),"name":format!("[{i}]"),"strength":"strong"})
            )
            .collect::<Vec<_>>()
    );
    let mut objects = vec![root];
    for i in 0..1000 {
        objects.push(
            json!({"id":format!("object-{i}"),"className":"Buffer","kind":"object","selfBytes":1}),
        );
    }
    v["objects"] = json!(objects);
    let index = decode("memory-graph-v1", v.to_string().as_bytes());
    let root = query(&receipt(), &index, &q("object", Some("root")), 0, 100).unwrap();
    assert_eq!(root["rows"][0]["modeledRetainedBytes"], 1001);
    assert_eq!(root["rows"][0]["edgeCount"], 1000);
    assert_eq!(root["rows"][0]["edges"].as_array().unwrap().len(), 16);
    let edges = query(&receipt(), &index, &q("edges", Some("root")), 900, 100).unwrap();
    assert_eq!(edges["rows"].as_array().unwrap().len(), 100);
    assert_eq!(edges["rows"][99]["edge"]["to"], "object-999");
    assert_eq!(
        query(
            &receipt(),
            &index,
            &q("retainers", Some("object-999")),
            0,
            100
        )
        .unwrap()["rows"][0]["from"],
        "root"
    );
}
#[test]
fn diagnostic_memory_dominators_match_independent_remove_and_reach_oracle_on_cycles_and_shared_roots()
 {
    for seed in 0..64u64 {
        let mut state = seed + 1;
        let n = 7usize;
        let mut adjacency = vec![vec![]; n];
        for row in &mut adjacency {
            for target in 0..n {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                if state >> 62 == 0 {
                    row.push(target);
                }
            }
        }
        let roots = if seed % 3 == 0 { vec![0, 3] } else { vec![0] };
        let reach = |removed: Option<usize>| {
            let mut seen = vec![false; n];
            let mut todo = roots.clone();
            while let Some(node) = todo.pop() {
                if removed == Some(node) || seen[node] {
                    continue;
                }
                seen[node] = true;
                todo.extend(adjacency[node].iter().copied());
            }
            seen
        };
        let initial = reach(None);
        let v = json!({"schemaVersion":1,"runtime":"producer_native","graphComplete":true,"sizeDefinition":"logical payload bytes","objects":(0..n).map(|i|json!({"id":format!("n{i}"),"className":"Node","kind":"object","selfBytes":i+1,"roots":if roots.contains(&i){vec!["owner"]}else{vec![]},"edges":adjacency[i].iter().map(|j|json!({"to":format!("n{j}"),"name":"reference","strength":"strong"})).collect::<Vec<_>>()})).collect::<Vec<_>>()});
        let index = decode("memory-graph-v1", v.to_string().as_bytes());
        let observed = objects(&index).unwrap();
        for i in 0..n {
            let without = reach(Some(i));
            let expected = initial[i].then(|| {
                (0..n)
                    .filter(|&j| initial[j] && !without[j])
                    .map(|j| (j + 1) as u64)
                    .sum::<u64>()
            });
            assert_eq!(
                observed[i].modeled_retained_bytes, expected,
                "seed {seed}, object {i}"
            );
        }
    }
}
#[test]
fn diagnostic_memory_hprof_truncation_unknown_records_and_duplicate_ids_are_refused() {
    let bytes = hprof(8);
    for len in [0, 16, bytes.len() - 1] {
        assert!(super::super::decode::decode("java-hprof", &bytes[..len]).is_err());
    }
    let mut bad = graph();
    bad["objects"][1]["id"] = json!("root");
    assert!(super::super::decode::decode("memory-graph-v1", bad.to_string().as_bytes()).is_err());
    let mut bad = graph();
    bad["objects"][0]["modeledRetainedBytes"] = json!(99999);
    let index = decode("memory-graph-v1", bad.to_string().as_bytes());
    assert_eq!(
        query(&receipt(), &index, &q("object", Some("root")), 0, 100).unwrap()["rows"][0]["modeledRetainedBytes"],
        61
    );
}
fn series() -> Value {
    json!({"schemaVersion":1,"runtime":"native","processGeneration":"birth-a","buildId":"build-a","probeRefs":[],"samples":[{"phase":"warmup","iteration":0,"elapsedMicros":0,"counters":[{"resource":"textures","value":2,"unit":"count","source":"app probe"}]},{"phase":"baseline","iteration":0,"elapsedMicros":1,"counters":[{"resource":"textures","value":3,"unit":"count","source":"app probe"}]},{"phase":"after_action","iteration":1,"elapsedMicros":2,"actionId":"action:one","counters":[{"resource":"textures","value":8,"unit":"count","source":"app probe"}]},{"phase":"release","iteration":1,"elapsedMicros":3,"actionId":"action:one","counters":[{"resource":"textures","value":4,"unit":"count","source":"app probe"}]},{"phase":"cleanup","iteration":1,"elapsedMicros":4,"counters":[{"resource":"textures","value":3,"unit":"count","source":"app probe"}]}]})
}
#[test]
fn diagnostic_memory_resource_series_preserves_release_cleanup_and_never_calls_growth_a_leak() {
    let v = series();
    let index = decode("memory-series-v1", v.to_string().as_bytes());
    let result = query(&receipt(), &index, &q("series", None), 0, 100).unwrap();
    assert_eq!(result["rows"][0]["baselineDelta"], 1.0);
    assert_eq!(result["rows"][1]["baselineDelta"], 0.0);
    assert_eq!(result["leakEstablished"], false);
    let mut bad = v;
    bad["samples"][3]["actionId"] = json!("foreign");
    assert!(super::super::decode::decode("memory-series-v1", bad.to_string().as_bytes()).is_err());
}
#[test]
fn diagnostic_memory_comparison_is_class_only_and_refuses_changed_process_build_or_units() {
    let v = graph();
    let a = decode("memory-graph-v1", v.to_string().as_bytes());
    let mut v = v;
    v["objects"][3]["selfBytes"] = json!(40);
    let b = decode("memory-graph-v1", v.to_string().as_bytes());
    let mut r = receipt();
    r.format = "memory-graph-v1".into();
    r.binding = json!({"targetId":"host:acme","appId":"com.example.acme","buildId":"build-a","processId":42,"processGeneration":"birth-a"});
    r.metadata.sampling = Some("logical graph payload".into());
    let result = compare(&r, &a, &r, &b);
    assert_eq!(result["compatible"], true);
    assert_eq!(result["objectIdentityMatched"], false);
    assert_eq!(result["rows"][0]["modeledSelfBytesDelta"], 10);
    for key in ["processGeneration", "buildId"] {
        let mut other = r.clone();
        other.binding[key] = json!("different");
        assert_eq!(compare(&r, &a, &other, &b)["compatible"], false);
    }
    let mut other = r.clone();
    other.metadata.sampling = None;
    assert_eq!(compare(&r, &a, &other, &b)["compatible"], false);
}
#[test]
fn diagnostic_memory_apple_exports_keep_units_stacks_and_inspection_effects_separate() {
    let mut options:crate::diagnostics::native_trace::Options=serde_json::from_value(json!({"backend":"xctrace","mode":"capture","targetId":"device:ACME","appId":"com.example.acme","processId":42,"allowAttach":true,"template":"Allocations"})).unwrap();
    assert!(options.validate().is_err());
    options.allow_allocation_recording = true;
    assert!(options.validate().is_ok());
    options.template = Some("Leaks".into());
    assert!(options.validate().is_err());
    options.allow_heap_inspection = true;
    assert!(options.validate().is_ok());
    let tables=super::super::decode::native_trace::memory_tables(br#"<trace-toc><run><data><table schema="heap-allocations"/><table schema="vm-tracker"/><table schema="gui-private-memory-graph"/></data></run></trace-toc>"#).unwrap();
    assert_eq!(tables, vec!["heap-allocations", "vm-tracker"]);
    use base64::Engine;
    let e = json!({"schemaVersion":1,"backend":"xctrace","report":{"identity":{},"gaps":[]},"actionIds":[],"artifact":{"encoding":"identity","sha256":crate::diagnostics::digest(b"trace"),"base64":base64::engine::general_purpose::STANDARD.encode(b"trace")},"sections":[{"name":"heap-allocations","format":"xml","text":"<trace-query-result><row><size-in-bytes>32</size-in-bytes><backtrace><frame name=\"allocateBuffer\"/></backtrace></row><row><size>64</size></row></trace-query-result>"}]});
    let index = decode("native-trace-v1", e.to_string().as_bytes());
    assert_eq!(index.rows[0].kind, "allocation");
    assert_eq!(index.rows[0].unit.as_deref(), Some("bytes"));
    assert_eq!(index.rows[0].value, Some(32.0));
    assert!(index.rows[1].value.is_none());
    assert_eq!(
        index.rows[0].data["observation"]["allocationStackLeafFirst"][0]["attributes"]["name"],
        "allocateBuffer"
    );
    assert_eq!(index.rows[0].data["causeEstablished"], false);
}

fn append_hprof_classes(width: usize, heap: &mut Vec<u8>) {
    for i in 0..8 {
        heap.push(0x20);
        put(heap, 0x100 + i, width);
        put(heap, 0, 4);
        let super_id = match i {
            0 => 0,
            2 => 0x101,
            5 => 0x104,
            _ => 0x100,
        };
        put(heap, super_id, width);
        for _ in 0..5 {
            put(heap, 0, width);
        }
        put(
            heap,
            match i {
                1 => 24,
                2 => 32,
                3..=5 => 16,
                _ => 8,
            },
            4,
        );
        put(heap, 0, 2);
        let statics: Vec<(u64, u64)> = if i == 6 {
            vec![(24, 0x200), (25, 0x202), (26, 0x204), (27, 0x205)]
        } else {
            vec![]
        };
        put(heap, statics.len() as u64, 2);
        for (key, value) in statics {
            put(heap, key, width);
            heap.push(2);
            put(heap, value, width);
        }
        let fields = match i {
            1 => vec![(20, 4), (21, 2)],
            3 => vec![(22, 2)],
            4 => vec![(23, 2)],
            _ => vec![],
        };
        put(heap, fields.len() as u64, 2);
        for (key, ty) in fields {
            put(heap, key, width);
            heap.push(ty);
        }
    }
}
