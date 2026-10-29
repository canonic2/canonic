//! Profile proofs and concrete counterexamples use the shared resolver/classifier.

use super::{Prepared, partition};
use crate::policy::{
    Error, Request,
    profile_comparison::{self, Bound},
};
use std::collections::BTreeSet;

pub(super) fn prepare(
    first: &Prepared<'_>,
    second: &Prepared<'_>,
    fixed: Option<(&str, &str)>,
    regions: &mut partition::Partition,
) -> Result<Option<Bound>, Error> {
    if regions.incomplete != Some(partition::Incomplete::UnsupportedSemanticDomain)
        || first.effective.access_paths() != second.effective.access_paths()
    {
        return Ok(None);
    }
    let Some(bound) =
        profile_comparison::profiles(&first.packs, &second.packs, &first.custom, &second.custom)
    else {
        return Ok(None);
    };
    if bound == Bound::Equivalent {
        regions.incomplete = None;
        return Ok(Some(bound));
    }
    let packs: Vec<_> = first.packs.iter().chain(&second.packs).copied().collect();
    let customs: Vec<_> = first.custom.iter().chain(&second.custom).copied().collect();
    *regions = partition::shapes(&packs, &customs, fixed)?;
    let limit = partition::witness_budget(&packs, &customs);
    augment(&mut regions.requests, limit);
    regions.incomplete = Some(partition::Incomplete::StrictWitnessNotFound);
    Ok(Some(bound))
}

pub(super) fn finish(
    bound: Option<Bound>,
    expansion: bool,
    restriction: bool,
) -> Result<bool, Error> {
    match bound {
        Some(Bound::NonExpansive) if expansion => {
            Err(Error::Contract("profile bound contradicted"))
        }
        Some(Bound::NonRestrictive) if restriction => {
            Err(Error::Contract("profile bound contradicted"))
        }
        Some(Bound::NonExpansive) => Ok(restriction),
        Some(Bound::NonRestrictive) => Ok(expansion),
        Some(Bound::Equivalent) => Ok(true),
        None => Ok(false),
    }
}

pub(super) fn label(bound: Bound) -> &'static str {
    match bound {
        Bound::Equivalent => "equivalent",
        Bound::NonExpansive => "non-expansive",
        Bound::NonRestrictive => "non-restrictive",
    }
}

pub(super) fn augment(requests: &mut Vec<Request>, limit: usize) {
    let initial = requests.clone();
    let mut seen: BTreeSet<_> = requests.iter().map(key).collect();
    let mut bytes = requests.iter().map(size).sum::<usize>();
    for request in initial {
        let probes: &[&[&str]] = match request.executable.rsplit('/').next().unwrap_or_default() {
            "git" => &[
                &["status"],
                &["push", "origin", "HEAD"],
                &["reset", "--hard"],
                &["clean", "-fdx"],
                &["config", "--local", "acme.name", "Acme"],
                &["credential", "fill"],
                &["submodule", "foreach", "printf acme"],
                &["bisect", "run", "printf", "acme"],
            ],
            "docker" => &[
                &["inspect", "acme"],
                &[
                    "run",
                    "--privileged",
                    "--volume",
                    "/acme:/acme",
                    "acme-image",
                ],
                &["exec", "acme-container", "printf", "acme"],
                &["build", "."],
                &["image", "rm", "acme"],
                &["system", "prune"],
                &["plugin", "install", "acme/plugin"],
                &["push", "acme"],
            ],
            _ => &[],
        };
        for probe in probes {
            let mut candidate = request.clone();
            candidate
                .arguments
                .extend(probe.iter().map(|value| (*value).into()));
            let length = size(&candidate);
            if requests.len() >= limit || bytes + length > 8 * 1024 * 1024 {
                return;
            }
            if candidate.validate().is_ok() && seen.insert(key(&candidate)) {
                bytes += length;
                requests.push(candidate);
            }
        }
    }
}

fn key(request: &Request) -> (String, Vec<String>, String, String) {
    (
        request.executable.clone(),
        request.arguments.clone(),
        request.cwd.clone(),
        request.project.clone(),
    )
}

fn size(request: &Request) -> usize {
    request.executable.len()
        + request.arguments.iter().map(String::len).sum::<usize>()
        + request.cwd.len()
        + request.project.len()
}
