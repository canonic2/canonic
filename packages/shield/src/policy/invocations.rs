//! Finite shared argv routes, indexed command membership and reversible rule identities.
use super::{Error, Request, model::PackDocument, rules::Rule};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[cfg(test)]
mod tests;

pub(super) type Segments = Vec<Vec<Vec<String>>>;
const MAX_ROUTES: usize = 4096;
const MAX_WORK: usize = 1_000_000;
const MAX_MATERIALIZED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Budget {
    bytes: usize,
    bodies: BTreeSet<(usize, usize)>,
}

impl Budget {
    pub(super) fn consume(
        &mut self,
        identity: (usize, usize),
        body: usize,
        prefix: &[String],
    ) -> Result<(), Error> {
        // First copies are bounded by source/plan limits; count repeated bodies
        // and all inserted prefixes as additional materialization work.
        let body = if self.bodies.insert(identity) {
            0
        } else {
            body
        };
        let bytes = prefix.iter().try_fold(body, |bytes, token| {
            bytes
                .checked_add(token.len() + std::mem::size_of::<Value>())
                .ok_or(Error::Limit)
        })?;
        self.bytes = self.bytes.checked_add(bytes).ok_or(Error::Limit)?;
        if self.bytes > MAX_MATERIALIZED_BYTES {
            return Err(Error::Limit);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Invocation {
    pub(super) segments: Segments,
    pub(super) id_prefix: String,
    #[serde(skip)]
    extent: OnceLock<(u64, usize, bool)>,
}

#[derive(Clone, Copy)]
pub(super) struct Route {
    pub(super) offset: usize,
    pub(super) ordinal: u64,
}

fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

fn sensitive_option(token: &str) -> bool {
    token.starts_with("--") && crate::evidence::sensitive_key(token.trim_start_matches('-'))
}

impl Invocation {
    fn extent(&self) -> Result<(u64, usize, bool), Error> {
        if let Some(extent) = self.extent.get() {
            return Ok(*extent);
        }
        let extent = self.validate_extent()?;
        let _ = self.extent.set(extent);
        Ok(extent)
    }

    fn validate_extent(&self) -> Result<(u64, usize, bool), Error> {
        if self.segments.is_empty()
            || self.segments.len() > 16
            || !name(&self.id_prefix)
            || self.id_prefix.contains('-')
            || !self.id_prefix.as_bytes()[0].is_ascii_alphanumeric()
        {
            return Err(Error::Contract("invalid invocation group"));
        }
        let (mut count, mut width) = (1u64, 0usize);
        let mut secret = false;
        for segment in &self.segments {
            if segment.is_empty() || segment.len() > 256 {
                return Err(Error::Limit);
            }
            let mut seen = BTreeSet::new();
            let mut next_secret = false;
            for vector in segment {
                if vector.len() > 256 || vector.iter().any(|v| v.len() > 4096 || v.contains('\0')) {
                    return Err(Error::Limit);
                }
                if !seen.insert(vector) {
                    return Err(Error::Contract("duplicate invocation alternative"));
                }
                if (secret && !vector.is_empty()) || crate::evidence::redact(&mut json!(vector)) {
                    return Err(Error::Contract("credentials in invocation prefix"));
                }
                next_secret |= vector
                    .last()
                    .map_or(secret, |token| sensitive_option(token));
            }
            secret = next_secret;
            count = count
                .checked_mul(segment.len() as u64)
                .ok_or(Error::Limit)?;
            width += segment.iter().map(Vec::len).max().unwrap_or(0);
        }
        if width > 256 {
            return Err(Error::Limit);
        }
        Ok((count, width, secret))
    }

    fn prefix(&self, mut ordinal: u64) -> Result<Option<Vec<String>>, Error> {
        if ordinal >= self.extent()?.0 {
            return Ok(None);
        }
        let mut pieces = Vec::new();
        for segment in self.segments.iter().rev() {
            pieces.push(
                &segment
                    [usize::try_from(ordinal % segment.len() as u64).map_err(|_| Error::Limit)?],
            );
            ordinal /= segment.len() as u64;
        }
        Ok(Some(pieces.into_iter().rev().flatten().cloned().collect()))
    }

    pub(super) fn identity(&self, ordinal: u64, body: &str) -> String {
        format!("{}{ordinal}-{body}", self.id_prefix)
    }
}

pub(super) fn definitions(doc: &PackDocument) -> Result<(), Error> {
    if doc.invocations.len() > 64 || doc.command_sets.len() > 500_000 {
        return Err(Error::Limit);
    }
    for (key, group) in &doc.invocations {
        if !name(key) {
            return Err(Error::Contract("invalid invocation name"));
        }
        group.extent()?;
    }
    let mut count = 0usize;
    for (key, members) in &doc.command_sets {
        if !name(key) || members.is_empty() {
            return Err(Error::Contract("invalid command set"));
        }
        count = count.checked_add(members.len()).ok_or(Error::Limit)?;
        for command in members.keys() {
            if command.is_empty() || command.len() > 4096 || command.contains('\0') {
                return Err(Error::Contract("invalid command member"));
            }
            if crate::evidence::redact(&mut json!(command)) {
                return Err(Error::Contract("credentials in command member"));
            }
        }
    }
    if count > 500_000 {
        return Err(Error::Limit);
    }
    Ok(())
}

pub(super) fn binding<'a>(
    doc: &'a PackDocument,
    rule: &Rule,
) -> Result<Option<&'a Invocation>, Error> {
    let Some(name) = &rule.invocation else {
        if rule.command_set.is_some() {
            return Err(Error::Contract("command set requires invocation"));
        }
        return Ok(None);
    };
    let group = doc
        .invocations
        .get(name)
        .ok_or(Error::Contract("unknown invocation group"))?;
    if rule.conditions["domain"] != "argv"
        || rule.executable().is_none()
        || rule.conditions.get("arguments").is_none()
        || rule.conditions["arguments"].get("leading").is_some()
    {
        return Err(Error::Contract(
            "shared invocations require exact argv executable and no leading grammar",
        ));
    }
    let args = &rule.conditions["arguments"];
    let field = if matches!(args["kind"].as_str(), Some("exact" | "prefix")) {
        "values"
    } else {
        "prefix"
    };
    let width = args[field].as_array().map_or(0, Vec::len);
    if group.extent()?.2 && (width != 0 || rule.command_set.is_some()) {
        return Err(Error::Contract("credentials cross invocation boundary"));
    }
    if group.extent()?.1 + width + usize::from(rule.command_set.is_some()) > 256 {
        return Err(Error::Limit);
    }
    if rule
        .command_set
        .as_ref()
        .is_some_and(|set| !doc.command_sets.contains_key(set))
    {
        return Err(Error::Contract("unknown command set"));
    }
    if width != 0
        && rule.command_set.as_ref().is_some_and(|set| {
            doc.command_sets[set]
                .keys()
                .any(|token| sensitive_option(token))
        })
    {
        return Err(Error::Contract("credentials cross command member boundary"));
    }
    Ok(Some(group))
}

pub(super) fn validate(doc: &PackDocument) -> Result<(), Error> {
    definitions(doc)?;
    let mut used = BTreeSet::new();
    let mut sets = BTreeSet::new();
    // Rule and precondition IDs share one namespace with command members and
    // generated invocation identities.
    let mut ids: BTreeSet<&str> = doc
        .rules
        .iter()
        .map(|r| r.id.as_str())
        .chain(doc.preconditions.iter().map(|p| p.id.as_str()))
        .collect();
    let mut virtuals = BTreeMap::new();
    for rule in &doc.rules {
        if let Some(group) = binding(doc, rule)? {
            used.insert(rule.invocation.as_deref().ok_or(Error::Syntax)?);
            let check = |id: &'_ str| -> Result<(), Error> {
                if group.identity(group.extent()?.0 - 1, id).len() > 128 {
                    return Err(Error::Limit);
                }
                if !super::rules::valid_id(id) {
                    return Err(Error::Contract("invalid member identity"));
                }
                Ok(())
            };
            if let Some(set) = &rule.command_set {
                if !sets.insert(set.as_str()) {
                    return Err(Error::Contract("command set has multiple rule owners"));
                }
                for id in doc.command_sets[set].values() {
                    check(id)?;
                    if !ids.insert(id) {
                        return Err(Error::Contract("duplicate member identity"));
                    }
                    virtuals.insert(id.as_str(), group);
                }
            } else {
                check(&rule.id)?;
                virtuals.insert(rule.id.as_str(), group);
            }
        }
    }
    if used.len() != doc.invocations.len() || sets.len() != doc.command_sets.len() {
        return Err(Error::Contract("unused invocation group or command set"));
    }
    for id in ids {
        if let Some((head, body)) = id.split_once('-')
            && let Some(group) = virtuals.get(body)
            && let Some(number) = head.strip_prefix(&group.id_prefix)
            && let Ok(ordinal) = number.parse::<u64>()
            && number == ordinal.to_string()
            && ordinal < group.extent()?.0
        {
            return Err(Error::Contract(
                "rule identity collides with invocation identity",
            ));
        }
    }
    Ok(())
}

