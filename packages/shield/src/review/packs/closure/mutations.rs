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
        removed: Option<usize>,
    ) -> Result<Vec<Root<Profile>>, Error> {
        self.rows
            .iter()
            .zip(&self.releases)
            .enumerate()
            .filter(|(i, (row, _))| Some(*i) != removed && row["direct"] != false)
            .map(|(_, (row, release))| {
                Ok(Root {
                    instance: release.instance.clone(),
                    requirement: "*"
                        .parse()
                        .map_err(|_| Error::Invalid("version requirement"))?,
                    pin: if floating.contains(&release.instance) && row["pinned"] != true {
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
}

pub(in crate::review::packs) fn update(
    context: &Context,
    before: &State,
    reference: Option<&str>,
) -> Result<(Value, Value), Error> {
    let mut retained = Retained::load(before)?;
    let aliases = super::super::current_aliases(&before.operator);
    let target = reference
        .map(|r| super::super::selection(&retained.rows, &aliases, r))
        .transpose()?;
    let seeds: BTreeSet<_> = retained
        .rows
        .iter()
        .zip(&retained.releases)
        .enumerate()
        .filter(|(i, (row, _))| {
            row["pinned"] != true && target.map_or(row["direct"] != false, |t| t == *i)
        })
        .map(|(_, (_, r))| r.instance.clone())
        .collect();
    if seeds.is_empty() {
        return Ok((
            before.candidates["project"].clone(),
            before.operator.clone(),
        ));
    }
    let roots = retained.roots(&seeds, None)?;
    let locked = retained
        .rows
        .iter()
        .zip(&retained.releases)
        .filter(|(row, r)| {
            row["direct"] != false && (row["pinned"] == true || !seeds.contains(&r.instance))
        })
        .map(|(_, r)| r.instance.clone())
        .collect();
    let floating = acquire_closure(
        context,
        None,
        &retained.rows,
        Acquisition {
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
    // Unchanged roots and dependencies reachable only from skipped pinned roots
    // retain exact locks. Shared dependencies reached by tracking roots can move.
    retained.releases.retain(|release| {
        retained
            .rows
            .iter()
            .find(|row| identity(row).is_ok_and(|i| i == release.instance))
            .is_none_or(|row| {
                if row["pinned"] == true
                    || !floating.contains(&release.instance)
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

/// The pinned roots an update of `reference`, or of every root, leaves at
/// their exact versions.
pub(in crate::review::packs) fn skipped_pinned(
    rows: &[Value],
    aliases: &[(String, String)],
    reference: Option<&str>,
) -> Result<Vec<Value>, Error> {
    let target = reference
        .map(|r| super::super::selection(rows, aliases, r))
        .transpose()?;
    Ok(rows
        .iter()
        .enumerate()
        .filter(|(i, row)| {
            row["pinned"] == true && target.map_or(row["direct"] != false, |t| t == *i)
        })
        .map(|(_, row)| {
            serde_json::json!({"selector":row["selector"],"id":row["id"],"version":row["version"]})
        })
        .collect())
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
    let retained = Retained::load(before)?;
    let aliases = super::super::current_aliases(&before.operator);
    let index = super::super::selection(&retained.rows, &aliases, reference)?;
    let target = &retained.releases[index].instance;
    let roots = retained.roots(&BTreeSet::new(), Some(index))?;
    let selected = resolution::resolve(&roots, &retained.releases, &retained.enrolled)?;
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
    let (candidate, rows) = render(before, &retained.rows, &selected, &retained.artifacts)?;
    let mut meta = before.operator.clone();
    meta["installed"] = rows;
    Ok((candidate, meta))
}
