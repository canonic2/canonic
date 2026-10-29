//! Prepares verified enrolled-source dependency candidates, retaining exact identities.
pub(super) mod mutations;
use super::{candidate_artifact, new_selector, rebuild, registries};
use crate::{
    policy::{self, Pack, Profile},
    review::{Context, Error, model::State},
};
use canonic_registry::{
    catalog::Catalog,
    registrations::{Effective, Scope},
    resolution::{self, Instance, Root},
    source::Source,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

type Release = resolution::Release<Profile>;
type Selection<'a> = resolution::Selection<'a, Profile>;
type Artifacts = BTreeMap<(Instance, semver::Version), (Value, String)>;

struct Acquisition {
    pending: BTreeSet<Instance>,
    locked: BTreeSet<Instance>,
    minimum_versions: BTreeMap<Instance, semver::Version>,
    offline: bool,
    catalogs: BTreeMap<String, Catalog>,
}

fn identity(row: &Value) -> Result<Instance, Error> {
    Ok(Instance {
        registry: row["source_identity"]
            .as_str()
            .ok_or(Error::Invalid("source identity missing"))?
            .into(),
        pack: row["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .ok_or(Error::Invalid("pack identity missing"))?,
    })
}

fn release(row: &Value, bytes: &str) -> Result<Release, Error> {
    let pack = Pack::from_yaml(bytes)?;
    if pack.id() != row["id"]
        || pack.version() != row["version"]
        || row["digest"].as_str().is_none_or(|digest| {
            policy::fingerprint(bytes.as_bytes()) != digest.to_ascii_lowercase()
        })
    {
        return Err(Error::Invalid(
            "retained dependency artifact identity changed",
        ));
    }
    Ok(Release {
        instance: identity(row)?,
        version: pack
            .version()
            .parse()
            .map_err(|_| Error::Invalid("invalid release version"))?,
        dependencies: pack
            .dependencies()
            .iter()
            .map(policy::Declaration::parsed)
            .collect::<Result<_, _>>()
            .map_err(|_| Error::Invalid("invalid dependency declarations"))?,
    })
}

fn dependencies(release: &Release) -> impl Iterator<Item = Instance> + '_ {
    release.dependencies.iter().map(|edge| Instance {
        registry: edge
            .registry
            .clone()
            .unwrap_or_else(|| release.instance.registry.clone()),
        pack: edge.pack.clone(),
    })
}

pub(super) fn prepare(
    context: &Context,
    before: &State,
    mut meta: Value,
    bytes: &[String],
    target: usize,
    floating: bool,
    catalog: Option<Catalog>,
) -> Result<(Value, Value), Error> {
    let rows = meta["installed"]
        .as_array()
        .ok_or(Error::Invalid("installed packs missing"))?;
    if rows.len() != bytes.len() || target >= rows.len() {
        return Err(Error::Invalid(
            "installed dependency artifact set is incomplete",
        ));
    }
    let mut artifacts = BTreeMap::new();
    let mut releases = Vec::new();
    let mut roots = Vec::new();
    let mut enrolled = BTreeSet::new();
    for (index, (row, bytes)) in rows.iter().zip(bytes).enumerate() {
        let release = release(row, bytes)?;
        enrolled.insert(release.instance.registry.clone());
        if row["direct"] != false || index == target {
            roots.push(Root {
                instance: release.instance.clone(),
                requirement: "*"
                    .parse()
                    .map_err(|_| Error::Invalid("version requirement"))?,
                pin: if floating && index == target {
                    None
                } else {
                    Some(release.version.clone())
                },
                level: serde_json::from_value(row["profile"].clone())
                    .map_err(|_| Error::Invalid("profile invalid"))?,
            });
        }
        artifacts.insert(
            (release.instance.clone(), release.version.clone()),
            (row.clone(), bytes.clone()),
        );
        releases.push(release);
    }
    if !releases.iter().any(|r| !r.dependencies.is_empty()) {
        let candidate = rebuild(&before.candidates["project"], rows, bytes, None)?;
        return Ok((candidate, meta));
    }
    let target_identity = identity(&rows[target])?;
    let offline = catalog.is_none();
    let catalogs = catalog
        .map(|catalog| BTreeMap::from([(target_identity.registry.clone(), catalog)]))
        .unwrap_or_default();
    let mut pending: BTreeSet<_> = dependencies(&releases[target]).collect();
    if floating {
        pending.insert(target_identity.clone());
    }
    acquire_closure(
        context,
        None,
        rows,
        Acquisition {
            pending,
            locked: roots
                .iter()
                .filter(|r| r.instance != target_identity)
                .map(|r| r.instance.clone())
                .collect(),
            offline,
            catalogs,
            minimum_versions: BTreeMap::new(),
        },
        &mut releases,
        &mut artifacts,
    )?;
    enrolled.extend(releases.iter().map(|r| r.instance.registry.clone()));
    let selected = resolution::resolve(&roots, &releases, &enrolled)?;
    let (candidate, chosen) = render(before, rows, &selected, &artifacts)?;
    meta["installed"] = chosen;
    Ok((candidate, meta))
}

fn acquire_closure(
    context: &Context,
    registered: Option<&Effective>,
    rows: &[Value],
    mut acquisition: Acquisition,
    releases: &mut Vec<Release>,
    artifacts: &mut Artifacts,
) -> Result<BTreeSet<Instance>, Error> {
    let mut acquired = BTreeSet::new();
    let mut catalogs = acquisition.catalogs;
    while let Some(instance) = acquisition.pending.pop_first() {
        if !acquired.insert(instance.clone()) {
            continue;
        }
        if acquired.len() > 64 {
            return Err(policy::Error::Limit.into());
        }
        // Existing roots remain exact during add. Retained dependencies can satisfy
        // offline profile changes; acquisition is only needed for a missing closure.
        if (acquisition.offline || acquisition.locked.contains(&instance))
            && releases.iter().any(|r| r.instance == instance)
        {
            for release in releases.iter().filter(|r| r.instance == instance) {
                acquisition.pending.extend(dependencies(release));
            }
            continue;
        }
        if acquisition.offline {
            return Err(Error::Invalid("retained dependency closure is incomplete"));
        }
        let current;
        let registered = if let Some(registered) = registered {
            registered
        } else {
            current = registries::effective(Some(context))?;
            &current
        };
        let (alias, source) = enrolled_source(registered, rows, &instance)?;
        if !catalogs.contains_key(&instance.registry) {
            catalogs.insert(
                instance.registry.clone(),
                registries::checked_catalog(context, &source)?,
            );
        }
        let catalog = &catalogs[&instance.registry];
        let mut published = false;
        for entry in catalog.entries().iter().filter(|entry| {
            entry.id == instance.pack
                && acquisition
                    .minimum_versions
                    .get(&instance)
                    .is_none_or(|minimum| entry.version >= *minimum)
        }) {
            published = true;
            let Some(bytes) = candidate_artifact(&context.fetcher, &source, entry)? else {
                continue;
            };
            let row = json!({"id":entry.id,"version":entry.version,"digest":entry.sha256,"source":source,"source_identity":source.identity(),"source_alias":alias,"direct":false,"pinned":false});
            let release = release(&row, &bytes)?;
            acquisition.pending.extend(dependencies(&release));
            let key = (release.instance.clone(), release.version.clone());
            if let Some((_, retained)) = artifacts.get(&key) {
                if retained != &bytes {
                    return Err(Error::Invalid(
                        "same dependency release has different bytes",
                    ));
                }
                continue;
            }
            artifacts.insert(key, (row, bytes));
            releases.push(release);
            if releases.len() > 4096
                || artifacts.values().map(|(_, b)| b.len()).sum::<usize>()
                    > crate::configuration::MAX_BUNDLE_BYTES
            {
                return Err(policy::Error::Limit.into());
            }
        }
        if published && !releases.iter().any(|r| r.instance == instance) {
            return Err(Error::UnsupportedReleases(instance.pack.to_string()));
        }
    }
    Ok(acquired)
}

fn enrolled_source(
    registered: &Effective,
    rows: &[Value],
    instance: &Instance,
) -> Result<(String, Source), Error> {
    let direct = rows.iter().any(|row| {
        row["direct"] != false && identity(row).is_ok_and(|retained| retained == *instance)
    });
    // Retained direct selections can update from the pinned built-in source.
    // Foreign dependencies still require an explicitly enrolled source.
    let Some((alias, source, _)) = registered
        .by_identity(&instance.registry)
        .filter(|(_, _, scope)| direct || *scope != Scope::Builtin)
    else {
        return Err(Error::Invalid(
            "dependency registry identity is not enrolled",
        ));
    };
    if rows
        .iter()
        .any(|row| row["source_identity"] == instance.registry && row["source"] != json!(source))
    {
        return Err(Error::Invalid("retained registry identity binding changed"));
    }
    Ok((alias.to_owned(), source.clone()))
}

fn render(
    before: &State,
    rows: &[Value],
    selected: &[Selection<'_>],
    artifacts: &Artifacts,
) -> Result<(Value, Value), Error> {
    let mut chosen_rows = Vec::new();
    let mut chosen_bytes = Vec::new();
    for selection in selected {
        let release = selection.release;
        let (mut row, bytes) =
            artifacts[&(release.instance.clone(), release.version.clone())].clone();
        if let Some(old) = rows
            .iter()
            .find(|r| identity(r).is_ok_and(|id| id == release.instance))
        {
            row["selector"] = old["selector"].clone();
            row["pinned"] = old["pinned"].clone();
        } else {
            row["selector"] =
                new_selector(before, &release.instance.registry, &release.instance.pack);
        }
        row["profile"] = json!(selection.level);
        row["direct"] = json!(selection.direct);
        row["dependencies"] = json!(Pack::from_yaml(&bytes)?.dependencies());
        chosen_rows.push(row);
        chosen_bytes.push(bytes);
    }
    for row in &mut chosen_rows {
        let id = identity(row)?;
        row["incoming"] = json!(selected.iter().flat_map(|s| s.release.dependencies.iter().filter(|d| d.pack == id.pack && d.registry.as_ref().unwrap_or(&s.release.instance.registry) == &id.registry).map(|d| json!({"parent":s.release.instance.pack,"registry_identity":s.release.instance.registry,"requirement":d.requirement.to_string()}))).collect::<Vec<_>>());
    }
    let candidate = rebuild(
        &before.candidates["project"],
        &chosen_rows,
        &chosen_bytes,
        None,
    )?;
    Ok((candidate, json!(chosen_rows)))
}
