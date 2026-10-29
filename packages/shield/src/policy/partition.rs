//! Partitions current literal argv and context predicates with bounded resources.

use super::{CustomRules, Error, Pack, Profile, Request, model::Matcher};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

/// Hard bounds shared by CLI and library comparisons.
pub(crate) const MAX_REGIONS: usize = 4096;
const MAX_NODES: usize = 8192;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_RULE_CHECKS: usize = 2_000_000;

#[derive(Debug, Serialize)]
pub(crate) struct Limits {
    regions: usize,
    nodes: usize,
    tracked_bytes: usize,
    sweep_rule_checks: usize,
}

pub(crate) fn limits() -> Limits {
    Limits {
        regions: MAX_REGIONS,
        nodes: MAX_NODES,
        tracked_bytes: MAX_BYTES,
        sweep_rule_checks: MAX_RULE_CHECKS,
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Incomplete {
    UnsupportedSemanticDomain,
    /// A declared directory option rewrites commands before matching, which
    /// the partitions of written commands do not model.
    DirectoryOptions,
    StrictWitnessNotFound,
    PartitionBudget,
    EvaluationBudget,
    RequestLimitBoundary,
}

pub(crate) struct Partition {
    pub(crate) requests: Vec<Request>,
    pub(crate) incomplete: Option<Incomplete>,
    pub(crate) semantic: bool,
}

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
}

struct Builder {
    roots: BTreeMap<String, Node>,
    nodes: usize,
    bytes: usize,
    max_regions: usize,
    fixed_context: bool,
    partition: Partition,
}

/// Every returned representative has one fixed matching-rule truth vector.
/// Intermediate trie nodes cover termination; a fresh child covers all branches
/// absent from the trie. Deeper suffixes in an absent branch cannot match another
/// exact/prefix predicate. Unmentioned executables form the final unknown class.
pub(crate) fn build(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    cwd: &str,
    project: &str,
) -> Result<Partition, Error> {
    if complex(packs, customs) {
        return Ok(unsupported());
    }
    let matchers = matchers(packs, customs);
    build_matchers(&matchers, cwd, project)
}

fn matchers<'a>(packs: &[(&'a Pack, Profile)], customs: &[&'a CustomRules]) -> Vec<&'a Matcher> {
    packs
        .iter()
        .flat_map(|(pack, _)| {
            pack.document
                .rules
                .iter()
                .filter_map(|rule| rule.matcher.as_ref())
        })
        .chain(customs.iter().flat_map(|custom| {
            custom
                .document
                .rules
                .iter()
                .filter_map(|rule| rule.matcher.as_ref())
        }))
        .collect()
}

fn complex(packs: &[(&Pack, Profile)], customs: &[&CustomRules]) -> bool {
    packs
        .iter()
        .any(|(p, _)| !p.document.preconditions.is_empty())
        || packs
            .iter()
            .flat_map(|(p, _)| &p.document.rules)
            .chain(customs.iter().flat_map(|c| &c.document.rules))
            .any(|r| r.matcher.is_none())
}
fn unsupported() -> Partition {
    Partition {
        requests: vec![],
        incomplete: Some(Incomplete::UnsupportedSemanticDomain),
        semantic: true,
    }
}

fn builder(matchers: &[&Matcher]) -> Builder {
    Builder {
        roots: BTreeMap::new(),
        nodes: 0,
        bytes: 0,
        max_regions: MAX_REGIONS.min(MAX_RULE_CHECKS / matchers.len().max(1)),
        fixed_context: true,
        partition: Partition {
            requests: Vec::new(),
            incomplete: None,
            semantic: matchers.iter().any(|matcher| matcher.semantic()),
        },
    }
}

fn build_matchers(matchers: &[&Matcher], cwd: &str, project: &str) -> Result<Partition, Error> {
    build_matcher_shapes(matchers, cwd, project, true)
}

fn build_matcher_shapes(
    matchers: &[&Matcher],
    cwd: &str,
    project: &str,
    strict: bool,
) -> Result<Partition, Error> {
    if strict && matchers.iter().any(|matcher| !matcher.partitionable()) {
        return Ok(Partition {
            requests: Vec::new(),
            incomplete: Some(Incomplete::UnsupportedSemanticDomain),
            semantic: true,
        });
    }
    let mut builder = builder(matchers);
    for matcher in matchers {
        if !matcher.context_matches(cwd, project) {
            continue;
        }
        if let Err(reason) = builder.insert(matcher) {
            builder.partition.incomplete = Some(reason);
            return Ok(builder.partition);
        }
    }
    builder.context(cwd, project)?;
    Ok(builder.partition)
}

/// Partition both exact context predicates and argv predicates. Budgets cover
/// the whole Cartesian sweep, not each context separately.
pub(crate) fn build_all(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
) -> Result<Partition, Error> {
    if complex(packs, customs) {
        return Ok(unsupported());
    }
    build_all_matchers(&matchers(packs, customs))
}

fn build_all_matchers(matchers: &[&Matcher]) -> Result<Partition, Error> {
    build_all_shapes(matchers, true)
}

fn build_all_shapes(matchers: &[&Matcher], strict: bool) -> Result<Partition, Error> {
    if strict && matchers.iter().any(|matcher| !matcher.partitionable()) {
        return Ok(Partition {
            requests: Vec::new(),
            incomplete: Some(Incomplete::UnsupportedSemanticDomain),
            semantic: true,
        });
    }
    let mut builder = builder(matchers);
    let mut contexts = [BTreeSet::new(), BTreeSet::new()];
    for matcher in matchers {
        for (values, value) in contexts.iter_mut().zip(matcher.contexts()) {
            if let Some(value) = value
                && values.insert(value)
            {
                builder.nodes += 1;
                builder.bytes += value.len();
                if builder.nodes > MAX_NODES || builder.bytes > MAX_BYTES {
                    builder.partition.incomplete = Some(Incomplete::PartitionBudget);
                    return Ok(builder.partition);
                }
            }
        }
        if let Err(reason) = builder.insert(matcher) {
            builder.partition.incomplete = Some(reason);
            return Ok(builder.partition);
        }
    }
    let other = contexts.each_ref().map(|values| {
        let root = absolute_root();
        if values.contains(root) {
            format!(
                "{root}{}",
                fresh(
                    |value| values.contains(format!("{root}{value}").as_str()),
                    false
                )
            )
        } else {
            root.into()
        }
    });
    for cwd in contexts[0]
        .iter()
        .copied()
        .chain(std::iter::once(other[0].as_str()))
    {
        for project in contexts[1]
            .iter()
            .copied()
            .chain(std::iter::once(other[1].as_str()))
        {
            builder.fixed_context = contexts[0].contains(cwd) && contexts[1].contains(project);
            builder.context(cwd, project)?;
            if builder.partition.incomplete.is_some() {
                return Ok(builder.partition);
            }
        }
    }
    Ok(builder.partition)
}

/// Candidates only: effect predicates vary within these argv shapes.
pub(crate) fn shapes(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    fixed: Option<(&str, &str)>,
) -> Result<Partition, Error> {
    if complex(packs, customs) {
        return Ok(unsupported());
    }
    let matchers = matchers(packs, customs);
    if let Some((cwd, project)) = fixed {
        build_matcher_shapes(&matchers, cwd, project, false)
    } else {
        build_all_shapes(&matchers, false)
    }
}

/// Representative requests from the command rules alone, for witness search
/// when preconditions or other rule domains make a complete partition
/// unsupported. Each witness is still decided by the full policies; the
/// partition stays incomplete, so it never establishes a relation.
pub(crate) fn examples(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    fixed: Option<(&str, &str)>,
) -> Vec<Request> {
    let matchers = matchers(packs, customs);
    let shapes = if let Some((cwd, project)) = fixed {
        build_matcher_shapes(&matchers, cwd, project, false)
    } else {
        build_all_shapes(&matchers, false)
    };
    shapes
        .map(|partition| partition.requests)
        .unwrap_or_default()
}

pub(crate) fn witness_budget(packs: &[(&Pack, Profile)], customs: &[&CustomRules]) -> usize {
    MAX_REGIONS.min(MAX_RULE_CHECKS / matchers(packs, customs).len().max(1))
}

pub(crate) fn absolute_root() -> &'static str {
    if cfg!(windows) { "C:\\" } else { "/" }
}

