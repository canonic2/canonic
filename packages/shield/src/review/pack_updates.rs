//! Cached pack availability, independent of policy preparation and enforcement.
mod cache;
mod notices;
use super::{Context, model::State, registries};
use canonic_registry_kit::{
    catalog::{Catalog, Entry},
    checkpoint::Checkpoint,
    source::Source,
    timestamp,
};
pub(crate) use notices::CLI_NEXT_STEP;
pub(crate) use notices::pending;
pub(crate) use notices::summary as inspection_summary;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::UNIX_EPOCH};

const DAY_MS: u64 = 86_400_000;
const OUTPUT_BYTES: usize = 8192;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    source: Source,
    observed_at: u64,
    expires_at: Option<String>,
    checkpoint: Checkpoint,
    heads: Vec<Entry>,
}

/// Only explicit, successfully admitted acquisition calls this. Failure is
/// disposable advisory failure and must not alter the primary operation.
pub(super) fn retain(context: &Context, source: &Source, catalog: &Catalog) {
    let _ = retain_at(context, source, catalog, super::clock::best_effort_ms());
}

fn retain_at(context: &Context, source: &Source, catalog: &Catalog, now: u64) -> Option<()> {
    if now == 0 || source.identity() != catalog.source_identity() {
        return None;
    }
    // The caller already admitted the catalog. Project only discovery heads;
    // do not repeat admission or copy installed/protected releases into this cache.
    let checkpoint = Checkpoint {
        sequence: catalog.sequence(),
        digest: catalog.digest().to_owned(),
        releases: catalog
            .heads()
            .iter()
            .map(|entry| {
                (
                    format!("{}@{}", entry.id, entry.version),
                    entry.sha256.clone(),
                )
            })
            .collect(),
        commands: BTreeMap::new(),
    };
    let snapshot = Snapshot {
        schema_version: 1,
        source: source.clone(),
        observed_at: now,
        expires_at: catalog.expires_at().map(str::to_owned),
        checkpoint,
        heads: catalog.heads().to_vec(),
    };
    cache::retain(context, &snapshot).ok()
}

/// Share the caller's existing checked metadata. No additional project read.
pub(super) fn inspect(context: &Context, state: &State, selector: Option<&str>) -> Value {
    inspect_at(context, state, selector, super::clock::best_effort_ms())
}

