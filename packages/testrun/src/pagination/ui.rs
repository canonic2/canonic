//! Observation-bound document-order UI pages. JSON pointers retain ancestry.
use super::*;

fn nodes(root: &Value) -> Vec<(String, &Value)> {
    let mut stack = vec![(String::new(), root)];
    let mut rows = Vec::new();
    while let Some((path, node)) = stack.pop() {
        if let Some(children) = node["children"].as_array() {
            stack.extend(
                children
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(index, child)| (format!("{path}/children/{index}"), child)),
            );
        }
        rows.push((path, node));
    }
    rows
}
fn shallow(node: &Value) -> Value {
    json!(
        node.as_object()
            .into_iter()
            .flatten()
            .filter(|(field, _)| field.as_str() != "children"
                && !(node["secure"] == true && field.as_str() == "value"))
            .map(|(field, value)| (field.clone(), value.clone()))
            .collect::<serde_json::Map<_, _>>()
    )
}
fn binding(query: &impl Serialize, root: &Value) -> Result<Value> {
    Ok(json!([
        query,
        canonic_mcp_kit::page::digest(root.to_string().as_bytes())
    ]))
}
pub(crate) fn find(
    root: &Value,
    query: &crate::ui::Query,
    args: &Value,
    identity: &Value,
) -> Result<Value> {
    query.validate()?;
    let mut error = None;
    let rows = nodes(root)
        .into_iter()
        .enumerate()
        .filter(|(_, (_, node))| match query.matches_node(node) {
            Ok(matched) => matched,
            Err(cause) => {
                error = Some(cause);
                false
            }
        })
        .map(|(ordinal, (_, node))| ((ordinal as u64).to_be_bytes().to_vec(), node));
    let page = collect(
        "ui_find",
        &binding(&(identity, &args["query"]), root)?,
        args,
        rows,
        |node| Ok(shallow(node)),
        "ui_read",
    )?;
    if let Some(error) = error {
        return Err(error);
    }
    Ok(json!({"matches":page.items,"nextCursor":page.next_cursor}))
}
pub(crate) fn snapshot(root: &Value, args: &Value, identity: &Value) -> Result<Value> {
    let request = Request::from_mcp_units(args, BYTES)?;
    let scope = scope("ui_snapshot", &binding(identity, root)?)?;
    let after = request
        .cursor
        .as_deref()
        .map(|cursor| scope.decode(cursor))
        .transpose()?;
    let after = match after {
        Some(Position::Keyset(key)) if key.len() == 8 => Some(u64::from_be_bytes(
            key.try_into()
                .map_err(|_| canonic_mcp_kit::page::Error::InvalidCursor)?,
        )),
        None => None,
        _ => return Err(canonic_mcp_kit::page::Error::InvalidCursor.into()),
    };
    let limits = Limits {
        max_bytes: usize::try_from(request.limit.get())
            .expect("byte limit")
            .max(2),
        ..BYTES
    };
    let mut builder = Builder::new(
        &scope,
        request.limit,
        limits,
        crate::protocol::MAX_REPLY,
        "ui_read",
    )?;
    let mut rows = nodes(root)
        .into_iter()
        .enumerate()
        .filter(|(index, _)| after.is_none_or(|after| *index as u64 > after));
    let mut ended = false;
    while !builder.is_full() {
        let Some((ordinal, (path, node))) = rows.next() else {
            ended = true;
            break;
        };
        if !builder.push(
            json!({"path":path,"node":shallow(node)}),
            &Position::Keyset((ordinal as u64).to_be_bytes().to_vec()),
        )? {
            break;
        }
    }
    let page = builder.finish(!ended && rows.size_hint().1 != Some(0));
    Ok(
        json!({"nodes":page.items,"nextCursor":page.next_cursor,"unit":"bytes","placement":"path is a JSON pointer into the observation's original root"}),
    )
}
