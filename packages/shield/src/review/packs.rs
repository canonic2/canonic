//! Retained selections and reviewed acquisition; source aliases are discovery only.
pub(super) mod bootstrap;
mod closure;
mod connections;
pub(super) mod settings;
use super::{project as state, registries};
use crate::review::inspection::Settings;
use crate::{
    configuration::Bundle,
    policy::{self, Profile},
    review::{Context, Error},
};
use canonic_registry::{
    acquisition::Fetcher,
    catalog::{Catalog, Entry, LookupError},
    names::{self, PackId},
    registrations::Effective,
    selector::{self, Installed, InstalledSelector, Reference, Selector, SelectorError},
    source::Source,
};
use serde_json::{Value, json};

pub(super) fn summaries(meta: &Value) -> Value {
    json!({"packs":meta["installed"]})
}
/// The alias, source and catalog reference an operator's pack argument names.
fn resolve(context: &Context, input: &str) -> Result<(String, Source, Reference), Error> {
    let effective = registries::effective(Some(context))?;
    resolve_effective(&effective, input)
}
fn resolve_effective(
    effective: &Effective,
    input: &str,
) -> Result<(String, Source, Reference), Error> {
    let reference = match selector::parse(input, |alias| effective.get(alias).is_some()) {
        Ok(Selector::Catalog(reference)) => reference,
        Err(SelectorError::Limit) => return Err(policy::Error::Limit.into()),
        Err(SelectorError::Version) => return Err(Error::Invalid("exact pack version required")),
        Ok(Selector::Installed(_)) | Err(_) => {
            return Err(Error::Invalid("invalid pack reference"));
        }
    };
    let (alias, source) = registry(effective, reference.alias.as_deref())?;
    Ok((alias, source, reference))
}
/// The alias and source of an explicit alias, or of the default.
fn registry(effective: &Effective, alias: Option<&str>) -> Result<(String, Source), Error> {
    let alias = alias
        .unwrap_or_else(|| effective.default_alias())
        .to_owned();
    let source = effective
        .select(Some(&alias))
        .cloned()
        .map_err(|_| Error::Invalid("registry is not enrolled"))?;
    Ok((alias, source))
}
/// A search query's registry and substring. Segments are split like a
/// reference but not validated, so partial and empty names still match.
fn search_query(effective: &Effective, input: &str) -> Result<(Option<String>, String), Error> {
    if input.len() > names::MAX_TEXT {
        return Err(policy::Error::Limit.into());
    }
    let name = match input.rsplit_once('@') {
        Some((name, version)) => {
            semver::Version::parse(version)
                .map_err(|_| Error::Invalid("exact pack version required"))?;
            name
        }
        None => input,
    };
    Ok(match name.split('/').collect::<Vec<_>>().as_slice() {
        [name] => (None, (*name).to_owned()),
        [alias, name] if effective.get(alias).is_some() => {
            (Some((*alias).to_owned()), (*name).to_owned())
        }
        [publisher, name] => (None, format!("{publisher}/{name}")),
        [alias, publisher, name] => (Some((*alias).to_owned()), format!("{publisher}/{name}")),
        _ => return Err(Error::Invalid("invalid pack reference")),
    })
}
struct Acquired {
    source: Source,
    entry: Entry,
    bytes: String,
    alias: String,
    catalog: Catalog,
}
fn acquire(
    context: &Context,
    before: &crate::review::model::State,
    input: &str,
) -> Result<Acquired, Error> {
    // The proposal already owns a validated capture. Resolve its registrations
    // from that capture; final publication still checks the current metadata.
    let effective = registries::effective_from_metadata(&before.operator)?;
    let (alias, source, reference) = resolve_effective(&effective, input)?;
    let catalog = registries::checked_catalog(context, &source)?;
    let id = catalog
        .lookup(&reference.pack)
        .map_err(|error| match error {
            LookupError::Ambiguous(ids) => catalog_ambiguity(&alias, &ids),
            LookupError::Missing => Error::Missing,
        })?;
    let (entry, bytes) = if let Some(version) = reference.version.as_ref() {
        let entry = catalog.release(&id, version).ok_or(Error::Missing)?;
        (entry.clone(), artifact(&context.fetcher, &source, entry)?)
    } else {
        let mut selected = None;
        let mut published = false;
        for entry in catalog
            .releases(&id)
            .into_iter()
            .filter(|entry| entry.version.pre.is_empty())
        {
            published = true;
            if let Some(bytes) = candidate_artifact(&context.fetcher, &source, entry)? {
                selected = Some((entry.clone(), bytes));
                break;
            }
        }
        selected.ok_or_else(|| {
            if published {
                Error::UnsupportedReleases(id.to_string())
            } else {
                Error::Missing
            }
        })?
    };
    Ok(Acquired {
        source,
        entry,
        bytes,
        alias,
        catalog,
    })
}
fn artifact(fetcher: &Fetcher, source: &Source, entry: &Entry) -> Result<String, Error> {
    let bytes = registries::artifact(fetcher, source, entry)?;
    matches_entry(&policy::Pack::from_yaml(&bytes)?, entry)?;
    Ok(bytes)
}
/// A candidate release's verified bytes, or `None` when this Shield version
/// cannot read the release's policy format or is outside its engine range.
/// Acquisition, digest, malformed content and identity failures still refuse.
fn candidate_artifact(
    fetcher: &Fetcher,
    source: &Source,
    entry: &Entry,
) -> Result<Option<String>, Error> {
    let bytes = registries::artifact(fetcher, source, entry)?;
    match policy::Pack::from_yaml(&bytes) {
        Ok(pack) => {
            matches_entry(&pack, entry)?;
            Ok(Some(bytes))
        }
        Err(policy::Error::Syntax | policy::Error::Contract(_)) if unsupported_release(&bytes) => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}
/// The header fields that decide whether this engine admits a release at all,
/// read without the version-1 schema's field and content rules.
#[derive(serde::Deserialize)]
struct ReleaseHeader {
    schema_version: Value,
    kind: Value,
    #[serde(default)]
    engines: Value,
    /// The single engine range of releases published before `engines`.
    #[serde(default)]
    engine: Value,
    #[serde(default)]
    capabilities: Value,
}
/// Whether a release that failed validation declares a policy schema or kind
/// this engine does not read, a capability it does not implement, or an engine
/// range excluding this Shield version, in `engines.shield` or the older single
/// `engine` field. Any other failure is malformed content, not an unsupported
/// release.
fn unsupported_release(bytes: &str) -> bool {
    let Ok(header) = policy::parse::<ReleaseHeader>(bytes) else {
        return false;
    };
    if header.schema_version != json!(1) || header.kind != json!("pack") {
        return true;
    }
    let unknown_capability = header
        .capabilities
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|name| !policy::CAPABILITIES.contains(&name));
    if unknown_capability {
        return true;
    }
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"));
    header.engines["shield"]
        .as_str()
        .or_else(|| header.engine.as_str())
        .and_then(|range| semver::VersionReq::parse(range).ok())
        .zip(current.ok())
        .is_some_and(|(range, current)| !range.matches(&current))
}
fn matches_entry(pack: &policy::Pack, entry: &Entry) -> Result<(), Error> {
    if pack.id() != entry.id.to_string() || pack.version() != entry.version.to_string() {
        return Err(Error::Invalid("registry artifact identity mismatch"));
    }
    Ok(())
}
/// A new installed instance's stable `pack:` selector.
fn new_selector(
    before: &crate::review::model::State,
    identity: &str,
    id: &impl std::fmt::Display,
) -> Value {
    // Publication advances the retained sequence even when policy returns to
    // identical bytes. Removed selectors therefore cannot become new instances.
    let seed = json!([
        "installed-pack",
        before.project_id,
        before.sequence,
        identity,
        id.to_string()
    ]);
    json!(InstalledSelector::from_seed(seed.to_string().as_bytes()).as_str())
}
/// Current aliases and the source identity each is bound to, from captured
/// project metadata and the global registrations. Installed lookups need no
/// registry access, so registrations that do not resolve leave only the
/// retained aliases; acquisition still refuses them.
fn current_aliases(meta: &Value) -> Vec<(String, String)> {
    registries::effective_from_metadata(meta).map_or_else(
        |_| Vec::new(),
        |effective| {
            effective
                .iter()
                .map(|(alias, source, _)| (alias.to_owned(), source.identity().to_owned()))
                .collect()
        },
    )
}
/// The source identity a catalog-shaped `reference` names through the
/// captured registrations, or `None` for a `pack:` selector or a reference
/// those registrations cannot resolve, which match installed rows directly.
fn named_source(meta: &Value, reference: &str) -> Option<String> {
    if reference.starts_with("pack:") {
        return None;
    }
    let effective = registries::effective_from_metadata(meta).ok()?;
    let (_, source, _) = resolve_effective(&effective, reference).ok()?;
    Some(source.identity().to_owned())
}
/// The installed row `reference` names. A row answers to its retained source
/// alias and to every current alias bound to its exact source identity.
fn selection(
    rows: &[Value],
    aliases: &[(String, String)],
    reference: &str,
) -> Result<usize, Error> {
    let ids = rows
        .iter()
        .map(|row| {
            row["id"]
                .as_str()
                .and_then(|id| id.parse::<PackId>().ok())
                .ok_or(Error::Invalid("pack identity missing"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut entries = Vec::new();
    for (index, (row, id)) in rows.iter().zip(&ids).enumerate() {
        let selector = row["selector"].as_str().unwrap_or_default();
        let retained = row["source_alias"].as_str();
        entries.push((
            index,
            Installed {
                selector,
                alias: retained,
                id,
            },
        ));
        for (alias, identity) in aliases {
            if row["source_identity"] == identity.as_str() && retained != Some(alias.as_str()) {
                entries.push((
                    index,
                    Installed {
                        selector,
                        alias: Some(alias.as_str()),
                        id,
                    },
                ));
            }
        }
    }
    let matched = matching_rows(&entries, reference);
    match matched.as_slice() {
        [index] => Ok(*index),
        [] => Err(Error::Missing),
        _ => Err(Error::ambiguous(
            "installed reference is ambiguous",
            installed_alternatives(&entries, &matched),
        )),
    }
}
/// The distinct rows whose installed entries `reference` matches. A leading
/// alias of any entry scopes the match to that alias.
fn matching_rows(entries: &[(usize, Installed<'_>)], reference: &str) -> Vec<usize> {
    let name = reference.split('@').next().unwrap_or_default();
    let scope = name
        .split_once('/')
        .map(|(alias, _)| alias)
        .filter(|alias| entries.iter().any(|(_, i)| i.alias == Some(*alias)));
    let mut rows: Vec<usize> = entries
        .iter()
        .filter(|(_, item)| scope.is_none_or(|alias| item.alias == Some(alias)))
        .filter(|(_, item)| selector::find_installed([*item], reference).is_ok())
        .map(|(row, _)| *row)
        .collect();
    rows.dedup();
    rows
}
/// References naming exactly one of the matched rows: an alias-qualified name
/// when it is unique, else the row's selector.
fn installed_alternatives(entries: &[(usize, Installed<'_>)], matched: &[usize]) -> Vec<String> {
    matched
        .iter()
        .map(|row| {
            let own = || entries.iter().filter(|(index, _)| index == row);
            own()
                .filter_map(|(_, item)| item.alias.map(|alias| format!("{alias}/{}", item.id)))
                .find(|qualified| matching_rows(entries, qualified) == [*row])
                .or_else(|| own().next().map(|(_, item)| item.selector.to_owned()))
                .unwrap_or_default()
        })
        .collect()
}
/// Fully qualified catalog references for an ambiguous short name.
fn catalog_ambiguity(alias: &str, ids: &[PackId]) -> Error {
    Error::ambiguous(
        "pack name is ambiguous",
        ids.iter().map(|id| format!("{alias}/{id}")).collect(),
    )
}
pub(super) fn list(context: &Context) -> Result<Value, Error> {
    let current = state::load(context)?;
    let mut value = summaries(&state::metadata(&current));
    value["policy_revision"] = json!(current.policy_revision);
    Ok(value)
}

pub(super) fn show(context: &Context, reference: &str) -> Result<Value, Error> {
    let current = state::load(context)?;
    let meta = state::metadata(&current);
    let rows = meta["installed"]
        .as_array()
        .ok_or(Error::Invalid("installed packs missing"))?;
    let index = selection(rows, &current_aliases(&meta), reference)?;
    let artifact: Value = crate::policy::parse(
        current.candidates["project"]["packs"][index]
            .as_str()
            .ok_or(Error::Invalid("pack bytes missing"))?,
    )?;
    Ok(json!({"policy_revision":current.policy_revision,"pack":rows[index],"artifact":artifact}))
}
pub(super) fn search(context: &Context, query: &str) -> Result<Value, Error> {
    let profiles = json!(["loose", "recommended", "strict"]);
    let effective = registries::effective(Some(context))?;
    let (alias, name) = search_query(&effective, query)?;
    let (alias, source) = registry(&effective, alias.as_deref())?;
    let catalog = registries::checked_catalog(context, &source)?;
    Ok(
        json!({"source":alias,"source_identity":source.identity(),"packs":catalog.search(&name),"profiles":profiles}),
    )
}

pub(super) fn info(context: &Context, query: &str) -> Result<Value, Error> {
    let profiles = json!(["loose", "recommended", "strict"]);
    let (alias, source, reference) = resolve(context, query)?;
    let catalog = registries::checked_catalog(context, &source)?;
    let version = reference.version.as_ref();
    let mut result =
        json!({"source":alias,"source_identity":source.identity(),"packs":[],"profiles":profiles});
    let id = match catalog.lookup(&reference.pack) {
        Ok(id) => id,
        Err(LookupError::Missing) => return Err(Error::Missing),
        // An exact version can name the one publisher that lists it.
        Err(LookupError::Ambiguous(ids)) => {
            let ids_all = ids.clone();
            let listed: Vec<_> = ids
                .into_iter()
                .filter(|id| version.is_some_and(|v| catalog.release(id, v).is_some()))
                .collect();
            match <[PackId; 1]>::try_from(listed) {
                Ok([id]) => id,
                Err(listed) if listed.is_empty() && version.is_some() => {
                    return Err(Error::Missing);
                }
                Err(listed) => {
                    // Without a listing version, every publisher is an alternative.
                    let ids = if listed.is_empty() { &ids_all } else { &listed };
                    return Err(catalog_ambiguity(&alias, ids));
                }
            }
        }
    };
    let entries: Vec<_> = catalog
        .entries()
        .iter()
        .filter(|e| e.id == id && version.is_none_or(|v| e.version == *v))
        .collect();
    result["packs"] = json!(entries);
    let selected = catalog.select(&id, version).ok_or(Error::Missing)?;
    let bytes = artifact(&context.fetcher, &source, selected)?;
    let document: Value = crate::policy::parse(&bytes)?;
    result["selected_release"] = json!(selected);
    result["description"] = document["description"].clone();
    for field in [
        "invocations",
        "command_sets",
        "variables",
        "preconditions",
        "dependencies",
        "mcp_integrations",
    ] {
        if let Some(value) = document.get(field) {
            result[field] = value.clone();
        }
    }
    result["compatibility"] = json!({"engines":document["engines"],"supported":true,"capabilities":document["capabilities"]});
    result["rules"] = json!(
        document["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                let mut rule =
                    crate::review::inspection::public_pack_rule(row, &json!("recommended"));
                rule["profile_decisions"] = row["decision"].clone();
                rule
            })
            .collect::<Vec<_>>()
    );
    Ok(result)
}
pub(super) fn rebuild(
    candidate: &Value,
    rows: &[Value],
    bytes: &[String],
    rules: Option<&str>,
) -> Result<Value, Error> {
    let previous = crate::configuration::layers::merged(candidate)?;
    let mut selections = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let mut selection = json!({"id":row["id"],"registry_identity":row["source_identity"],"version":row["version"],"profile":row["profile"],"path":format!("cache/{}.yml",row["digest"].as_str().unwrap_or_default())});
        let artifact = bytes.get(index).map(String::as_str);
        let earlier = earlier(row, &previous);
        let binding = binding(row, earlier, artifact)?;
        if !binding.variables.is_empty() {
            selection["variables"] = json!(binding.variables);
        }
        if !binding.overrides.is_empty() {
            selection["overrides"] = json!(binding.overrides);
        }
        if row["layer"] == "local"
            || (row.get("layer").is_none() && earlier.is_some_and(|e| e["layer"] == "local"))
        {
            selection["layer"] = json!("local");
        } else if let Some(overlay) = overlay(row, earlier, artifact)? {
            selection["local"] = json!(overlay);
        }
        selections.push(selection);
    }
    // Re-adding a shared selection re-enables it; other disabled ones are kept.
    let mut disabled = Vec::new();
    let mut retained = Vec::new();
    for (index, entry) in previous["disabled"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if !rows.iter().any(|row| same_selection(entry, row)) {
            disabled.push(entry.clone());
            retained.push(
                candidate["disabled"][index]
                    .as_str()
                    .ok_or(Error::Invalid("disabled pack bytes missing"))?
                    .to_owned(),
            );
        }
    }
    let mut config = json!({"schema_version":1,"packs":selections});
    for field in [
        "access_paths",
        "local_access_paths",
        "local_rules",
        "mcp",
        "local_mcp",
    ] {
        if let Some(value) = previous.get(field) {
            config[field] = value.clone();
        }
    }
    connections::retain(candidate, &mut config)?;
    if !disabled.is_empty() {
        config["disabled"] = json!(disabled);
    }
    let rules = rules.or_else(|| candidate["rules"].as_str());
    if rules.is_some() {
        config["rules"] = json!("rules.yml");
    }
    Ok(json!(Bundle::revise(
        candidate,
        &config,
        bytes,
        rules,
        candidate["local_rules"].as_str(),
        &retained,
    )?))
}

fn same_selection(entry: &Value, row: &Value) -> bool {
    entry["id"] == row["id"]
        && (entry["registry_identity"].is_null()
            || entry["registry_identity"] == row["source_identity"])
}

/// The previous configuration's entry for a row's selection.
fn earlier<'a>(row: &Value, previous: &'a Value) -> Option<&'a Value> {
    previous["packs"]
        .as_array()
        .and_then(|packs| packs.iter().find(|p| same_selection(p, row)))
}

/// A shared selection's local overlay: the row's own when present, otherwise
/// the previous one, without names and IDs the artifact no longer declares.
fn overlay(
    row: &Value,
    earlier: Option<&Value>,
    artifact: Option<&str>,
) -> Result<Option<crate::configuration::layers::Overlay>, Error> {
    let source = match row.get("local") {
        Some(value) => value.clone(),
        None => earlier.map_or(Value::Null, |e| e["local"].clone()),
    };
    if source.is_null() {
        return Ok(None);
    }
    let mut overlay: crate::configuration::layers::Overlay = serde_json::from_value(source)
        .map_err(|_| Error::Invalid("invalid local variables or overrides"))?;
    if row.get("local").is_none()
        && let Some(artifact) = artifact
    {
        let mut kept = policy::Binding {
            variables: overlay.variables.clone(),
            overrides: overlay.overrides.clone(),
        };
        policy::retain_declared(artifact, &mut kept)?;
        let declared = policy::declarations(artifact)?;
        overlay.variables = kept.variables;
        overlay.overrides = kept.overrides;
        overlay.unset.retain(|name| declared.contains_key(name));
    }
    Ok((!overlay.is_empty()).then_some(overlay))
}
/// A row's variable values and overrides: its own when present, otherwise the
/// previous configuration's for the same selection. Names and IDs the pack
/// artifact no longer declares are dropped; review reports the difference.
fn binding(
    row: &Value,
    earlier: Option<&Value>,
    artifact: Option<&str>,
) -> Result<policy::Binding, Error> {
    let fields = |from: &Value| {
        let mut source = json!({});
        for field in ["variables", "overrides"] {
            if !from[field].is_null() {
                source[field] = from[field].clone();
            }
        }
        serde_json::from_value::<policy::Binding>(source)
            .map_err(|_| Error::Invalid("invalid pack variables or overrides"))
    };
    // The operator's own values stay as given, so capture reports invalid ones.
    if row.get("variables").is_some() || row.get("overrides").is_some() {
        return fields(row);
    }
    let mut binding = earlier.map_or_else(|| Ok(policy::Binding::default()), fields)?;
    if let Some(artifact) = artifact {
        policy::retain_declared(artifact, &mut binding)?;
    }
    Ok(binding)
}
/// A candidate whose selection `reference` has the binding `change` makes.
pub(super) fn prepare_configure(
    before: &crate::review::model::State,
    reference: &str,
    change: impl FnOnce(&str, &mut Settings) -> Result<(), Error>,
) -> Result<(Value, Value), Error> {
    let mut meta = state::metadata(before);
    let aliases = current_aliases(&meta);
    let candidate = &before.candidates["project"];
    let artifacts: Vec<String> = serde_json::from_value(candidate["packs"].clone())
        .map_err(|_| Error::Invalid("pack artifacts missing"))?;
    let rows = meta["installed"]
        .as_array_mut()
        .ok_or(Error::Invalid("installed packs missing"))?;
    let index = selection(rows, &aliases, reference)?;
    let mut settings = crate::review::inspection::selection_settings(candidate, index)?;
    change(&artifacts[index], &mut settings)?;
    bind(&mut rows[index], &settings);
    let candidate = rebuild(candidate, rows, &artifacts, None)?;
    unbind(rows);
    Ok((candidate, meta))
}

/// Carry a selection's values and overrides, by layer, into the next rebuild.
pub(super) fn bind(row: &mut Value, settings: &Settings) {
    row["variables"] = json!(settings.shared.variables);
    row["overrides"] = json!(settings.shared.overrides);
    if let Some(overlay) = &settings.local {
        row["local"] = json!(overlay);
    }
}

/// Configuration owns bindings after a rebuild; rows never retain them, so a
/// reviewed manual edit of config.yml stays authoritative.
pub(super) fn unbind(rows: &mut [Value]) {
    for row in rows {
        if let Some(fields) = row.as_object_mut() {
            fields.remove("variables");
            fields.remove("overrides");
            fields.remove("local");
        }
    }
}

/// Add a selection; with `local`, the selection and dependencies it newly
/// brings in belong to the local layer. Without `profile`, `choose` supplies
/// a new selection's profile from its pack id and version; an installed
/// selection keeps its own.
pub(super) fn prepare_add(
    context: &Context,
    before: &crate::review::model::State,
    reference: &str,
    profile: Option<Profile>,
    local: bool,
    choose: impl FnOnce(&str, &str) -> Result<Profile, Error>,
) -> Result<(Value, Value), Error> {
    let (candidate, mut meta) = prepare_shared_add(context, before, reference, profile, choose)?;
    if !local {
        return Ok((candidate, meta));
    }
    let known: Vec<Value> = state::metadata(before)["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| row["selector"].clone())
        .collect();
    let rows = meta["installed"]
        .as_array_mut()
        .ok_or(Error::Invalid("installed packs missing"))?;
    let mut added = false;
    for row in rows.iter_mut() {
        if !known.contains(&row["selector"]) {
            row["layer"] = json!("local");
            added = true;
        }
    }
    if !added {
        return Err(Error::Invalid(
            "the pack is already installed; use shield pack configure --local for personal settings",
        ));
    }
    let artifacts: Vec<String> = serde_json::from_value(candidate["packs"].clone())
        .map_err(|_| Error::Invalid("pack artifacts missing"))?;
    let candidate = rebuild(&candidate, rows, &artifacts, None)?;
    Ok((candidate, meta))
}

fn prepare_shared_add(
    context: &Context,
    before: &crate::review::model::State,
    reference: &str,
    profile: Option<Profile>,
    choose: impl FnOnce(&str, &str) -> Result<Profile, Error>,
) -> Result<(Value, Value), Error> {
    let mut meta = state::metadata(before);
    let pinned = reference.contains('@');
    if !pinned {
        let aliases = current_aliases(&meta);
        let named = named_source(&meta, reference);
        let rows = meta["installed"]
            .as_array_mut()
            .ok_or(Error::Invalid("installed packs missing"))?;
        // A catalog reference names one source: the default for a bare name,
        // else its alias's. Only rows retained from that source answer to it,
        // so another source's same-named row is never re-rooted in its place.
        let candidates: Vec<usize> = (0..rows.len())
            .filter(|index| {
                named
                    .as_deref()
                    .is_none_or(|identity| rows[*index]["source_identity"] == identity)
            })
            .collect();
        let scoped: Vec<Value> = candidates
            .iter()
            .map(|index| rows[*index].clone())
            .collect();
        let selected = match selection(&scoped, &aliases, reference) {
            Ok(index) => Some(candidates[index]),
            Err(Error::Missing) => None,
            Err(error) => return Err(error),
        };
        if let Some(index) = selected {
            if let Some(profile) = profile {
                rows[index]["profile"] = json!(profile);
            }
            let artifacts: Vec<String> =
                serde_json::from_value(before.candidates["project"]["packs"].clone())
                    .map_err(|_| Error::Invalid("pack artifacts missing"))?;
            rows[index]["direct"] = json!(true);
            return closure::prepare(context, before, meta, &artifacts, index, false, None);
        }
        if reference.starts_with("pack:") {
            return Err(Error::Missing);
        }
    }
    let Acquired {
        source,
        entry,
        bytes,
        alias,
        catalog,
    } = acquire(context, before, reference)?;
    let rows = meta["installed"]
        .as_array_mut()
        .ok_or(Error::Invalid("installed packs missing"))?;
    let mut artifacts: Vec<String> =
        serde_json::from_value(before.candidates["project"]["packs"].clone())
            .map_err(|_| Error::Invalid("pack artifacts missing"))?;
    let index = rows
        .iter()
        .position(|r| r["source_identity"] == source.identity() && r["id"] == json!(entry.id));
    if let Some(index) = index.filter(|_| !pinned) {
        // An installed selection added without a version keeps its version and pin mode.
        if let Some(profile) = profile {
            rows[index]["profile"] = json!(profile);
        }
        rows[index]["direct"] = json!(true);
        return closure::prepare(context, before, meta, &artifacts, index, false, None);
    }
    let (selector, chosen) = if let Some(index) = index {
        if rows[index]["version"] == json!(entry.version) && artifacts[index] != bytes {
            return Err(Error::Invalid("same pack release has different bytes"));
        }
        (
            rows[index]["selector"].clone(),
            profile.map_or_else(|| rows[index]["profile"].clone(), |p| json!(p)),
        )
    } else {
        (
            new_selector(before, source.identity(), &entry.id),
            json!(match profile {
                Some(profile) => profile,
                None => choose(&entry.id.to_string(), &entry.version.to_string())?,
            }),
        )
    };
    let row = json!({"selector":selector,"id":entry.id,"version":entry.version,"profile":chosen,"source_alias":alias,"source_identity":source.identity(),"source":source,"digest":entry.sha256,"pinned":pinned,"direct":true});
    let target = index.unwrap_or(rows.len());
    let floating = index.is_none() && !pinned;
    if let Some(index) = index {
        rows[index] = row;
        artifacts[index] = bytes;
    } else {
        rows.push(row);
        artifacts.push(bytes);
    }
    closure::prepare(
        context,
        before,
        meta,
        &artifacts,
        target,
        floating,
        Some(catalog),
    )
}
pub(super) fn prepare_remove(
    before: &crate::review::model::State,
    reference: &str,
) -> Result<(Value, Value), Error> {
    closure::mutations::remove(before, reference)
}
pub(super) fn prepare_update(
    context: &Context,
    before: &crate::review::model::State,
    reference: Option<&str>,
) -> Result<(Value, Value), Error> {
    closure::mutations::update(context, before, reference)
}
/// The pinned roots an update of `meta`'s installed packs leaves unchanged,
/// for the operator's review and the agent's proposal result.
pub(super) fn update_skipped(meta: &Value, reference: Option<&str>) -> Result<Vec<Value>, Error> {
    let rows: Vec<Value> = serde_json::from_value(meta["installed"].clone())
        .map_err(|_| Error::Invalid("installed packs missing"))?;
    closure::mutations::skipped_pinned(&rows, &current_aliases(meta), reference)
}

pub(super) fn prepare_change(
    context: &Context,
    before: &super::model::State,
    change: &super::changes::Change,
) -> Result<(Value, Value), Error> {
    match change {
        super::changes::Change::PackAdd {
            reference,
            profile,
            layer,
        } => prepare_add(
            context,
            before,
            reference,
            *profile,
            *layer == Some(super::changes::Layer::Local),
            |_, _| Ok(crate::policy::Profile::Recommended),
        ),
        super::changes::Change::PackUpdate { reference } => {
            prepare_update(context, before, reference.as_deref())
        }
        super::changes::Change::PackRemove { reference } => prepare_remove(before, reference),
        super::changes::Change::PackConfigure {
            reference,
            set,
            unset,
            overrides,
            clear_override,
            layer,
        } => {
            if set.is_empty()
                && unset.is_empty()
                && overrides.is_empty()
                && clear_override.is_empty()
            {
                return Err(Error::Invalid("pack_configure names at least one change"));
            }
            let local = *layer == Some(super::changes::Layer::Local);
            prepare_configure(before, reference, |artifact, settings| {
                settings.edit(local, |binding| {
                    settings::assign(artifact, binding, set, unset)?;
                    binding.overrides.extend(overrides.clone());
                    for id in clear_override {
                        binding.overrides.remove(id);
                    }
                    Ok(())
                })
            })
        }
        _ => Err(Error::Invalid("pack change required")),
    }
}
