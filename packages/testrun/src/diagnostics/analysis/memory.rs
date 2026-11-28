//! Runtime-specific graph observations and bounded retained-evidence queries.
pub(super) mod hprof;
mod series;
#[cfg(test)]
mod tests;
use super::model::{Index, Receipt};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub(super) const MAX_NODES: usize = 10000;
pub(super) const MAX_EDGES: usize = 50000;
pub(super) fn references(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    Ok(series::references(bytes)?
        .into_iter()
        .map(|p| (p.collector_id, p.sha256))
        .collect())
}
fn invalid() -> Error {
    Error::new(
        "INVALID_MEMORY_ARTIFACT",
        "Memory artifact has missing, ambiguous or unsupported graph records",
    )
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Edge {
    pub to: String,
    pub name: String,
    pub strength: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Object {
    pub id: String,
    pub class_name: String,
    pub kind: String,
    pub self_bytes: u64,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub allocation_stack: Vec<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub immediate_dominator: Option<String>,
    #[serde(default)]
    pub modeled_retained_bytes: Option<u64>,
    #[serde(default)]
    pub reachable: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Graph {
    pub schema_version: u32,
    pub runtime: String,
    pub size_definition: String,
    pub graph_complete: bool,
    #[serde(default)]
    pub identity: Value,
    pub objects: Vec<Object>,
    #[serde(default)]
    pub gaps: Vec<String>,
}
fn label(s: &str, max: usize) -> Result<()> {
    crate::diagnostics::model::label(s, max)
}
pub(super) fn decode(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    if format == "memory-series-v1" {
        return series::decode(bytes, index);
    }
    let mut graph = if format == "java-hprof" {
        hprof::decode(bytes)?
    } else {
        serde_json::from_value::<Graph>(super::decode::bounded_json(bytes, super::model::MAX_RAW)?)
            .map_err(|_| invalid())?
    };
    let (ids, edges) = validate_graph(format, &mut graph)?;
    dominate(&mut graph, &ids)?;
    let snapshot = crate::diagnostics::digest(bytes);
    let summary = json!({"schemaVersion":1,"runtime":graph.runtime,"snapshotDigest":snapshot,"identity":graph.identity,"objects":graph.objects.len(),"edges":edges,"graphComplete":graph.graph_complete,"sizeDefinition":graph.size_definition,"gaps":graph.gaps,"identityProvenance":"producer/caller declared; file digest identifies this snapshot only","leakEstablished":false});
    let mut r = super::decode::row(
        "event",
        "Memory graph",
        if format == "java-hprof" {
            "binary:header".into()
        } else {
            "/".into()
        },
        json!({"memorySummary":summary,"appId":null}),
    );
    super::decode::push(index, r.clone())?;
    for (i, mut o) in graph.objects.into_iter().enumerate() {
        let source = if format == "java-hprof" {
            o.source.clone()
        } else {
            format!("/objects/{i}")
        };
        o.source = source.clone();
        let edges = std::mem::take(&mut o.edges);
        let object_id = o.id.clone();
        o.edges = edges.iter().take(16).cloned().collect();
        r = super::decode::row(
            "retention",
            &o.class_name,
            source.clone(),
            json!({"memoryObject":o,"snapshotDigest":snapshot,"runtime":graph.runtime,"sizeDefinition":graph.size_definition,"appId":null,"leakEstablished":false}),
        );
        r.value = Some(r.data["memoryObject"]["selfBytes"].as_u64().unwrap() as f64);
        r.unit = Some("bytes".into());
        r.definition = Some(graph.size_definition.clone());
        super::decode::push(index, r)?;
        for (chunk, edges) in edges.get(16..).unwrap_or(&[]).chunks(32).enumerate() {
            let pointer = if format == "java-hprof" {
                source.clone()
            } else {
                format!("{source}/edges")
            };
            super::decode::push(
                index,
                super::decode::row(
                    "retention",
                    "Memory reference edges",
                    pointer,
                    json!({"appId":null,"snapshotDigest":snapshot,"memoryEdges":{"from":object_id,"edgeOffset":16+chunk*32,"edges":edges}}),
                ),
            )?;
        }
    }
    index.unsupported.push("Modelled graph/byte weights are runtime-specific observations, not native/RSS ownership or leak proof; snapshot object IDs never match across captures".into());
    if !graph.graph_complete {
        index.unsupported.push(
            "Incomplete or uncertain reference graph: dominators/retained bytes unavailable".into(),
        );
    }
    Ok(())
}
fn dominate(graph: &mut Graph, ids: &BTreeMap<String, usize>) -> Result<()> {
    let n = graph.objects.len();
    let root = n;
    let (next, previous) = reference_graph(graph, ids);
    let (visited, mut post) = visit_graph(&next, root);
    for (i, o) in graph.objects.iter_mut().enumerate() {
        o.reachable = visited[i];
    }
    if !graph.graph_complete {
        return Ok(());
    }
    post.reverse();
    let mut rank = vec![0; n + 1];
    for (i, &node) in post.iter().enumerate() {
        rank[node] = i;
    }
    let mut dom = vec![None; n + 1];
    dom[root] = Some(root);
    let mut work = 0usize;
    loop {
        let mut changed = false;
        for &node in post.iter().skip(1) {
            let mut candidate: Option<usize> = None;
            for &p in &previous[node] {
                if dom[p].is_none() {
                    continue;
                }
                candidate = Some(if let Some(mut a) = candidate {
                    let mut b = p;
                    while a != b {
                        work += 1;
                        if work > 4_000_000 {
                            return Err(Error::new(
                                "ANALYSIS_BOUNDS",
                                "Dominator work budget exceeded",
                            ));
                        }
                        if rank[a] > rank[b] {
                            a = dom[a].unwrap();
                        } else {
                            b = dom[b].unwrap();
                        }
                    }
                    a
                } else {
                    p
                });
            }
            if dom[node] != candidate {
                dom[node] = candidate;
                changed = true;
            }
            work += previous[node].len();
            if work > 4_000_000 {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "Dominator work budget exceeded",
                ));
            }
        }
        if !changed {
            break;
        }
    }
    let mut retained: Vec<u64> = graph
        .objects
        .iter()
        .map(|o| o.self_bytes)
        .chain(std::iter::once(0))
        .collect();
    for &node in post.iter().rev() {
        if node == root {
            continue;
        }
        let parent = dom[node].unwrap();
        retained[parent] = retained[parent]
            .checked_add(retained[node])
            .ok_or_else(invalid)?;
    }
    let names: Vec<_> = graph.objects.iter().map(|o| o.id.clone()).collect();
    for (i, o) in graph.objects.iter_mut().enumerate() {
        if visited[i] {
            o.modeled_retained_bytes = Some(retained[i]);
            o.immediate_dominator = dom[i].filter(|&p| p != root).map(|p| names[p].clone());
        }
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Query {
    pub operation: String,
    pub object_id: Option<String>,
    pub class_name: Option<String>,
    pub max_depth: Option<usize>,
}
fn objects(index: &Index) -> Result<Vec<Object>> {
    let mut objects: Vec<Object> = index
        .rows
        .iter()
        .filter_map(|r| r.data.get("memoryObject"))
        .map(|o| serde_json::from_value(o.clone()).map_err(|_| invalid()))
        .collect::<Result<_>>()?;
    let ids: BTreeMap<_, _> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| (o.id.clone(), i))
        .collect();
    for row in &index.rows {
        if let Some(chunk) = row.data.get("memoryEdges") {
            let id = chunk["from"].as_str().ok_or_else(invalid)?;
            let &i = ids.get(id).ok_or_else(invalid)?;
            let edges: Vec<Edge> =
                serde_json::from_value(chunk["edges"].clone()).map_err(|_| invalid())?;
            if objects[i].edges.len() != chunk["edgeOffset"].as_u64().ok_or_else(invalid)? as usize
            {
                return Err(invalid());
            }
            objects[i].edges.extend(edges);
        }
    }
    Ok(objects)
}
fn display(o: &Object) -> Value {
    let mut result = serde_json::to_value(o).unwrap();
    result["edges"] = json!(o.edges.iter().take(16).collect::<Vec<_>>());
    result["edgeCount"] = json!(o.edges.len());
    result["edgesTruncated"] = json!(o.edges.len() > 16);
    result
}
pub(super) fn query(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    if !matches!(
        index.format.as_str(),
        "java-hprof" | "memory-graph-v1" | "memory-series-v1"
    ) {
        return Err(Error::new(
            "MEMORY_GRAPH_UNAVAILABLE",
            "Memory views require an explicitly decoded graph or resource series",
        ));
    }
    if q.object_id.is_some()
        != matches!(
            q.operation.as_str(),
            "object" | "edges" | "retainers" | "paths"
        )
        || q.class_name.is_some()
            && !matches!(q.operation.as_str(), "classes" | "objects" | "dominators")
        || q.max_depth.is_some() && q.operation != "paths"
    {
        return Err(Error::invalid(
            "Use objectId only for object/edges/retainers/paths, className for class/object lists and maxDepth only for paths",
        ));
    }
    if let Some(c) = &q.class_name {
        label(c, 512)?;
    }
    if q.operation == "series" {
        let mut result = series::query(index)?;
        let rows = result["rows"].as_array().unwrap();
        let matched = rows.len();
        let page = rows
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next = offset + page.len();
        result["rows"] = json!(page);
        result["matched"] = json!(matched);
        result["nextOffset"] = if next < matched {
            json!(next)
        } else {
            Value::Null
        };
        result["collectorId"] = json!(receipt.collector_id);
        result["rawSha256"] = json!(receipt.raw_sha256);
        return Ok(result);
    }
    let summary=index.rows.iter().find_map(|r|r.data.get("memorySummary")).ok_or_else(||Error::new("MEMORY_GRAPH_UNAVAILABLE","Select java-hprof or memory-graph-v1; use runtime-native queries for browser/Dart and allocation rows for samples"))?;
    let objects = objects(index)?;
    let max_depth = q.max_depth.unwrap_or(32);
    if !(1..=64).contains(&max_depth) {
        return Err(Error::invalid("memory.maxDepth must be 1..64"));
    }
    let mut rows = vec![];
    match q.operation.as_str() {
        "summary" => rows.push(summary.clone()),
        "classes" => {
            for (class, (count, bytes)) in classes(index)? {
                if q.class_name.as_ref().is_none_or(|name| name == &class) {
                    rows.push(json!({"className":class,"count":count,"modeledSelfBytes":bytes,"retainedBytesAdditive":false}));
                }
            }
        }
        "objects" | "dominators" => {
            for o in &objects {
                if q.class_name.as_ref().is_none_or(|c| c == &o.class_name) {
                    if q.operation == "dominators" && o.modeled_retained_bytes.is_none() {
                        continue;
                    }
                    rows.push(display(o));
                }
            }
        }
        "object" | "edges" | "retainers" | "paths" => {
            rows = object_rows(&objects, summary, q, max_depth)?;
        }
        _ => {
            return Err(Error::invalid(
                "memory.operation must be summary/classes/objects/object/edges/retainers/paths/dominators/series",
            ));
        }
    }
    let matched = rows.len();
    let page: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();
    let next = offset + page.len();
    let result = json!({"view":"memory","operation":q.operation,"rows":page,"matched":matched,"nextOffset":if next<matched{Some(next)}else{None},"summary":summary,"collectorId":receipt.collector_id,"rawSha256":receipt.raw_sha256,"snapshotObjectIdentity":"this raw digest only","rootPathMeaning":"recorded strong-reference path, not proof of unexpected ownership or a leak; empty bounded query does not prove absence","leakEstablished":false,"private":true});
    Ok(result)
}
fn path(objects: &[Object], id: &str, max_depth: usize, android: bool) -> Result<Vec<Value>> {
    let map: BTreeMap<_, _> = objects.iter().map(|o| (o.id.as_str(), o)).collect();
    let mut queue = VecDeque::new();
    let mut previous: BTreeMap<&str, Option<(&str, &Edge)>> = BTreeMap::new();
    for o in objects {
        if !o.roots.is_empty() {
            queue.push_back((o.id.as_str(), 0));
            previous.insert(o.id.as_str(), None);
        }
    }
    let mut clipped = false;
    while let Some((current, depth)) = queue.pop_front() {
        if current == id {
            let mut nodes = vec![];
            let mut cursor = current;
            let mut live_activity = false;
            loop {
                let o = map[cursor];
                live_activity |=
                    o.attributes.get("androidActivityDestroyed") == Some(&json!(false));
                nodes.push(json!({"objectId":o.id,"className":o.class_name,"roots":o.roots,"attributes":o.attributes,"via":previous[cursor].map(|(_,e)|e)}));
                if let Some((parent, _)) = previous[cursor] {
                    cursor = parent;
                } else {
                    break;
                }
            }
            nodes.reverse();
            return Ok(vec![
                json!({"nodes":nodes,"underNotDestroyedActivity":if android{Some(live_activity)}else{None},"activityMeaning":"mDestroyed=false where recorded; only on this path; not resumed/foreground or valid release proof","shortestRecordedStrongPath":true,"truncated":false}),
            ]);
        }
        if depth == max_depth {
            clipped = true;
            continue;
        }
        for edge in &map[current].edges {
            if edge.strength == "strong"
                && map.contains_key(edge.to.as_str())
                && !previous.contains_key(edge.to.as_str())
            {
                previous.insert(edge.to.as_str(), Some((current, edge)));
                queue.push_back((edge.to.as_str(), depth + 1));
            }
        }
    }
    Ok(vec![
        json!({"nodes":[],"truncated":clipped,"reason":"no path in the supplied graph within depth; absence not established"}),
    ])
}
fn classes(index: &Index) -> Result<BTreeMap<String, (u64, u64)>> {
    let mut out = BTreeMap::new();
    for o in objects(index)? {
        let e = out.entry(o.class_name).or_insert((0u64, 0u64));
        e.0 += 1;
        e.1 = e.1.checked_add(o.self_bytes).ok_or_else(invalid)?;
    }
    Ok(out)
}
pub(super) fn compare(a: &Receipt, ai: &Index, b: &Receipt, bi: &Index) -> Value {
    let valid = ai.format == bi.format
        && matches!(ai.format.as_str(), "java-hprof" | "memory-graph-v1")
        && [
            "targetId",
            "appId",
            "buildId",
            "processId",
            "processGeneration",
        ]
        .iter()
        .all(|k| !a.binding[k].is_null() && a.binding[k] == b.binding[k])
        && a.metadata.sampling.is_some()
        && a.metadata.sampling == b.metadata.sampling;
    let summaries = |i: &Index| {
        i.rows
            .iter()
            .find_map(|r| r.data.get("memorySummary"))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let sa = summaries(ai);
    let sb = summaries(bi);
    let compatible = valid
        && !sa.is_null()
        && !sb.is_null()
        && sa["graphComplete"] == true
        && sb["graphComplete"] == true
        && sa["runtime"] == sb["runtime"]
        && sa["sizeDefinition"] == sb["sizeDefinition"];
    let mut rows = vec![];
    if compatible && let (Ok(left), Ok(right)) = (classes(ai), classes(bi)) {
        for class in left
            .keys()
            .chain(right.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(200)
        {
            let x = left.get(class).copied().unwrap_or_default();
            let y = right.get(class).copied().unwrap_or_default();
            rows.push(json!({"className":class,"countDelta":y.0 as i64-x.0 as i64,"modeledSelfBytesDelta":y.1 as i128-x.1 as i128}));
        }
    }
    json!({"method":"runtime-class-observations/1","compatible":compatible,"rows":rows,"objectIdentityMatched":false,"leakEstablished":false,"meaning":"class count/defined byte-weight observations under equal caller-declared process/build/sampling; no object IDs, cross-runtime heap identity, retained-size sum or leak attribution"})
}

fn validate_graph(format: &str, graph: &mut Graph) -> Result<(BTreeMap<String, usize>, usize)> {
    if graph.schema_version != 1 || graph.objects.len() > MAX_NODES || graph.gaps.len() > 32 {
        return Err(invalid());
    }
    if !matches!(
        graph.runtime.as_str(),
        "jvm_hprof"
            | "android_hprof"
            | "producer_v8"
            | "producer_hermes"
            | "producer_dart"
            | "producer_native"
    ) {
        return Err(invalid());
    }
    if format != "java-hprof" && matches!(graph.runtime.as_str(), "jvm_hprof" | "android_hprof") {
        return Err(invalid());
    }
    label(&graph.size_definition, 1024)?;
    let (ids, edges) = validate_objects(graph)?;
    if graph
        .objects
        .iter()
        .flat_map(|o| &o.edges)
        .any(|e| !ids.contains_key(&e.to))
    {
        graph.graph_complete = false;
        graph
            .gaps
            .push("references outside the supplied graph; retained-size model unavailable".into());
    }
    let total = graph
        .objects
        .iter()
        .try_fold(0u64, |sum, o| sum.checked_add(o.self_bytes))
        .ok_or_else(invalid)?;
    if total > 9007199254740991 {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Memory byte model exceeds exact numeric range",
        ));
    }
    Ok((ids, edges))
}
fn validate_objects(graph: &mut Graph) -> Result<(BTreeMap<String, usize>, usize)> {
    let mut ids = BTreeMap::new();
    let mut edges = 0;
    for (i, o) in graph.objects.iter_mut().enumerate() {
        label(&o.id, 128)?;
        label(&o.class_name, 512)?;
        label(&o.kind, 64)?;
        if o.self_bytes > 9007199254740991
            || ids.insert(o.id.clone(), i).is_some()
            || o.edges.len() > MAX_EDGES
            || o.roots.len() > 32
            || o.attributes.len() > 16
            || o.allocation_stack.len() > 64
        {
            return Err(invalid());
        }
        // Derived values in an imported graph never bypass owned analysis.
        o.immediate_dominator = None;
        o.modeled_retained_bytes = None;
        o.reachable = false;
        for (key, value) in &o.attributes {
            label(key, 256)?;
            if !(value.is_boolean()
                || value.is_number()
                || value.is_null()
                || value.as_str().is_some_and(|s| s.len() <= 512))
            {
                return Err(invalid());
            }
        }
        for name in o.roots.iter().chain(&o.allocation_stack) {
            label(name, 512)?;
        }
        for edge in &o.edges {
            label(&edge.to, 128)?;
            label(&edge.name, 512)?;
            if !matches!(edge.strength.as_str(), "strong" | "weak" | "unknown") {
                return Err(invalid());
            }
            if edge.strength == "unknown" {
                graph.graph_complete = false;
            }
            edges += 1;
        }
    }
    if edges > MAX_EDGES {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Memory graph exceeds 50000 edges",
        ));
    }
    Ok((ids, edges))
}
type Adjacency = Vec<Vec<usize>>;
fn reference_graph(graph: &Graph, ids: &BTreeMap<String, usize>) -> (Adjacency, Adjacency) {
    let n = graph.objects.len();
    let root = n;
    let mut next = vec![vec![]; n + 1];
    let mut previous = vec![vec![]; n + 1];
    for (i, o) in graph.objects.iter().enumerate() {
        if !o.roots.is_empty() {
            next[root].push(i);
            previous[i].push(root);
        }
        for e in &o.edges {
            if e.strength == "strong"
                && let Some(&j) = ids.get(&e.to)
            {
                next[i].push(j);
                previous[j].push(i);
            }
        }
    }
    (next, previous)
}
fn object_rows(
    objects: &[Object],
    summary: &Value,
    q: &Query,
    max_depth: usize,
) -> Result<Vec<Value>> {
    let mut rows = vec![];
    let id = q
        .object_id
        .as_deref()
        .ok_or_else(|| Error::invalid("memory.objectId is required"))?;
    label(id, 128)?;
    let object = objects.iter().find(|o| o.id == id).ok_or_else(|| {
        Error::new(
            "MEMORY_OBJECT_UNAVAILABLE",
            "Object ID is absent from this snapshot; no replacement selected",
        )
    })?;
    if q.operation == "object" {
        rows.push(display(object));
    } else if q.operation == "edges" {
        rows = object
            .edges
            .iter()
            .map(|e| json!({"from":object.id,"edge":e,"source":object.source}))
            .collect();
    } else if q.operation == "retainers" {
        for o in objects {
            for e in &o.edges {
                if e.to == id {
                    rows.push(json!({"from":o.id,"className":o.class_name,"edge":e}));
                }
            }
        }
    } else {
        rows = path(
            objects,
            id,
            max_depth,
            summary["runtime"] == "android_hprof",
        )?;
    }
    Ok(rows)
}

fn visit_graph(next: &Adjacency, root: usize) -> (Vec<bool>, Vec<usize>) {
    let mut visited = vec![false; next.len()];
    let mut post = vec![];
    let mut stack = vec![(root, 0)];
    visited[root] = true;
    while let Some((node, cursor)) = stack.last_mut() {
        if *cursor < next[*node].len() {
            let child = next[*node][*cursor];
            *cursor += 1;
            if !visited[child] {
                visited[child] = true;
                stack.push((child, 0));
            }
        } else {
            post.push(*node);
            stack.pop();
        }
    }
    (visited, post)
}