pub(super) fn routes(
    segments: &Segments,
    args: &[String],
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Vec<Route>, Error> {
    let mut states = vec![Route {
        offset: 0,
        ordinal: 0,
    }];
    let mut work = 0usize;
    for segment in segments {
        let mut next = Vec::new();
        for state in states {
            for (ordinal, vector) in segment.iter().enumerate() {
                checkpoint()?;
                work += vector.len().max(1);
                if work > MAX_WORK {
                    return Err(Error::Limit);
                }
                if args
                    .get(state.offset..)
                    .is_some_and(|tail| tail.starts_with(vector))
                {
                    if next.len() == MAX_ROUTES {
                        return Err(Error::Limit);
                    }
                    next.push(Route {
                        offset: state.offset + vector.len(),
                        ordinal: state
                            .ordinal
                            .checked_mul(segment.len() as u64)
                            .and_then(|n| n.checked_add(ordinal as u64))
                            .ok_or(Error::Limit)?,
                    });
                }
            }
        }
        states = next;
    }
    Ok(states)
}

pub(super) fn materialize(
    rule: &Rule,
    group: &Invocation,
    route: Route,
    prefix: &[String],
    member: Option<&str>,
) -> Rule {
    let mut result = rule.clone();
    result.id = group.identity(route.ordinal, member.unwrap_or(&rule.id));
    result.invocation = None;
    result.command_set = None;
    let arguments = &mut result.conditions["arguments"];
    let kind = arguments["kind"].as_str().unwrap_or("any");
    if kind == "any" {
        if !prefix.is_empty() {
            *arguments = json!({"kind":"prefix","values":prefix});
        }
    } else {
        let field = if matches!(kind, "exact" | "prefix") {
            "values"
        } else {
            "prefix"
        };
        let values: Vec<Value> = prefix
            .iter()
            .map(|s| json!(s))
            .chain(arguments[field].as_array().into_iter().flatten().cloned())
            .collect();
        arguments[field] = json!(values);
    }
    result.prepare();
    result
}

pub(super) fn inspect(doc: &PackDocument, identity: &str) -> Result<Option<Rule>, Error> {
    if let Some(rule) = doc.rules.iter().find(|r| r.id == identity) {
        return Ok(Some(rule.clone()));
    }
    let Some((head, body)) = identity.split_once('-') else {
        return Ok(None);
    };
    for rule in &doc.rules {
        let Some(group) = binding(doc, rule)? else {
            continue;
        };
        let Some(number) = head.strip_prefix(&group.id_prefix) else {
            continue;
        };
        let Ok(ordinal) = number.parse::<u64>() else {
            continue;
        };
        if number != ordinal.to_string() {
            continue;
        }
        let Some(mut prefix) = group.prefix(ordinal)? else {
            continue;
        };
        let member = if let Some(set) = &rule.command_set {
            let Some((command, _)) = doc.command_sets[set]
                .iter()
                .find(|(_, id)| id.as_str() == body)
            else {
                continue;
            };
            prefix.push(command.clone());
            Some(body)
        } else if rule.id == body {
            None
        } else {
            continue;
        };
        return Ok(Some(materialize(
            rule,
            group,
            Route {
                offset: prefix.len(),
                ordinal,
            },
            &prefix,
            member,
        )));
    }
    Ok(None)
}

pub(super) fn matching<'a>(
    doc: &'a PackDocument,
    rule: &'a Rule,
    request: &'a Request,
    cached: &'a BTreeMap<&str, Vec<Route>>,
    budget: &'a mut Budget,
    identity: (usize, usize),
) -> Result<impl Iterator<Item = Result<Rule, Error>> + 'a, Error> {
    let bytes = serde_json::to_vec(rule).map_err(|_| Error::Syntax)?.len();
    let name = rule
        .invocation
        .as_ref()
        .ok_or(Error::Contract("missing invocation"))?;
    let group = doc
        .invocations
        .get(name)
        .ok_or(Error::Contract("unknown invocation"))?;
    let routes = cached
        .get(name.as_str())
        .ok_or(Error::Contract("missing invocation routes"))?;
    Ok(routes.iter().filter_map(move |route| {
        let (offset, member) = if let Some(set) = &rule.command_set {
            let command = request.arguments.get(route.offset)?;
            let id = doc.command_sets[set].get(command)?;
            (route.offset + 1, Some(id.as_str()))
        } else {
            (route.offset, None)
        };
        let prefix = &request.arguments[..offset];
        Some(
            budget
                .consume(identity, bytes, prefix)
                .map(|()| materialize(rule, group, *route, prefix, member)),
        )
    }))
}
