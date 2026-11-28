//! Flat package-size inventories and distinct, producer-declared UI trees.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Node {
    id: String,
    parent: Option<String>,
    role: String,
    name: Option<String>,
    interactive: bool,
    hidden: bool,
    bounds: Option<[f64; 4]>,
}
pub(super) fn tree(
    index: &mut Index,
    base: &Value,
    path: &str,
    id: &str,
    kind: &str,
    framework: &str,
    nodes: Vec<Node>,
) -> Result<()> {
    label(id)?;
    label(framework)?;
    if !matches!(kind, "view" | "accessibility" | "component") || nodes.len() > 4096 {
        return Err(invalid());
    }
    let mut parents = BTreeMap::new();
    for n in &nodes {
        label(&n.id)?;
        label(&n.role)?;
        if let Some(name) = &n.name
            && name.len() > 1024
        {
            return Err(invalid());
        }
        if parents.insert(n.id.as_str(), n.parent.as_deref()).is_some() {
            return Err(invalid());
        }
        if let Some(b) = n.bounds
            && (b.iter().any(|v| !v.is_finite() || v.abs() > 1e9) || b[2] < 0.0 || b[3] < 0.0)
        {
            return Err(invalid());
        }
    }
    // A linear color walk validates even deep trees without recursive stack growth.
    let mut finished = BTreeSet::new();
    for n in &nodes {
        let mut seen = BTreeSet::new();
        let mut current = Some(n.id.as_str());
        while let Some(id) = current {
            if finished.contains(id) {
                break;
            }
            if !seen.insert(id) {
                return Err(invalid());
            }
            current = *parents.get(id).ok_or_else(invalid)?;
        }
        finished.extend(seen);
    }
    for (i, n) in nodes.into_iter().enumerate() {
        let mut data = base.clone();
        data["performance"] = json!({"category":"trees","treeId":id,"treeKind":kind,"framework":framework,"boundsUnit":"producer_logical_units","node":{"id":n.id,"parent":n.parent,"role":n.role,"name":n.name,"interactive":n.interactive,"hidden":n.hidden,"bounds":n.bounds},"meaning":"view, accessibility and framework component trees are separate; imported identities/semantics are producer declared"});
        emit(
            index,
            "event",
            "UI tree node",
            format!("{path}/nodes/{i}"),
            data,
            None,
            None,
        )?;
    }
    Ok(())
}
pub(super) fn checks(
    index: &Index,
    min: Option<f64>,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let mut rows = vec![];
    for r in &index.rows {
        let tree = &r.data["performance"];
        if tree["category"] != "trees" || tree["treeKind"] != "accessibility" {
            continue;
        }
        let n = &tree["node"];
        if n["hidden"] == true || n["interactive"] != true {
            continue;
        }
        let unnamed = n["name"].as_str().is_none_or(|s| s.trim().is_empty());
        let small = min
            .zip(n["bounds"].as_array())
            .is_some_and(|(m, b)| b[2].as_f64().unwrap() < m || b[3].as_f64().unwrap() < m);
        if unnamed || small {
            rows.push(json!({"source":r.source,"treeId":tree["treeId"],"nodeId":n["id"],"missingAccessibleName":unnamed,"belowDeclaredTargetSize":small,"minTargetSize":min,"unit":"producer_logical_units","status":"candidate_requires_platform_semantic_review"}));
        }
    }
    let matched = rows.len();
    let next = offset.saturating_add(limit).min(matched);
    Ok(
        json!({"rows":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"matched":matched,"nextOffset":if next<matched{Some(next)}else{None},"coverage":"only visible interactive producer-declared accessibility nodes; name/optional logical target size candidates, no contrast/focus/reading-order/screen-reader/conformance proof"}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Package {
    schema_version: u32,
    platform: String,
    architecture: String,
    build_mode: String,
    build_id: String,
    basis: String,
    source: String,
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Entry {
    path: String,
    bytes: u64,
    category: String,
}
pub(super) fn decode(value: Value, index: &mut Index) -> Result<()> {
    let p: Package = serde_json::from_value(value).map_err(|_| invalid())?;
    if p.schema_version != 1
        || p.entries.len() > 10000
        || !matches!(
            p.build_mode.as_str(),
            "debug" | "profile" | "release" | "unknown"
        )
        || !matches!(
            p.basis.as_str(),
            "archive_compressed" | "archive_uncompressed" | "installed" | "download" | "section"
        )
    {
        return Err(invalid());
    }
    for s in [
        &p.platform,
        &p.architecture,
        &p.build_mode,
        &p.build_id,
        &p.source,
    ] {
        label(s)?;
    }
    let mut paths = BTreeSet::new();
    let mut total = 0u64;
    for (i, e) in p.entries.into_iter().enumerate() {
        crate::diagnostics::model::label(&e.path, 1024)?;
        label(&e.category)?;
        if e.bytes > 9007199254740991
            || !paths.insert(e.path.clone())
            || e.path.starts_with('/')
            || e.path.contains('\\')
            || e.path
                .split('/')
                .any(|s| s == ".." || s == "." || s.is_empty())
        {
            return Err(invalid());
        }
        total = total.checked_add(e.bytes).ok_or_else(invalid)?;
        if total > 9007199254740991 {
            return Err(invalid());
        }
        let data = json!({"appId":null,"performance":{"category":"sizes","path":e.path,"assetCategory":e.category,"platform":p.platform,"architecture":p.architecture,"buildMode":p.build_mode,"buildId":p.build_id,"basis":p.basis,"sourceTool":p.source,"identityProvenance":"producer_declared","meaning":"flat disjoint producer inventory; no recursive parent totals, decompression, archive execution or startup impact inferred"}});
        emit(
            index,
            "metric",
            "package entry",
            format!("/entries/{i}"),
            data,
            None,
            Some((
                e.bytes as f64,
                "bytes",
                &format!("flat entry bytes, {} basis", p.basis),
            )),
        )?;
    }
    index.unsupported.push("Size inventory is producer-declared and may cover only part of a package; it is not archive validation, total download/install size, or startup causality. Pair with measured startup evidence.".into());
    Ok(())
}
pub(super) fn compare(
    a: &super::super::model::Receipt,
    ai: &Index,
    b: &super::super::model::Receipt,
    bi: &Index,
) -> Value {
    let unavailable =
        |reason: &str| json!({"compatible":false,"reason":reason,"startupImpactEstablished":false});
    if ai.format != "package-size-v1" || bi.format != "package-size-v1" {
        return unavailable("requires two explicit package-size-v1 inventories");
    }
    for key in ["targetId", "appId"] {
        if a.binding[key].as_str().is_none_or(|s| s.is_empty()) || a.binding[key] != b.binding[key]
        {
            return unavailable("matching declared targetId and appId required");
        }
    }
    let (Some(first_a), Some(first_b)) = (ai.rows.first(), bi.rows.first()) else {
        return unavailable("empty size inventory");
    };
    let left = &first_a.data["performance"];
    let right = &first_b.data["performance"];
    for key in [
        "platform",
        "architecture",
        "buildMode",
        "basis",
        "sourceTool",
    ] {
        if left[key] != right[key] {
            return unavailable(
                "different platform, architecture, build mode, size basis or source tool",
            );
        }
    }
    if left["buildMode"] == "unknown" {
        return unavailable("unknown build mode");
    }
    let map = |index: &Index| {
        index
            .rows
            .iter()
            .map(|r| {
                (
                    r.data["performance"]["path"].as_str().unwrap().to_owned(),
                    (
                        r.value.unwrap() as u64,
                        r.source.clone(),
                        r.data["performance"]["assetCategory"].clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let l = map(ai);
    let r = map(bi);
    let keys: BTreeSet<_> = l.keys().chain(r.keys()).collect();
    let mut changed = 0;
    let mut rows = vec![];
    for path in keys {
        let before = l.get(path);
        let after = r.get(path);
        if before.map(|v| v.0) == after.map(|v| v.0) && before.map(|v| &v.2) == after.map(|v| &v.2)
        {
            continue;
        }
        changed += 1;
        if rows.len() < 100 {
            rows.push(json!({"path":path,"beforeBytes":before.map(|v|v.0),"afterBytes":after.map(|v|v.0),"deltaBytes":before.zip(after).map(|(l,r)|r.0 as i64-l.0 as i64),"beforeSource":before.map(|v|&v.1),"afterSource":after.map(|v|&v.1),"meaning":"missing entry means absent from this inventory, not zero package bytes"}));
        }
    }
    json!({"compatible":true,"beforeBuildId":left["buildId"],"afterBuildId":right["buildId"],"basis":left["basis"],"changedEntries":changed,"rows":rows,"truncated":changed>100,"startupImpactEstablished":false,"meaning":"same declared target/app and producer size model; inventory membership may be partial, no download/install or startup-time inference"})
}
