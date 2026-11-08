//! Apply a confirmed file delta without claiming unrelated host configuration.
use super::{Error, MAX_CONFIG};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

mod callbacks;
mod ordered;
#[cfg(test)]
mod tests;
mod toml;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Conflict {
    Refuse,
    ReplaceOwned,
    KeepCurrent,
}

pub(super) fn reconcile(
    path: &str,
    before: Option<&str>,
    after: Option<&str>,
    current: Option<&str>,
) -> Result<Option<String>, Error> {
    resolve(path, before, after, current, Conflict::Refuse).map(|(source, _)| source)
}

/// The boolean records a manual conflict kept during cancellation. Such a
/// fragment must no longer be claimed by the cancelled registration.
pub(super) fn resolve(
    path: &str,
    before: Option<&str>,
    after: Option<&str>,
    current: Option<&str>,
    conflict: Conflict,
) -> Result<(Option<String>, bool), Error> {
    if [before, after, current]
        .into_iter()
        .flatten()
        .any(|source| source.len() as u64 > MAX_CONFIG)
    {
        return Err(Error::Invalid("integration configuration exceeds limit"));
    }
    let mut output = if current == after {
        (current.map(str::to_owned), false)
    } else if current == before {
        (after.map(str::to_owned), false)
    } else if shared_json(path) || path == ".codex/config.toml" {
        let merged = if shared_json(path) {
            json(before, after, current, conflict)
        } else {
            toml::resolve(before, after, current, conflict)
        };
        match merged {
            Ok(output) => output,
            Err(_) if conflict == Conflict::KeepCurrent => (current.map(str::to_owned), true),
            Err(error) => return Err(error),
        }
    } else {
        let mut kept = false;
        let source = choose(after, current, conflict, &mut kept)?.map(str::to_owned);
        (source, kept)
    };
    let aliases = if shared_json(path) {
        json_aliases(before, after, output.0.as_deref())
    } else if path == ".codex/config.toml" {
        toml::validate_aliases(before, after, output.0.as_deref())
    } else {
        Ok(())
    };
    if let Err(error) = aliases {
        if conflict == Conflict::KeepCurrent {
            output = (current.map(str::to_owned), true);
        } else {
            return Err(error);
        }
    }
    if shared_json(path)
        && output.0.as_deref() != current
        && let Some(source) = output.0.as_deref()
    {
        let value = document(Some(source))?.expect("present validated JSON");
        output.0 = Some(
            ordered::format(current, &value)
                .map_err(|_| Error::Invalid("merged integration JSON encoding"))?,
        );
    }
    if output
        .0
        .as_ref()
        .is_some_and(|source| source.len() as u64 > MAX_CONFIG)
    {
        return Err(Error::Invalid(
            "merged integration configuration exceeds limit",
        ));
    }
    Ok(output)
}

fn shared_json(path: &str) -> bool {
    [
        ".mcp.json",
        ".claude/settings.local.json",
        ".codex/hooks.json",
        ".gemini/settings.json",
        ".agents/hooks.json",
        ".agents/mcp_config.json",
    ]
    .contains(&path)
}

fn choose<'a, T: ?Sized>(
    after: Option<&'a T>,
    current: Option<&'a T>,
    conflict: Conflict,
    kept: &mut bool,
) -> Result<Option<&'a T>, Error> {
    match conflict {
        Conflict::Refuse => Err(Error::Invalid(
            "confirmed Shield fragments changed; review the conflict in this session",
        )),
        Conflict::ReplaceOwned => Ok(after),
        Conflict::KeepCurrent => {
            *kept = true;
            Ok(current)
        }
    }
}

fn document(source: Option<&str>) -> Result<Option<Value>, Error> {
    source
        .map(|source| {
            crate::json::parse(source.as_bytes())
                .map_err(|_| Error::Invalid("invalid shared integration JSON"))
                .and_then(|value| {
                    if value.is_object() {
                        Ok(value)
                    } else {
                        Err(Error::Invalid("integration JSON must be an object"))
                    }
                })
        })
        .transpose()
}

