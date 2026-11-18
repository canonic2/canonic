//! Prepare complete update/removal closures without publishing or confirming them.
use super::{
    Acquisition, Artifacts, Instance, Release, Root, Selection, acquire_closure, identity, release,
    render, resolution,
};
use crate::{
    policy::Profile,
    review::{Context, Error, model::State},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

struct Retained {
    rows: Vec<Value>,
    releases: Vec<Release>,
    artifacts: Artifacts,
    enrolled: BTreeSet<String>,
}
impl Retained {
    fn load(before: &State) -> Result<Self, Error> {
        let rows: Vec<Value> = serde_json::from_value(before.operator["installed"].clone())
            .map_err(|_| Error::Invalid("installed packs missing"))?;
        let bytes: Vec<String> =
            serde_json::from_value(before.candidates["project"]["packs"].clone())
                .map_err(|_| Error::Invalid("pack bytes missing"))?;
        if rows.len() != bytes.len() {
            return Err(Error::Invalid("installed artifact set is incomplete"));
        }
        let mut releases = Vec::new();
        let mut artifacts = BTreeMap::new();
        let mut enrolled = BTreeSet::new();
        for (row, bytes) in rows.iter().zip(bytes) {
            let release = release(row, &bytes)?;
            enrolled.insert(release.instance.registry.clone());
            artifacts.insert(
                (release.instance.clone(), release.version.clone()),
                (row.clone(), bytes),
            );
            releases.push(release);
        }
        Ok(Self {
            rows,
            releases,
            artifacts,
            enrolled,
        })
    }

    fn roots(
        &self,
        floating: &BTreeSet<Instance>,
        removed: &BTreeSet<usize>,
    ) -> Result<Vec<Root<Profile>>, Error> {
        self.rows
            .iter()
            .zip(&self.releases)
            .enumerate()
            .filter(|(i, (row, _))| !removed.contains(i) && row["direct"] != false)
            .map(|(_, (row, release))| {
                Ok(Root {
                    instance: release.instance.clone(),
                    requirement: "*"
                        .parse()
                        .map_err(|_| Error::Invalid("version requirement"))?,
                    pin: if floating.contains(&release.instance) {
                        None
                    } else {
                        Some(release.version.clone())
                    },
                    level: serde_json::from_value(row["profile"].clone())
                        .map_err(|_| Error::Invalid("profile invalid"))?,
                })
            })
            .collect()
    }

    /// Keep exact retained versions for instances this update did not float.
    fn retain_locked_versions(&mut self, floating: &BTreeSet<Instance>, roots: &[Root<Profile>]) {
        self.releases.retain(|release| {
            self.rows
                .iter()
                .find(|row| identity(row).is_ok_and(|i| i == release.instance))
                .is_none_or(|row| {
                    if !floating.contains(&release.instance)
                        || roots
                            .iter()
                            .any(|r| r.instance == release.instance && r.pin.is_some())
                    {
                        row["version"] == release.version.to_string()
                    } else {
                        true
                    }
                })
        });
    }
}

pub(in crate::review::packs) fn update(
    context: &Context,
    before: &State,
    reference: Option<&str>,
) -> Result<(Value, Value), Error> {
    let mut retained = Retained::load(before)?;
    let aliases = super::super::current_aliases(&before.settings);
    let requested = reference
        .and_then(|reference| reference.rsplit_once('@'))
        .map(|(_, version)| {
            version
                .parse::<semver::Version>()
                .map_err(|_| Error::Invalid("update version must be an exact semantic version"))
        })
        .transpose()?;
    let target = reference
        .map(|r| super::super::selection(&retained.rows, &aliases, r))
        .transpose()?;
    let seeds: BTreeSet<_> = retained
        .rows
        .iter()
        .zip(&retained.releases)
        .enumerate()
        .filter(|(i, (row, _))| target.map_or(row["direct"] != false, |t| t == *i))
        .map(|(_, (_, r))| r.instance.clone())
        .collect();
    if seeds.is_empty() {
        return Ok((
            before.candidates["project"].clone(),
            before.operator.clone(),
        ));
    }
    let mut roots = retained.roots(&seeds, &BTreeSet::new())?;
    if let (Some(index), Some(version)) = (target, requested.as_ref())
        && let Some(root) = roots
            .iter_mut()
            .find(|root| root.instance == retained.releases[index].instance)
    {
        // Direct exact requests can select prereleases; dependency requests
        // still need their parents to explicitly permit the prerelease.
        root.requirement = format!("={version}")
            .parse()
            .map_err(|_| Error::Invalid("invalid update version"))?;
    }
    let exact_versions = target
        .zip(requested)
        .map(|(index, version)| (retained.releases[index].instance.clone(), version))
        .into_iter()
        .collect();
    let locked = retained
        .rows
        .iter()
        .zip(&retained.releases)
        .filter(|(row, r)| row["direct"] != false && !seeds.contains(&r.instance))
        .map(|(_, r)| r.instance.clone())
        .collect();
    let floating = acquire_closure(
        context,
        None,
        &retained.rows,
        &roots,
        Acquisition {
            exact_versions,
            minimum_versions: retained
                .rows
                .iter()
                .zip(&retained.releases)
                .filter(|(row, release)| {
                    row["direct"] != false && seeds.contains(&release.instance)
                })
                .map(|(_, release)| (release.instance.clone(), release.version.clone()))
                .collect(),
            pending: seeds,
            locked,
            offline: false,
            catalogs: BTreeMap::new(),
        },
        &mut retained.releases,
        &mut retained.artifacts,
    )?;
    retained.enrolled.extend(
        retained
            .releases
            .iter()
            .map(|r| r.instance.registry.clone()),
    );
    // Only this explicit request floats the selected roots and dependencies.
    // Rendering preserves each selection's pin mode at the resulting version.
    retained.retain_locked_versions(&floating, &roots);
    let selected = resolution::resolve(&roots, &retained.releases, &retained.enrolled)?;
    if unchanged(&retained.rows, &selected) {
        return Ok((
            before.candidates["project"].clone(),
            before.operator.clone(),
        ));
    }
    let (candidate, rows) = render(before, &retained.rows, &selected, &retained.artifacts)?;
    let mut meta = before.operator.clone();
    meta["installed"] = rows;
    Ok((candidate, meta))
}

fn unchanged(rows: &[Value], selected: &[Selection<'_>]) -> bool {
    rows.len() == selected.len()
        && selected.iter().all(|s| {
            rows.iter().any(|r| {
                identity(r).is_ok_and(|i| i == s.release.instance)
                    && r["version"] == s.release.version.to_string()
                    && serde_json::from_value::<Profile>(r["profile"].clone())
                        .is_ok_and(|p| p == s.level)
                    && (r["direct"] != false) == s.direct
            })
        })
}

pub(in crate::review::packs) fn remove(
    before: &State,
    reference: &str,
) -> Result<(Value, Value), Error> {
    remove_many(before, &[reference.to_owned()])
}

pub(in crate::review::packs) fn remove_many(
    before: &State,
    references: &[String],
) -> Result<(Value, Value), Error> {
    let retained = Retained::load(before)?;
    let aliases = super::super::current_aliases(&before.settings);
    let removed = references
        .iter()
        .map(|reference| super::super::selection(&retained.rows, &aliases, reference))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let roots = retained.roots(&BTreeSet::new(), &removed)?;
    let selected = resolution::resolve(&roots, &retained.releases, &retained.enrolled)?;
    for index in removed {
        let target = &retained.releases[index].instance;
        if selected.iter().any(|s| &s.release.instance == target) {
            let dependents = selected
                .iter()
                .filter(|s| {
                    s.release.dependencies.iter().any(|d| {
                        d.pack == target.pack
                            && d.registry.as_ref().unwrap_or(&s.release.instance.registry)
                                == &target.registry
                    })
                })
                .map(|s| {
                    format!(
                        "{}/{}",
                        s.release.instance.registry, s.release.instance.pack
                    )
                })
                .collect();
            return Err(Error::RequiredPack {
                pack: format!("{}/{}", target.registry, target.pack),
                dependents,
            });
        }
    }
    let (candidate, rows) = render(before, &retained.rows, &selected, &retained.artifacts)?;
    let mut meta = before.operator.clone();
    meta["installed"] = rows;
    Ok((candidate, meta))
}
