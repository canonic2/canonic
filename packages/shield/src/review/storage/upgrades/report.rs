//! What the one conversion reports: the packs and rules that differ between
//! the policy copy an earlier checkpoint approved and the policy files, which
//! decide requests from now on.
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// How long status and doctor show the conversion notice.
const NOTICE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// The differences between the approved copy in `raw`, an earlier checkpoint,
/// and the policy files under `root`.
pub(in crate::review) fn differences(root: &Path, raw: &Value) -> Value {
    let Some(old) = approved(root, raw) else {
        return json!({"compared":false,"reason":"the earlier approved copy is not readable"});
    };
    let files = match crate::review::integrations::project_sources::capture(
        &crate::layout::config_file(root),
    ) {
        Ok(bundle) => json!(bundle),
        Err(_) => {
            return json!({"compared":false,"reason":"the policy files are not readable"});
        }
    };
    let packs = changed(&packs(&old), &packs(&files));
    let rules = changed(&rules(&old), &rules(&files));
    json!({"compared":true,"same":packs.is_empty() && rules.is_empty(),"packs":packs,"rules":rules})
}

/// The notice status and doctor show for [`NOTICE_MS`] after the conversion.
pub(in crate::review) fn notice(differences: &Value) -> Value {
    json!({"converted_at_ms":crate::review::clock::best_effort_ms(),"policy":differences,
        "detail":"Shield now enforces the files in .shield/ directly. Settings this folder kept in its review state moved into .shield/config.local.yml."})
}

/// The conversion notice while it is recent, for status and doctor.
pub(in crate::review) fn recent(operator: &Value) -> Option<Value> {
    let notice = operator.get("upgrade_notice")?;
    let converted = notice["converted_at_ms"].as_u64()?;
    (crate::review::clock::best_effort_ms() <= converted.saturating_add(NOTICE_MS))
        .then(|| notice.clone())
}

/// Remove the per-folder records only the deleted layers used. Failures leave
/// them in place; nothing reads them.
pub(in crate::review) fn retire_folder_records(root: &Path) {
    let mut budget = super::super::retired::BATCH;
    for name in ["configuration-changes", "reviewed-policy", "portability"] {
        // `.shield` and the state folder are checked on the way, so a linked
        // folder never points the removal elsewhere. The state folder may
        // still carry its earlier name.
        for state in ["state", "review"] {
            let _ =
                super::super::retired::remove_within(root, &[".shield", state, name], &mut budget);
        }
    }
}

/// The policy copy an earlier checkpoint approved, when it still holds it
/// inline or its retained source is still readable.
fn approved(root: &Path, raw: &Value) -> Option<Value> {
    let candidate = [
        &raw["active"]["candidates"]["project"],
        &raw["candidates"]["project"],
    ]
    .into_iter()
    .find(|candidate| candidate.is_object())?;
    if candidate["config"].is_string() {
        return Some(candidate.clone());
    }
    let sources = crate::layout::state(root).join("sources");
    if candidate["kind"] == "retained-source-v2" {
        return manifest(&sources, digest(&candidate["manifest_digest"])?);
    }
    let bytes = verified(&sources, &format!("{}.json", digest(&candidate["digest"])?))?;
    crate::json::parse(&bytes)
        .ok()
        .filter(|source| source["config"].is_string())
}

fn digest(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A retained source file's bytes, when they still match the digest it is
/// named by.
fn verified(sources: &Path, name: &str) -> Option<Vec<u8>> {
    let digest = name.split('.').next()?;
    let bytes = std::fs::read(sources.join(name)).ok()?;
    (crate::policy::fingerprint(&bytes) == digest).then_some(bytes)
}

/// A candidate an earlier release stored as a manifest of digest-named blobs.
fn manifest(sources: &Path, digest: &str) -> Option<Value> {
    let manifest =
        crate::json::parse(&verified(sources, &format!("{digest}.manifest.json"))?).ok()?;
    let text = |reference: &Value| -> Option<String> {
        let bytes = verified(
            sources,
            &format!("{}.blob", self::digest(&reference["digest"])?),
        )?;
        String::from_utf8(bytes).ok()
    };
    let mut candidate = serde_json::Map::new();
    for (name, field) in manifest["fields"].as_object()? {
        let value = &field["value"];
        let decoded = match field["encoding"].as_str()? {
            "literal" => value.clone(),
            "text" if value.is_null() => Value::Null,
            "text" => json!(text(value)?),
            "json" => crate::json::parse(text(value)?.as_bytes()).ok()?,
            "texts" => json!(
                value
                    .as_array()?
                    .iter()
                    .map(text)
                    .collect::<Option<Vec<_>>>()?
            ),
            _ => return None,
        };
        candidate.insert(name.clone(), decoded);
    }
    Some(Value::Object(candidate)).filter(|candidate| candidate["config"].is_string())
}

/// Each selected pack by identity, with its version and profile.
fn packs(candidate: &Value) -> BTreeMap<String, Value> {
    let Ok(merged) = crate::configuration::layers::merged(candidate) else {
        return BTreeMap::new();
    };
    merged["packs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            let id = match row["registry_identity"].as_str() {
                Some(registry) => format!("{registry}/{}", row["id"].as_str().unwrap_or("?")),
                None => row["id"].as_str().unwrap_or("?").to_owned(),
            };
            let layer = if row["layer"] == "local" { "local" } else { "project" };
            (
                format!("{id} ({layer})"),
                json!({"version":row["version"],"profile":row["profile"],
                    "variables":row["variables"],"overrides":row["overrides"],"local":row["local"]}),
            )
        })
        .collect()
}

/// Each custom rule by layer and identifier.
fn rules(candidate: &Value) -> BTreeMap<String, Value> {
    let mut rules = BTreeMap::new();
    for (field, layer) in [("rules", "project"), ("local_rules", "local")] {
        let Some(source) = candidate[field].as_str() else {
            continue;
        };
        let Ok(document) = crate::policy::parse::<Value>(source) else {
            continue;
        };
        for rule in document["rules"].as_array().into_iter().flatten() {
            if let Some(id) = rule["id"].as_str() {
                rules.insert(format!("{id} ({layer})"), rule.clone());
            }
        }
    }
    rules
}

/// Names whose value was added, removed or changed.
fn changed(old: &BTreeMap<String, Value>, new: &BTreeMap<String, Value>) -> Vec<Value> {
    let mut rows = Vec::new();
    for (name, value) in old {
        match new.get(name) {
            None => rows.push(json!({"name":name,"change":"removed"})),
            Some(current) if current != value => {
                rows.push(json!({"name":name,"change":"changed"}));
            }
            Some(_) => {}
        }
    }
    for name in new.keys().filter(|name| !old.contains_key(*name)) {
        rows.push(json!({"name":name,"change":"added"}));
    }
    rows
}