fn inspect_at(context: &Context, state: &State, selector: Option<&str>, now: u64) -> Value {
    let mut items = Vec::new();
    let mut sources = Vec::new();
    let mut eligible: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    let mut direct = 0;
    let mut dependencies = 0;
    for row in state.operator["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| selector.is_none_or(|s| row["selector"] == s))
    {
        if row["direct"] == false {
            dependencies += 1;
            continue;
        }
        direct += 1;
        if let Some(id) = row["source_identity"].as_str() {
            eligible.entry(id).or_default().push(row);
        }
    }
    let effective = registries::effective_from(&state.settings).ok();
    let mut covered = 0;
    for (identity, rows) in &eligible {
        let source = effective.as_ref().and_then(|e| e.by_identity(identity));
        let usable = source.filter(|(_, source, _)| {
            let source = json!(source);
            state.operator["installed"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["source_identity"] == *identity)
                .all(|row| row["source"] == source)
        });
        let snapshot = usable.and_then(|(_, source, _)| cache::load(context, source).ok());
        let snapshot = snapshot
            .filter(|snapshot| valid(snapshot) && against_project(snapshot, state, identity));
        let freshness = snapshot
            .as_ref()
            .map_or("unavailable", |s| freshness(s, now));
        if freshness == "fresh" {
            covered += 1;
        }
        sources.push(json!({"source_identity":identity,"freshness":freshness,
            "reason":if usable.is_none() { Some("source_ineligible") }
                else if snapshot.is_none() { Some("metadata_unavailable") } else { None }}));
        let Some(snapshot) = snapshot else {
            continue;
        };
        let alias = usable.map(|(alias, _, _)| alias).unwrap_or_default();
        items.extend(candidates(&snapshot, rows, alias, freshness));
    }
    items.sort_by(|a, b| {
        (a["freshness"] != "fresh")
            .cmp(&(b["freshness"] != "fresh"))
            .then_with(|| inspection_order(a, b))
    });
    let total = items.len();
    let fresh_total = items
        .iter()
        .filter(|row| row["freshness"] == "fresh")
        .count();
    items.truncate(8);
    let mut value = json!({"availability":if eligible.is_empty() { "none" } else if covered == 0 { "unknown" }
        else if covered < eligible.len() { "partial" } else if fresh_total > 0 { "available" } else { "none" },
        "items":items,"sources":sources,"direct_total":direct,"excluded_dependencies":dependencies,
        "total":total,"fresh_total":fresh_total,"truncated":total > 8,
        "next_action":"Availability is cached and compatibility is unchecked. Refresh with pack search for the relevant enrolled source. Only prepare pack_update when the user requested updates; review before applying."});
    bound_output(&mut value);
    // Retain fresh notice candidates through both limits before restoring the
    // inspection's source/pack presentation order.
    if let Some(items) = value["items"].as_array_mut() {
        items.sort_by(inspection_order);
    }
    value
}

fn inspection_order(a: &Value, b: &Value) -> std::cmp::Ordering {
    (a["source_identity"].as_str(), a["id"].as_str())
        .cmp(&(b["source_identity"].as_str(), b["id"].as_str()))
}

/// Drop optional rows before metadata. Coverage stays explicit even when
/// source identities alone exhaust the advisory output budget.
fn bound_output(value: &mut Value) {
    while !crate::json::fits(&value, OUTPUT_BYTES) {
        value["truncated"] = json!(true);
        if value["sources"]
            .as_array_mut()
            .is_some_and(|rows| rows.pop().is_some())
        {
            continue;
        }
        if value["items"]
            .as_array_mut()
            .is_none_or(|rows| rows.pop().is_none())
        {
            break;
        }
    }
}

fn candidates(snapshot: &Snapshot, rows: &[&Value], alias: &str, freshness: &str) -> Vec<Value> {
    let mut heads: BTreeMap<_, &Entry> = BTreeMap::new();
    for head in snapshot
        .heads
        .iter()
        .filter(|head| head.version.pre.is_empty())
    {
        let selected = heads.entry(&head.id).or_insert(head);
        if head.version.cmp_precedence(&selected.version).is_gt() {
            *selected = head;
        }
    }
    rows.iter().filter_map(|row| {
        let version = row["version"].as_str()?.parse::<semver::Version>().ok()?;
        let id = row["id"].as_str()?.parse::<canonic_registry_kit::names::PackId>().ok()?;
        let head = heads.get(&id).filter(|head| head.version.cmp_precedence(&version).is_gt())?;
        Some(json!({"selector":row["selector"],"id":row["id"],"source_identity":snapshot.source.identity(),
            "reference":format!("{alias}/{}", head.id),"installed_version":version,"available_version":head.version,
            "observed_at":timestamp::format_utc(snapshot.observed_at / 1000),"expires_at":snapshot.expires_at,
            "freshness":freshness,"compatibility":"unchecked"}))
    }).collect()
}

fn valid(snapshot: &Snapshot) -> bool {
    snapshot.schema_version == 1
        && snapshot.observed_at != 0
        && snapshot.heads.len() <= canonic_registry_kit::catalog::MAX_HEADS
        && snapshot.checkpoint.releases.len() <= canonic_registry_kit::catalog::MAX_HEADS
        && canonic_registry_kit::digest::is_sha256_hex(&snapshot.checkpoint.digest)
        && (!snapshot.source.is_signed() || snapshot.expires_at.is_some())
        && snapshot
            .expires_at
            .as_deref()
            .is_none_or(timestamp::is_utc_seconds)
        && snapshot.heads.iter().all(|head| {
            canonic_registry_kit::digest::is_sha256_hex(&head.sha256)
                && canonic_registry_kit::names::is_artifact_path(&head.path)
                && snapshot
                    .checkpoint
                    .releases
                    .get(&format!("{}@{}", head.id, head.version))
                    == Some(&head.sha256)
        })
}

fn against_project(snapshot: &Snapshot, state: &State, identity: &str) -> bool {
    if let Some(value) = state.registry_checkpoints.get(identity) {
        let Ok(previous) = serde_json::from_value::<Checkpoint>(value.clone()) else {
            return false;
        };
        if !follows(&snapshot.checkpoint, &previous) {
            return false;
        }
    }
    state.operator["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["source_identity"] == identity)
        .all(|row| {
            snapshot
                .checkpoint
                .releases
                .get(&format!(
                    "{}@{}",
                    row["id"].as_str().unwrap_or_default(),
                    row["version"].as_str().unwrap_or_default()
                ))
                .is_none_or(|digest| row["digest"] == *digest)
        })
}

fn follows(next: &Checkpoint, previous: &Checkpoint) -> bool {
    previous.check_observation(next).is_ok()
}

fn freshness(snapshot: &Snapshot, now: u64) -> &'static str {
    if now == 0 || snapshot.observed_at > now {
        return "unavailable";
    }
    if snapshot.expires_at.as_deref().is_some_and(|expiry| {
        timestamp::is_expired(expiry, UNIX_EPOCH + std::time::Duration::from_millis(now))
            != Some(false)
    }) {
        return "expired";
    }
    if now - snapshot.observed_at >= DAY_MS {
        "stale"
    } else {
        "fresh"
    }
}

/// Launch preference is intentionally absent from hooks and enforcement.
pub(crate) fn enabled() -> Result<bool, super::Error> {
    match std::env::var("SHIELD_PACK_UPDATE_CHECK") {
        Err(std::env::VarError::NotPresent) => Ok(true),
        Ok(value) if value == "cached" => Ok(true),
        Ok(value) if value == "off" => Ok(false),
        _ => Err(super::Error::Invalid(
            "SHIELD_PACK_UPDATE_CHECK must be off or cached",
        )),
    }
}

#[cfg(test)]
mod tests;