impl Builder {
    fn context(&mut self, cwd: &str, project: &str) -> Result<(), Error> {
        let roots = std::mem::take(&mut self.roots);
        for (executable, node) in &roots {
            let request = Request {
                executable: executable.clone(),
                arguments: Vec::new(),
                cwd: cwd.into(),
                project: project.into(),
            };
            self.visit(node, &request)?;
            if self.partition.incomplete.is_some() {
                break;
            }
        }
        if self.partition.incomplete.is_none() {
            self.push(Request {
                executable: fresh(|value| roots.contains_key(value), false),
                arguments: Vec::new(),
                cwd: cwd.into(),
                project: project.into(),
            })?;
        }
        self.roots = roots;
        Ok(())
    }

    fn insert(&mut self, matcher: &Matcher) -> Result<(), Incomplete> {
        let mut node = &mut self.roots;
        let parts = std::iter::once(matcher.executable()).chain(
            matcher
                .argument_values()
                .unwrap_or_default()
                .iter()
                .map(String::as_str),
        );
        for part in parts {
            if !node.contains_key(part) {
                if self.nodes == MAX_NODES || self.bytes + part.len() > MAX_BYTES {
                    return Err(Incomplete::PartitionBudget);
                }
                self.nodes += 1;
                self.bytes += part.len();
            }
            node = &mut node.entry(part.into()).or_default().children;
        }
        Ok(())
    }