fn json_aliases(
    before: Option<&str>,
    after: Option<&str>,
    current: Option<&str>,
) -> Result<(), Error> {
    let (old, new, actual) = (document(before)?, document(after)?, document(current)?);
    let empty = Map::new();
    let old = old
        .as_ref()
        .and_then(|value| value.get("mcpServers"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let new = new
        .as_ref()
        .and_then(|value| value.get("mcpServers"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let actual = actual
        .as_ref()
        .and_then(|value| value.get("mcpServers"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    for (alias, value) in new {
        if alias != "shield" && old.get(alias) == Some(value) {
            continue;
        }
        if actual.contains_key(alias) {
            crate::agents::unambiguous_mcp_alias(alias, actual.keys().map(String::as_str))?;
        }
    }
    Ok(())
}

fn json(
    before: Option<&str>,
    after: Option<&str>,
    current: Option<&str>,
    conflict: Conflict,
) -> Result<(Option<String>, bool), Error> {
    let (old, new, actual) = (document(before)?, document(after)?, document(current)?);
    let mut kept = false;
    let merged = value(
        &[],
        old.as_ref(),
        new.as_ref(),
        actual.as_ref(),
        conflict,
        &mut kept,
    )?;
    if merged.as_ref() == actual.as_ref() {
        return Ok((current.map(str::to_owned), kept));
    }
    let text = merged
        .map(|value| {
            serde_json::to_string_pretty(&value)
                .map(|text| format!("{text}\n"))
                .map_err(|_| Error::Invalid("merged integration JSON encoding"))
        })
        .transpose()?;
    Ok((text, kept))
}

fn value(
    path: &[&str],
    before: Option<&Value>,
    after: Option<&Value>,
    current: Option<&Value>,
    conflict: Conflict,
    kept: &mut bool,
) -> Result<Option<Value>, Error> {
    if atomic(path, before, after) {
        return atom(before, after, current, conflict, kept);
    }
    if path.len() == 2
        && path[0] == "hooks"
        && [before, after]
            .into_iter()
            .flatten()
            .any(callbacks::has_owned)
    {
        return callbacks::merge(before, after, current, conflict, kept);
    }
    let reserved = path.is_empty()
        || (path == ["hooks"]
            && [before, after].into_iter().flatten().any(|value| {
                value
                    .as_object()
                    .into_iter()
                    .flatten()
                    .any(|(_, event)| callbacks::has_owned(event))
            }))
        || (path == ["mcpServers"]
            && [before, after]
                .into_iter()
                .flatten()
                .any(|value| value.get("shield").is_some()));
    if before == after && !reserved {
        return Ok(current.cloned());
    }
    if [before, after].into_iter().flatten().any(Value::is_object) {
        return object(path, before, after, current, conflict, kept);
    }
    if before == after {
        return Ok(current.cloned());
    }
    if string_list(path)
        && [before, after, current].into_iter().flatten().all(|value| {
            value
                .as_array()
                .is_some_and(|rows| rows.iter().all(Value::is_string))
        })
    {
        let old: BTreeSet<_> = rows(before).iter().filter_map(Value::as_str).collect();
        let new: BTreeSet<_> = rows(after).iter().filter_map(Value::as_str).collect();
        let mut actual = rows(current).to_vec();
        actual.retain(|entry| {
            entry
                .as_str()
                .is_none_or(|value| !old.contains(value) || new.contains(value))
        });
        let mut present: BTreeSet<_> = actual
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        for entry in rows(after) {
            let text = entry.as_str().expect("validated string collection");
            if !old.contains(text) && present.insert(text.to_owned()) {
                actual.push(entry.clone());
            }
        }
        return Ok((!actual.is_empty() || after.is_some()).then_some(Value::Array(actual)));
    }
    atom(before, after, current, conflict, kept)
}

fn atomic(path: &[&str], before: Option<&Value>, after: Option<&Value>) -> bool {
    (path == ["shield-project-enforcement"] && (before.is_some() || after.is_some()))
        || (path.len() == 2
            && path[0] == "mcpServers"
            && (before != after || (path[1] == "shield" && (before.is_some() || after.is_some()))))
        || (path == ["enabledPlugins", "shield@skills-dir"]
            && (before.is_some() || after.is_some()))
}

fn atom(
    before: Option<&Value>,
    after: Option<&Value>,
    current: Option<&Value>,
    conflict: Conflict,
    kept: &mut bool,
) -> Result<Option<Value>, Error> {
    if current == before || current == after {
        Ok(after.cloned())
    } else {
        choose(after, current, conflict, kept).map(Option::<&Value>::cloned)
    }
}

fn object(
    path: &[&str],
    before: Option<&Value>,
    after: Option<&Value>,
    current: Option<&Value>,
    conflict: Conflict,
    kept: &mut bool,
) -> Result<Option<Value>, Error> {
    if current.is_some_and(|value| !value.is_object()) && conflict != Conflict::ReplaceOwned {
        return choose(after, current, conflict, kept).map(Option::<&Value>::cloned);
    }
    let empty = Map::new();
    let old = before.and_then(Value::as_object).unwrap_or(&empty);
    let new = after.and_then(Value::as_object).unwrap_or(&empty);
    let mut actual = current
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let keys: BTreeSet<_> = old.keys().chain(new.keys()).collect();
    for key in keys {
        let mut nested = path.to_vec();
        nested.push(key);
        match value(
            &nested,
            old.get(key),
            new.get(key),
            actual.get(key),
            conflict,
            kept,
        )? {
            Some(value) => {
                actual.insert(key.clone(), value);
            }
            None => {
                actual.remove(key);
            }
        }
    }
    Ok(
        (!actual.is_empty() || (after.is_some() && current.is_some()))
            .then_some(Value::Object(actual)),
    )
}

fn rows(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn string_list(path: &[&str]) -> bool {
    path == ["enabledMcpjsonServers"]
        || (path.len() == 2
            && path[0] == "permissions"
            && ["allow", "ask", "deny"].contains(&path[1]))
}
