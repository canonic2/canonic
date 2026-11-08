//! Callback ownership does not include other integrations in the same group.
use super::{Conflict, Error, atom, rows};
use crate::agents::setup::{same_hook_header, shield_callback};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

struct Part {
    group: usize,
    callback: usize,
    header: String,
    value: String,
}

fn header(group: &Value) -> String {
    let fields: BTreeMap<_, _> = group
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.as_str() != "hooks")
        .collect();
    crate::json::fingerprint(&fields).expect("hook header encodes")
}

fn parts(groups: &[Value]) -> Vec<Part> {
    groups
        .iter()
        .enumerate()
        .flat_map(|(group_index, group)| {
            let callbacks: Vec<_> = rows(group.get("hooks"))
                .iter()
                .enumerate()
                .filter(|(_, callback)| shield_callback(callback))
                .collect();
            let key = if callbacks.is_empty() {
                String::new()
            } else {
                header(group)
            };
            callbacks.into_iter().map(move |(index, value)| Part {
                group: group_index,
                callback: index,
                header: key.clone(),
                value: crate::json::fingerprint(value).expect("hook callback encodes"),
            })
        })
        .collect()
}

pub(super) fn has_owned(value: &Value) -> bool {
    rows(Some(value))
        .iter()
        .any(|group| rows(group.get("hooks")).iter().any(shield_callback))
}

fn same_parts(left: &[Part], right: &[Part]) -> bool {
    let counts = |parts: &[Part]| {
        let mut counts = BTreeMap::new();
        for part in parts {
            *counts
                .entry((part.header.clone(), part.value.clone()))
                .or_insert(0_usize) += 1;
        }
        counts
    };
    counts(left) == counts(right)
}

fn foreign(groups: &[Value]) -> Vec<(&Value, Vec<&Value>)> {
    groups
        .iter()
        .filter_map(|group| {
            let callbacks: Vec<_> = rows(group.get("hooks"))
                .iter()
                .filter(|value| !shield_callback(value))
                .collect();
            (!callbacks.is_empty() || group.get("hooks").is_none()).then_some((group, callbacks))
        })
        .collect()
}

fn foreign_unchanged(before: &[Value], after: &[Value]) -> bool {
    let old = foreign(before);
    let new = foreign(after);
    old.len() == new.len()
        && old
            .iter()
            .zip(new)
            .all(|((left, callbacks), (right, other))| {
                if left.is_object() && right.is_object() {
                    same_hook_header(left, right) && *callbacks == other
                } else {
                    *left == right
                }
            })
}

pub(super) fn merge(
    before: Option<&Value>,
    after: Option<&Value>,
    current: Option<&Value>,
    conflict: Conflict,
    kept: &mut bool,
) -> Result<Option<Value>, Error> {
    if [before, after, current]
        .into_iter()
        .flatten()
        .any(|value| !value.is_array())
    {
        return atom(before, after, current, conflict, kept);
    }
    let old = rows(before);
    let new = rows(after);
    let actual = rows(current);
    if !foreign_unchanged(old, new) {
        return Err(Error::Invalid(
            "confirmed hook delta changes unrelated callbacks",
        ));
    }
    let owned_old = parts(old);
    let owned_new = parts(new);
    let selected = parts(actual);
    if !same_parts(&selected, &owned_old) && !same_parts(&selected, &owned_new) {
        match conflict {
            Conflict::Refuse => {
                return Err(Error::Invalid(
                    "confirmed Shield callback changed; review the conflict in this session",
                ));
            }
            Conflict::KeepCurrent => {
                *kept = true;
                return Ok(current.cloned());
            }
            Conflict::ReplaceOwned => {}
        }
    }
    let selected: BTreeSet<_> = selected
        .iter()
        .map(|part| (part.group, part.callback))
        .collect();
    let known_headers: BTreeSet<_> = owned_old
        .iter()
        .chain(&owned_new)
        .map(|part| part.header.as_str())
        .collect();
    let mut pending: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut by_header: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for part in &owned_new {
        pending.entry(part.group).or_default().push(part.callback);
        by_header
            .entry(part.header.clone())
            .or_default()
            .insert(part.group);
    }
    let mut output = Vec::with_capacity(actual.len() + pending.len());
    for (index, group) in actual.iter().enumerate() {
        if selected
            .range((index, 0)..=(index, usize::MAX))
            .next()
            .is_none()
        {
            output.push(group.clone());
            continue;
        }
        let key = header(group);
        let mut hooks = Vec::new();
        for (callback, value) in rows(group.get("hooks")).iter().enumerate() {
            if selected.contains(&(index, callback)) {
                let matching = by_header.remove(&key).unwrap_or_default();
                for group in matching {
                    for callback in pending.remove(&group).expect("pending group") {
                        hooks.push(rows(new[group].get("hooks"))[callback].clone());
                    }
                }
            } else {
                hooks.push(value.clone());
            }
        }
        if !hooks.is_empty() || !known_headers.contains(key.as_str()) {
            let mut value = group.clone();
            value["hooks"] = Value::Array(hooks);
            output.push(value);
        }
    }
    for (group, callbacks) in pending {
        let mut fields: Map<_, _> = new[group]
            .as_object()
            .expect("owned group object")
            .iter()
            .filter(|(key, _)| key.as_str() != "hooks")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        fields.insert(
            "hooks".into(),
            Value::Array(
                callbacks
                    .into_iter()
                    .map(|index| rows(new[group].get("hooks"))[index].clone())
                    .collect(),
            ),
        );
        output.push(Value::Object(fields));
    }
    Ok((!output.is_empty() || after.is_some()).then_some(Value::Array(output)))
}