    fn visit(&mut self, node: &Node, request: &Request) -> Result<(), Error> {
        match request.validate() {
            // Prune only fixed classes: a residual path may have a shorter member.
            Err(Error::Limit) => {
                if !self.fixed_context {
                    self.partition.incomplete = Some(Incomplete::RequestLimitBoundary);
                }
                return Ok(());
            }
            result => result?,
        }
        self.push(request.clone())?;
        if self.partition.incomplete.is_some() || request.arguments.len() == 256 {
            return Ok(());
        }
        let mut other = request.clone();
        other
            .arguments
            .push(fresh(|value| node.children.contains_key(value), true));
        self.push(other)?;
        if self.partition.incomplete.is_some() {
            return Ok(());
        }
        for (argument, child) in &node.children {
            let mut next = request.clone();
            next.arguments.push(argument.clone());
            self.visit(child, &next)?;
            if self.partition.incomplete.is_some() {
                break;
            }
        }
        Ok(())
    }

    fn push(&mut self, request: Request) -> Result<(), Error> {
        match request.validate() {
            Err(Error::Limit) => {
                // A shorter fresh token could make this class reachable; never
                // infer non-expansion from a representative that does not fit.
                self.partition.incomplete = Some(Incomplete::RequestLimitBoundary);
                return Ok(());
            }
            result => result?,
        }
        let bytes = request.executable.len()
            + request.cwd.len()
            + request.project.len()
            + request.arguments.iter().map(String::len).sum::<usize>();
        let reason = if self.partition.requests.len() == self.max_regions {
            Some(Incomplete::EvaluationBudget)
        } else if self.bytes + bytes > MAX_BYTES {
            Some(Incomplete::PartitionBudget)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.partition.incomplete = Some(reason);
        } else {
            self.bytes += bytes;
            self.partition.requests.push(request);
        }
        Ok(())
    }
}

fn fresh(mut taken: impl FnMut(&str) -> bool, empty_allowed: bool) -> String {
    if empty_allowed && !taken("") {
        return String::new();
    }
    let mut index = 0;
    loop {
        let value = format!("shield-unmatched-{index}");
        if !taken(&value) {
            return value;
        }
        index += 1;
    }
}
