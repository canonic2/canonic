//! Edit the current TOML tree so unrelated tables, comments and layout survive.
use super::{Conflict, Error, choose};
use std::collections::BTreeSet;
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

fn document(source: Option<&str>) -> Result<DocumentMut, Error> {
    source
        .unwrap_or("")
        .parse()
        .map_err(|_| Error::Invalid("invalid shared Codex TOML"))
}

fn same(left: Option<&Item>, right: Option<&Item>) -> bool {
    if let (Some(left), Some(right)) = (
        left.and_then(Item::as_table_like),
        right.and_then(Item::as_table_like),
    ) {
        return same_table(left, right);
    }
    match (left, right) {
        (None, None) | (Some(Item::None), Some(Item::None)) => true,
        (Some(Item::Value(left)), Some(Item::Value(right))) => same_value(left, right),
        (Some(Item::Table(left)), Some(Item::Table(right))) => same_table(left, right),
        (Some(Item::ArrayOfTables(left)), Some(Item::ArrayOfTables(right))) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| same_table(left, right))
        }
        _ => false,
    }
}

fn same_table(left: &dyn TableLike, right: &dyn TableLike) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|(key, value)| same(Some(value), right.get(key)))
}

fn same_value(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::String(left), Value::String(right)) => left.value() == right.value(),
        (Value::Integer(left), Value::Integer(right)) => left.value() == right.value(),
        (Value::Float(left), Value::Float(right)) => {
            left.value().to_bits() == right.value().to_bits()
        }
        (Value::Boolean(left), Value::Boolean(right)) => left.value() == right.value(),
        (Value::Datetime(left), Value::Datetime(right)) => left.value() == right.value(),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| same_value(left, right))
        }
        (Value::InlineTable(left), Value::InlineTable(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right.get(key).is_some_and(|right| same_value(value, right))
                })
        }
        _ => false,
    }
}

fn detach(item: &mut Item) {
    if let Some(table) = item.as_table_mut() {
        detach_table(table);
    }
}

fn detach_table(table: &mut Table) {
    table.set_position(None);
    for (_, child) in table.iter_mut() {
        detach(child);
    }
}

fn owned(path: &[&str], before: Option<&Item>, after: Option<&Item>) -> bool {
    path.len() == 2
        && path[0] == "mcp_servers"
        && (!same(before, after) || (path[1] == "shield" && (before.is_some() || after.is_some())))
}

fn merge(
    path: &[&str],
    before: Option<&Item>,
    after: Option<&Item>,
    current: Option<&Item>,
    conflict: Conflict,
    kept: &mut bool,
) -> Result<Option<Item>, Error> {
    if owned(path, before, after) {
        return atomic(before, after, current, conflict, kept);
    }
    if same(before, after) && !path.is_empty() && path != ["mcp_servers"] {
        return Ok(current.cloned());
    }
    if [before, after]
        .into_iter()
        .flatten()
        .any(|item| item.as_table_like().is_some())
    {
        if current.is_some_and(|value| value.as_table_like().is_none()) {
            return atomic(before, after, current, conflict, kept);
        }
        let empty = Table::new();
        let old = before.and_then(Item::as_table_like).unwrap_or(&empty);
        let new = after.and_then(Item::as_table_like).unwrap_or(&empty);
        let mut actual_item = current
            .cloned()
            .unwrap_or_else(|| Item::Table(Table::new()));
        let actual = actual_item
            .as_table_like_mut()
            .expect("validated table-like container");
        let keys: BTreeSet<_> = old
            .iter()
            .map(|(key, _)| key)
            .chain(new.iter().map(|(key, _)| key))
            .collect();
        for key in keys {
            let mut nested = path.to_vec();
            nested.push(key);
            let merged = merge(
                &nested,
                old.get(key),
                new.get(key),
                actual.get(key),
                conflict,
                kept,
            )?;
            if same(merged.as_ref(), actual.get(key)) {
                continue;
            }
            match merged {
                Some(item) => {
                    actual.insert(key, item);
                }
                None => {
                    actual.remove(key);
                }
            }
        }
        return Ok(
            (!actual.is_empty() || (after.is_some() && current.is_some())).then_some(actual_item),
        );
    }
    atomic(before, after, current, conflict, kept)
}

fn atomic(
    before: Option<&Item>,
    after: Option<&Item>,
    current: Option<&Item>,
    conflict: Conflict,
    kept: &mut bool,
) -> Result<Option<Item>, Error> {
    if same(current, after) {
        return Ok(current.cloned());
    }
    let mut result = if same(current, before) {
        after.cloned()
    } else {
        choose(after, current, conflict, kept).map(Option::<&Item>::cloned)?
    };
    if (conflict != Conflict::KeepCurrent || same(current, before))
        && let Some(item) = &mut result
    {
        detach(item);
    }
    Ok(result)
}

pub(super) fn resolve(
    before: Option<&str>,
    after: Option<&str>,
    current: Option<&str>,
    conflict: Conflict,
) -> Result<(Option<String>, bool), Error> {
    let old = Item::Table(document(before)?.as_table().clone());
    let new = Item::Table(document(after)?.as_table().clone());
    let mut actual = document(current)?;
    let current_item = Item::Table(actual.as_table().clone());
    let mut kept = false;
    let merged = merge(
        &[],
        Some(&old),
        Some(&new),
        Some(&current_item),
        conflict,
        &mut kept,
    )?;
    if same(merged.as_ref(), Some(&current_item)) {
        return Ok((current.map(str::to_owned), kept));
    }
    *actual.as_table_mut() = merged
        .and_then(|item| item.as_table().cloned())
        .unwrap_or_default();
    let text = actual.to_string();
    Ok((
        (!text.trim().is_empty() || after.is_some()).then_some(text),
        kept,
    ))
}

pub(super) fn validate_aliases(
    before: Option<&str>,
    after: Option<&str>,
    current: Option<&str>,
) -> Result<(), Error> {
    let old = document(before)?;
    let new = document(after)?;
    let actual = document(current)?;
    let empty = Table::new();
    let old = old
        .get("mcp_servers")
        .and_then(Item::as_table_like)
        .unwrap_or(&empty);
    let new = new
        .get("mcp_servers")
        .and_then(Item::as_table_like)
        .unwrap_or(&empty);
    let actual = actual
        .get("mcp_servers")
        .and_then(Item::as_table_like)
        .unwrap_or(&empty);
    for (alias, value) in new.iter() {
        if alias != "shield" && same(old.get(alias), Some(value)) {
            continue;
        }
        if actual.contains_key(alias) {
            crate::agents::unambiguous_mcp_alias(
                "codex",
                alias,
                actual.iter().map(|(key, _)| key),
            )?;
        }
    }
    Ok(())
}
