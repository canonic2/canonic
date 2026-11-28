//! Bounded scalar snapshots, materialized equality postings and declared read-plan budgets.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Snapshot {
    schema_version: u32,
    format: String,
    backend: String,
    dataset: String,
    scope: String,
    account: String,
    project: String,
    role: String,
    snapshot_id: String,
    consistency: String,
    source_owner: String,
    coverage: String,
    plan: Option<Plan>,
    collections: Vec<Collection>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Plan {
    preset: String,
    read_only: bool,
    indexed: bool,
    index_name: String,
    timeout_ms: u32,
    max_rows: usize,
    max_scanned: u64,
    estimated_scanned: u64,
    actual_scanned: Option<u64>,
    cost_unit: String,
    max_cost: f64,
    estimated_cost: f64,
    actual_cost: Option<f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Collection {
    name: String,
    indexes: Vec<String>,
    records: Vec<Entity>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Entity {
    key: String,
    fields: BTreeMap<String, Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Equals {
    pub field: String,
    pub value: Value,
}
fn scalar(value: &Value) -> Result<()> {
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(n) => {
            let n = n.as_f64().ok_or_else(invalid)?;
            if !n.is_finite() || n.abs() > 9007199254740991.0 {
                return Err(invalid());
            }
            Ok(())
        }
        Value::String(s) if s.len() <= 4096 => Ok(()),
        _ => Err(invalid()),
    }
}
#[expect(
    clippy::too_many_lines,
    reason = "one bounded snapshot admission validates declared plans and builds source-witnessed equality postings"
)]
pub(super) fn decode(value: Value, index: &mut Index, prefix: &str) -> Result<()> {
    let snapshot: Snapshot = serde_json::from_value(value).map_err(|_| invalid())?;
    if snapshot.schema_version != 1
        || snapshot.format != "data-snapshot-v1"
        || snapshot.collections.len() > 32
        || !matches!(
            snapshot.backend.as_str(),
            "preferences"
                | "files"
                | "sqlite"
                | "room"
                | "core_data"
                | "indexeddb"
                | "cache"
                | "remote_sql"
                | "remote_search"
                | "remote_cache"
        )
        || !matches!(
            snapshot.role.as_str(),
            "authoritative" | "derived" | "cache" | "index"
        )
        || !matches!(
            snapshot.consistency.as_str(),
            "atomic" | "best_effort" | "unknown"
        )
    {
        return Err(invalid());
    }
    for s in [
        &snapshot.dataset,
        &snapshot.scope,
        &snapshot.account,
        &snapshot.project,
        &snapshot.snapshot_id,
        &snapshot.source_owner,
        &snapshot.coverage,
    ] {
        label(s)?;
    }
    let count: usize = snapshot.collections.iter().map(|c| c.records.len()).sum();
    if count > 2000 {
        return Err(invalid());
    }
    if snapshot.backend.starts_with("remote_") && snapshot.plan.is_none() {
        return Err(Error::new(
            "DATA_READ_PLAN_REQUIRED",
            "Remote exports require a named indexed read-only plan with row/time/scan/cost budgets",
        ));
    }
    if let Some(p) = &snapshot.plan {
        label(&p.preset)?;
        label(&p.index_name)?;
        for n in [Some(p.max_cost), Some(p.estimated_cost), p.actual_cost]
            .into_iter()
            .flatten()
        {
            number(n)?;
        }
        if !p.read_only
            || !p.indexed
            || !(1..=10000).contains(&p.timeout_ms)
            || !(1..=2000).contains(&p.max_rows)
            || count > p.max_rows
            || p.max_scanned > 1_000_000
            || p.estimated_scanned > p.max_scanned
            || p.actual_scanned.is_some_and(|n| n > p.max_scanned)
            || p.estimated_cost > p.max_cost
            || p.actual_cost.is_some_and(|n| n > p.max_cost)
            || !matches!(p.cost_unit.as_str(), "rows" | "bytes" | "read_units")
        {
            return Err(Error::new(
                "DATA_READ_BUDGET",
                "Read-only/indexed declaration or query budget is invalid/exceeded",
            ));
        }
    }
    let summary = json!({"category":"schema","backend":snapshot.backend,"dataset":snapshot.dataset,"scope":snapshot.scope,"account":snapshot.account,"project":snapshot.project,"role":snapshot.role,"snapshotId":snapshot.snapshot_id,"consistency":snapshot.consistency,"sourceOwner":snapshot.source_owner,"coverage":snapshot.coverage,"plan":snapshot.plan,"records":count,"collections":snapshot.collections.iter().map(|c|json!({"name":c.name,"indexes":c.indexes})).collect::<Vec<_>>(),"provenance":"producer-declared; broker/account/transaction/read-only/plan/cost claims not authenticated by parsing"});
    let summary_ordinal = index.rows.len();
    emit(
        index,
        "event",
        "Data snapshot schema",
        format!("{prefix}/schemaVersion"),
        summary,
    )?;
    let mut names = BTreeSet::new();
    for (c, collection) in snapshot.collections.into_iter().enumerate() {
        label(&collection.name)?;
        if !names.insert(collection.name.clone())
            || collection.indexes.len() > 4
            || collection.indexes.iter().collect::<BTreeSet<_>>().len() != collection.indexes.len()
        {
            return Err(invalid());
        }
        for field in &collection.indexes {
            label(field)?;
            if field == "$key" {
                return Err(invalid());
            }
        }
        let mut keys = BTreeSet::new();
        let mut postings: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        for (i, entity) in collection.records.into_iter().enumerate() {
            label(&entity.key)?;
            if !keys.insert(entity.key.clone()) || entity.fields.len() > 32 {
                return Err(invalid());
            }
            let ordinal = index.rows.len();
            postings
                .entry(("$key".into(), json!(entity.key).to_string()))
                .or_default()
                .push(ordinal);
            for (field, value) in &entity.fields {
                label(field)?;
                scalar(value)?;
                if collection.indexes.contains(field) {
                    postings
                        .entry((field.clone(), value.to_string()))
                        .or_default()
                        .push(ordinal);
                }
            }
            emit(
                index,
                "event",
                "Data snapshot record",
                format!("{prefix}/collections/{c}/records/{i}"),
                json!({"category":"data","summaryOrdinal":summary_ordinal,"collection":collection.name,"key":entity.key,"fields":entity.fields}),
            )?;
        }
        for ((field, value), ordinals) in postings {
            for chunk in ordinals.chunks(128) {
                emit(
                    index,
                    "event",
                    "Data equality index",
                    format!("{prefix}/collections/{c}"),
                    json!({"category":"data_index","collection":collection.name,"field":field,"value":serde_json::from_str::<Value>(&value).unwrap(),"ordinals":chunk,"meaning":"owned derived equality posting; missing fields have no posting, explicit null is indexed"}),
                )?;
            }
        }
    }
    index.unsupported.push("Snapshot consistency, sandbox/decryption access and read-only/cost plans are producer declarations; data inspection does not open live DBs, run SQL, repair data, flush caches or obtain credentials".into());
    Ok(())
}
#[expect(
    clippy::too_many_lines,
    reason = "one bounded query validates index selection and projects its exact retained witnesses"
)]
pub(super) fn query(index: &Index, q: &Query, offset: usize, limit: usize) -> Result<Value> {
    if q.id.is_some()
        || q.identity_scope.is_some()
        || q.trace_id.is_some()
        || q.request_id.is_some()
        || q.key.is_some() && q.equals.is_some()
    {
        return Err(Error::invalid(
            "Data queries use collection and either key or equals; no network selectors",
        ));
    }
    if let Some(name) = &q.collection {
        label(name)?;
    }
    if q.operation != "data" && (q.collection.is_some() || q.key.is_some() || q.equals.is_some()) {
        return Err(Error::invalid(
            "schema/query_plan does not accept record selectors",
        ));
    }
    let summary = index
        .rows
        .iter()
        .find(|r| r.data["networkData"]["category"] == "schema")
        .ok_or_else(|| {
            Error::new(
                "DATA_SNAPSHOT_UNAVAILABLE",
                "Select a decoded data snapshot or settled HTTP JSON snapshot export",
            )
        })?;
    let schema = &summary.data["networkData"];
    if q.operation == "schema" || q.operation == "query_plan" {
        return Ok(page(
            vec![json!({"source":summary.source,"schema":schema})],
            offset,
            limit,
        ));
    }
    if (q.key.is_some() || q.equals.is_some()) && q.collection.is_none() {
        return Err(Error::invalid("Indexed selection requires collection"));
    }
    if let Some(collection) = &q.collection
        && !schema["collections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == *collection)
    {
        return Err(Error::invalid("Unknown snapshot collection"));
    }
    let selection = if let Some(key) = &q.key {
        label(key)?;
        Some(("$key".to_owned(), json!(key)))
    } else if let Some(eq) = &q.equals {
        label(&eq.field)?;
        scalar(&eq.value)?;
        let collection = schema["collections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"].as_str() == q.collection.as_deref())
            .ok_or_else(invalid)?;
        if !collection["indexes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == &eq.field)
        {
            return Err(Error::new(
                "DATA_INDEX_REQUIRED",
                "Equality field must be declared indexed; no full-scan query fallback",
            ));
        }
        Some((eq.field.clone(), eq.value.clone()))
    } else {
        None
    };
    let mut candidates = vec![];
    let mut posting_rows = 0;
    if let Some((field, value)) = &selection {
        for r in &index.rows {
            let n = &r.data["networkData"];
            if n["category"] == "data_index"
                && n["collection"] == *q.collection.as_ref().unwrap()
                && n["field"] == *field
                && n["value"] == *value
            {
                posting_rows += 1;
                for ordinal in n["ordinals"].as_array().unwrap() {
                    let ordinal = ordinal.as_u64().ok_or_else(invalid)? as usize;
                    let r = index.rows.get(ordinal).ok_or_else(invalid)?;
                    if r.data["networkData"]["category"] != "data" {
                        return Err(invalid());
                    }
                    candidates.push(r);
                }
            }
        }
    } else {
        candidates.extend(index.rows.iter().filter(|r| {
            r.data["networkData"]["category"] == "data"
                && q.collection
                    .as_ref()
                    .is_none_or(|c| r.data["networkData"]["collection"] == *c)
        }));
    }
    let mut result = page(
        candidates
            .into_iter()
            .map(|r| json!({"source":r.source,"record":r.data["networkData"]}))
            .collect(),
        offset,
        limit,
    );
    result["selection"] = json!({"method":if selection.is_some(){"materialized_equality_postings"}else{"bounded_snapshot_browse"},"matchingPostingRows":posting_rows,"remoteExecution":false,"liveDatabaseRead":false});
    result["schema"] = schema.clone();
    Ok(result)
}
pub(super) fn compare(
    a: &super::super::model::Receipt,
    ai: &Index,
    b: &super::super::model::Receipt,
    bi: &Index,
) -> Value {
    let summary = |i: &Index| {
        i.rows
            .iter()
            .find(|r| r.data["networkData"]["category"] == "schema")
            .map(|r| r.data["networkData"].clone())
    };
    let unavailable =
        |reason: &str| json!({"compatible":false,"reason":reason,"repairPerformed":false});
    let (Some(left), Some(right)) = (summary(ai), summary(bi)) else {
        return unavailable("two decoded data snapshots required");
    };
    for field in ["dataset", "scope", "account", "project"] {
        if left[field] != right[field] {
            return unavailable("different dataset/scope/account/project");
        }
    }
    for field in ["appId", "targetId"] {
        if a.binding[field].as_str().is_none_or(|s| s.is_empty())
            || a.binding[field] != b.binding[field]
        {
            return unavailable("matching known declared target/app required");
        }
    }
    let map = |i: &Index| {
        i.rows
            .iter()
            .filter(|r| r.data["networkData"]["category"] == "data")
            .map(|r| {
                let n = &r.data["networkData"];
                (
                    (
                        n["collection"].as_str().unwrap().to_owned(),
                        n["key"].as_str().unwrap().to_owned(),
                    ),
                    (n["fields"].clone(), r.source.clone()),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let l = map(ai);
    let r = map(bi);
    let keys: BTreeSet<_> = l.keys().chain(r.keys()).collect();
    let mut differences = vec![];
    let mut changed = 0;
    for key in keys {
        let before = l.get(key);
        let after = r.get(key);
        if before.map(|r| &r.0) == after.map(|r| &r.0) {
            continue;
        }
        changed += 1;
        if differences.len() < 100 {
            differences.push(json!({"collection":key.0,"key":key.1,"beforePresent":before.is_some(),"afterPresent":after.is_some(),"beforeDigest":before.map(|r|crate::diagnostics::digest(r.0.to_string().as_bytes())),"afterDigest":after.map(|r|crate::diagnostics::digest(r.0.to_string().as_bytes())),"beforeSource":before.map(|r|&r.1),"afterSource":after.map(|r|&r.1)}));
        }
    }
    json!({"compatible":true,"beforeRole":left["role"],"afterRole":right["role"],"beforeSnapshot":left["snapshotId"],"afterSnapshot":right["snapshotId"],"beforeConsistency":left["consistency"],"afterConsistency":right["consistency"],"differences":differences,"changedRecords":changed,"truncated":changed>100,"meaning":"observed exact scalar snapshot differences; missing record differs from present null; roles/consistency are declared, not proof of stale cache/index or authoritative correctness","repairPerformed":false})
}
