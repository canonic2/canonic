//! Shield adapters for the shared pagination contract; no retained results.
use super::Error;
use canonic_mcp_kit::page::{self, Builder, Limits, Position, Request, Scope};
use serde_json::{Value, json};

pub(super) const SUGGESTIONS: Limits = Limits {
    default: 16,
    ..Limits::STANDARD
};

pub(super) fn limits(name: &str) -> Limits {
    if name == "suggestion_list" {
        SUGGESTIONS
    } else {
        Limits::STANDARD
    }
}

pub(super) fn error(error: page::Error) -> Error {
    match error {
        page::Error::InvalidCursor => Error::InvalidCursor,
        page::Error::InvalidArguments(_) => {
            Error::Invalid("limit must be an integer from 1 to 100")
        }
        page::Error::InvalidPosition(_) => Error::Invalid("invalid list position"),
        page::Error::ItemTooLarge { read_tool } => Error::PageItemTooLarge(read_tool),
    }
}

pub(super) fn scope(name: &str, args: &Value) -> Scope {
    let mut query = args.clone();
    if let Some(fields) = query.as_object_mut() {
        fields.remove("cursor");
        fields.remove("limit");
    }
    Scope::new(name, page::digest(canonical(&query).to_string().as_bytes()))
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        _ => value.clone(),
    }
}

pub(super) fn query(context: &super::Context, name: &str, args: &Value) -> Value {
    let mut query = args.clone();
    if !(name == "registry_list" && args["scope"] == "user") {
        query["project_id"] = json!(context.project_id);
    }
    if name == "suggestion_list" && query.get("session_id").is_none() {
        if let Some(session) = &context.session_id {
            query["session_id"] = json!(session);
        }
    }
    query
}

pub(super) fn validate(context: &super::Context, name: &str, args: &Value) -> Result<(), Error> {
    let request = Request::from_mcp(args, limits(name)).map_err(error)?;
    if ["status", "doctor"].contains(&name) {
        return Ok(());
    }
    let query = query(context, name, args);
    after(&scope(name, &query), &request)?;
    Ok(())
}

/// Canonical identity bytes order ordinary IDs lexically. Long identities use
/// a 77-byte prefix and a digest tie-breaker; this is the actual sort key.
pub(super) fn key(identity: &Value) -> Vec<u8> {
    let bytes = canonical(identity).to_string().into_bytes();
    if bytes.len() <= page::MAX_KEY_BYTES {
        return bytes;
    }
    let mut key = bytes[..77].to_vec();
    key.push(0);
    key.extend(page::digest(&bytes));
    key
}

pub(super) fn after(scope: &Scope, request: &Request) -> Result<Option<Vec<u8>>, Error> {
    request
        .cursor
        .as_deref()
        .map(|cursor| match scope.decode(cursor).map_err(error)? {
            Position::Keyset(key) => Ok(key),
            Position::Offset(_) => Err(Error::InvalidCursor),
        })
        .transpose()
}

pub(super) fn collect(
    name: &str,
    args: &Value,
    rows: impl IntoIterator<Item = (Vec<u8>, Value)>,
    read_tool: &str,
) -> Result<page::Page, Error> {
    collect_results(name, args, rows.into_iter().map(Ok), read_tool)
}

pub(super) fn collect_results(
    name: &str,
    args: &Value,
    rows: impl IntoIterator<Item = Result<(Vec<u8>, Value), Error>>,
    read_tool: &str,
) -> Result<page::Page, Error> {
    let request = Request::from_mcp(args, limits(name)).map_err(error)?;
    let scope = scope(name, args);
    let after = after(&scope, &request)?;
    let mut rows = rows.into_iter();
    let mut builder = Builder::new(
        &scope,
        request.limit,
        limits(name),
        4 * 1024 * 1024 - 16 * 1024,
        read_tool,
    )
    .map_err(error)?;
    let mut exhausted = false;
    let mut pending = false;
    let mut admitted = false;
    while !builder.is_full() {
        let Some(row) = rows.next() else {
            exhausted = true;
            break;
        };
        let (key, row) = row?;
        if after.as_ref().is_some_and(|last| key <= *last) {
            continue;
        }
        if !admitted {
            // MCP exposes the item in structured data and in JSON compatibility
            // text. Account for escaping the latter before admitting a lone
            // oversized row; ordinary pages remain far below the frame limit.
            let encoded = row.to_string();
            if encoded.len() > Limits::STANDARD.max_bytes
                && encoded.len() + json!(encoded).to_string().len() > 4 * 1024 * 1024 - 16 * 1024
            {
                return Err(Error::PageItemTooLarge(read_tool.to_owned()));
            }
        }
        if !builder.push(row, &Position::Keyset(key)).map_err(error)? {
            pending = true;
            break;
        }
        admitted = true;
    }
    // Do not build an unreturned view just to inspect the iterator's next item.
    // Unknown exhaustion uses a conservative continuation, as the contract allows.
    Ok(builder.finish(pending || (!exhausted && rows.size_hint().1 != Some(0))))
}

pub(super) fn identity(name: &str, row: &Value) -> Value {
    if name == "pack_list" {
        return json!([row["id"], row["selector"]]);
    }
    if name == "rule_list" && row.get("rule_ref").is_some() {
        return json!([row["id"], row["source"]["id"], row["rule_ref"]]);
    }
    let fields: &[&str] = match name {
        "rule_list" => &["rule_ref", "id"],
        "pack_list" => &["selector"],
        "history_list" => &["entry_id"],
        "request_list" => &["request_id", "id"],
        "suggestion_list" => &["review_id"],
        "policy_show" => &["event_id", "proposal_id", "id"],
        "policy_draft_list" => &["draft_id", "id"],
        _ => &[
            "selector",
            "id",
            "proposal_id",
            "request_id",
            "event_id",
            "review_id",
            "grant_id",
            "session_ref",
            "reference",
        ],
    };
    if name == "pack_search" {
        return json!([row["id"], row["version"]]);
    }
    if let Some(id) = fields.iter().find_map(|field| row.get(field)) {
        return id.clone();
    }
    let qualifiers: serde_json::Map<_, _> = ["agent", "name", "path", "code", "server", "tool"]
        .into_iter()
        .filter_map(|field| {
            row.get(field)
                .map(|value| (field.to_owned(), value.clone()))
        })
        .collect();
    if qualifiers.is_empty() {
        row.clone()
    } else {
        Value::Object(qualifiers)
    }
}

/// Select bounded checkpoint metadata before constructing public row views.
pub(super) fn borrowed<'a, T: 'a>(
    context: &super::Context,
    name: &str,
    args: &Value,
    rows: impl IntoIterator<Item = &'a T>,
    identity: impl Fn(&T) -> Value,
    view: impl Fn(&T) -> Value,
    read_tool: &str,
) -> Result<page::Page, Error> {
    let request = Request::from_mcp(args, limits(name)).map_err(error)?;
    let query = query(context, name, args);
    let position = after(&scope(name, &query), &request)?;
    let mut rows: Vec<_> = rows
        .into_iter()
        .map(|row| (key(&identity(row)), row))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    collect(
        name,
        &query,
        rows.into_iter()
            .filter(|(key, _)| position.as_ref().is_none_or(|last| key > last))
            .map(|(key, row)| (key, view(row))),
        read_tool,
    )
}

fn row_key(name: &str, row: &Value) -> Vec<u8> {
    if name == "history_list" {
        let mut bytes: Vec<_> = row["requested_at"]
            .as_str()
            .unwrap_or_default()
            .bytes()
            .map(|byte| 255 - byte)
            .collect();
        bytes.push(0);
        bytes.extend(page::digest(row["entry_id"].to_string().as_bytes()));
        return bytes;
    }
    key(&identity(name, row))
}

pub(super) fn bounded(name: &str, args: &Value, data: Value, field: &str) -> Result<Value, Error> {
    bounded_with(name, args, data, field, |row| row)
}

pub(super) fn bounded_with(
    name: &str,
    args: &Value,
    mut data: Value,
    field: &str,
    view: impl Fn(Value) -> Value,
) -> Result<Value, Error> {
    let Some(collection) = data.get_mut(field) else {
        return Ok(data);
    };
    let object = collection.is_object();
    let rows = match collection.take() {
        Value::Array(rows) => rows,
        Value::Object(rows) => rows
            .into_iter()
            .map(|(name, value)| json!({"id":name,"value":value}))
            .collect(),
        other => {
            data[field] = other;
            return Ok(data);
        }
    };
    let mut rows: Vec<_> = rows
        .into_iter()
        .map(|row| (row_key(name, &row), row))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let read_tool = match name {
        "pack_list" | "pack_search" => "pack_show or pack_info with rule_id",
        "rule_list" => "rule_list with rule_ref",
        "request_list" | "suggestion_list" => "request_show",
        "history_list" => "history_show",
        "policy_show" => "policy_show with proposal_id",
        "policy_draft_list" => "policy_draft_list with draft_id",
        _ => name,
    };
    let request = Request::from_mcp(args, limits(name)).map_err(error)?;
    let position = after(&scope(name, args), &request)?;
    let selected = rows
        .into_iter()
        .filter(|(key, _)| position.as_ref().is_none_or(|last| key > last))
        .map(|(key, row)| (key, view(row)));
    let result = collect(name, args, selected, read_tool)?;
    data[field] = if object {
        Value::Object(
            result
                .items
                .into_iter()
                .map(|row| {
                    (
                        row["id"].as_str().expect("map key").to_owned(),
                        row["value"].clone(),
                    )
                })
                .collect(),
        )
    } else {
        json!(result.items)
    };
    data["next_cursor"] = json!(result.next_cursor);
    Ok(data)
}

pub(super) fn preview(value: &mut Value, field: &str, route: Value) {
    let previous = value.get(format!("{field}_total")).and_then(Value::as_u64);
    let Some(collection) = value.get_mut(field) else {
        return;
    };
    let total = match collection {
        Value::Array(rows) => {
            let total = rows.len();
            rows.truncate(16);
            total
        }
        Value::Object(rows) => {
            let total = rows.len();
            let keys: Vec<_> = rows.keys().skip(16).cloned().collect();
            for key in keys {
                rows.remove(&key);
            }
            total
        }
        _ => return,
    };
    value[format!("{field}_total")] = json!(previous.unwrap_or(total as u64).max(total as u64));
    value[format!("{field}_list")] = route;
}

/// Status and doctor expose collection sections without turning summaries into
/// retained snapshots. Domain lists named by an existing preview remain primary.
pub(super) fn sections(
    context: &super::Context,
    name: &str,
    args: &Value,
    mut data: Value,
) -> Result<Value, Error> {
    Request::from_mcp(args, limits(name)).map_err(error)?;
    if let Some(section) = args.get("section") {
        let pointer = section
            .as_str()
            .filter(|value| value.starts_with('/') && value.len() <= 1024)
            .ok_or(Error::Invalid("section must be a returned JSON Pointer"))?;
        let mut current = &data;
        let mut owners = Vec::new();
        let mut total = None;
        for part in pointer.split('/').skip(1) {
            let part = part.replace("~1", "/").replace("~0", "~");
            if let Some(rows) = current.as_array() {
                let index = part
                    .parse::<usize>()
                    .map_err(|_| Error::Invalid("invalid collection section"))?;
                current = rows
                    .get(index)
                    .ok_or(Error::Invalid("unknown collection section"))?;
                owners.push(identity(name, current));
                total = None;
            } else {
                total = current.get(format!("{part}_total")).and_then(Value::as_u64);
                current = current
                    .get(&part)
                    .ok_or(Error::Invalid("unknown collection section"))?;
            }
        }
        let rows = current
            .as_array()
            .ok_or(Error::Invalid("section must name a collection"))?;
        if total.is_some_and(|total| total > rows.len() as u64) {
            return Err(Error::Invalid(
                "this section is a preview; use its named list tool",
            ));
        }
        let mut query = query(context, name, args);
        query["section_owner"] = json!(owners);
        let indices: std::collections::BTreeMap<_, _> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row_key(name, row), index))
            .collect();
        let mut result = bounded_with(name, &query, json!({"items":rows}), "items", |mut row| {
            let index = indices[&row_key(name, &row)];
            preview_arrays(&mut row, name, &format!("{pointer}/{index}"));
            row
        })?;
        result["section"] = json!(pointer);
        result["total"] = json!(rows.len());
        return Ok(result);
    }
    if args.get("cursor").is_some() {
        return Err(Error::InvalidCursor);
    }
    preview_arrays(&mut data, name, "");
    Ok(data)
}

fn preview_arrays(value: &mut Value, name: &str, pointer: &str) {
    let Some(fields) = value.as_object_mut() else {
        return;
    };
    let keys: Vec<_> = fields.keys().cloned().collect();
    for key in keys {
        // Returned calls and atomic requests must remain executable unchanged.
        if key == "arguments"
            || key == "request"
            || key == "next_action"
            || key.ends_with("_arguments")
            || key.ends_with("_list")
            || key.starts_with("inspect_")
        {
            continue;
        }
        let path = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
        let previous = fields.get(&format!("{key}_total")).and_then(Value::as_u64);
        let Some(field) = fields.get_mut(&key) else {
            continue;
        };
        if let Some(rows) = field.as_array_mut() {
            let total = previous.unwrap_or(rows.len() as u64);
            rows.truncate(16);
            for (index, row) in rows.iter_mut().enumerate() {
                preview_arrays(row, name, &format!("{path}/{index}"));
            }
            fields.insert(format!("{key}_total"), json!(total));
            fields
                .entry(format!("{key}_list"))
                .or_insert_with(|| json!({"tool":name,"section":path}));
        } else {
            preview_arrays(field, name, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(values: &[u64], bytes: usize) -> Vec<(Vec<u8>, Value)> {
        values
            .iter()
            .map(|id| {
                (
                    id.to_be_bytes().to_vec(),
                    json!({"id":id,"text":"x".repeat(bytes)}),
                )
            })
            .collect()
    }

    #[test]
    fn traverses_statelessly_and_allows_limit_changes_and_insertions() {
        let first = collect(
            "records",
            &json!({"filter":"open","limit":2}),
            rows(&[1, 3, 5], 0),
            "record_show",
        )
        .unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|v| v["id"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [1, 3]
        );
        let args = json!({"filter":"open","limit":1,"cursor":first.next_cursor});
        let next = collect(
            "records",
            &args,
            rows(&[0, 1, 2, 3, 4, 5], 0),
            "record_show",
        )
        .unwrap();
        assert_eq!(next.items[0]["id"], 4);
        let last = collect(
            "records",
            &json!({"filter":"open","cursor":next.next_cursor}),
            rows(&[0, 1, 2, 3, 4, 5], 0),
            "record_show",
        )
        .unwrap();
        assert_eq!(last.items[0]["id"], 5);
        assert_eq!(last.next_cursor, None);
        for args in [
            json!({"filter":"closed","cursor":first.next_cursor}),
            json!({"cursor":"broken"}),
            json!({"limit":0}),
            json!({"limit":101}),
        ] {
            assert!(collect("records", &args, rows(&[1, 3, 5], 0), "record_show").is_err());
        }
        assert!(matches!(
            collect(
                "foreign",
                &json!({"filter":"open","cursor":first.next_cursor}),
                rows(&[1, 3, 5], 0),
                "record_show"
            ),
            Err(Error::InvalidCursor)
        ));
    }

    #[test]
    fn byte_budget_stops_early_and_an_oversized_item_is_alone() {
        let page = collect("records", &json!({}), rows(&[1, 2], 150_000), "record_show").unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.next_cursor.is_some());
        let last = collect(
            "records",
            &json!({"cursor":page.next_cursor}),
            rows(&[1, 2], 150_000),
            "record_show",
        )
        .unwrap();
        assert_eq!(last.items[0]["id"], 2);
        assert!(last.next_cursor.is_none());
        let page = collect("records", &json!({}), rows(&[1, 2], 300_000), "record_show").unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.next_cursor.is_some());
        let page = collect(
            "records",
            &json!({}),
            rows(&[1, 2], 1_200_000),
            "record_show",
        )
        .unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.next_cursor.is_some());
        assert!(
            matches!(collect("records",&json!({}),[(vec![1],json!({"text":"\"".repeat(1_200_000)}))],"record_show"),Err(Error::PageItemTooLarge(tool)) if tool=="record_show")
        );
    }

    #[test]
    fn constructs_views_only_for_the_selected_page() {
        let calls = std::cell::Cell::new(0);
        let data = json!({"rows":(0..64).map(|id|json!({"id":id})).collect::<Vec<_>>()});
        let first = bounded_with(
            "records",
            &json!({"limit":1}),
            data.clone(),
            "rows",
            |row| {
                calls.set(calls.get() + 1);
                row
            },
        )
        .unwrap();
        assert_eq!(calls.get(), 1);
        calls.set(0);
        let second = bounded_with(
            "records",
            &json!({"limit":1,"cursor":first["next_cursor"]}),
            data,
            "rows",
            |row| {
                calls.set(calls.get() + 1);
                row
            },
        )
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_ne!(first["rows"][0]["id"], second["rows"][0]["id"]);
    }

    #[test]
    fn summary_sections_preview_then_page_and_bind_their_owner() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let data = json!({"diagnostics":(0..30).map(|index|json!({"id":format!("issue-{index:03}")})).collect::<Vec<_>>(),"other":[]});
        let summary = sections(&context, "doctor", &json!({}), data.clone()).unwrap();
        assert_eq!(summary["diagnostics"].as_array().unwrap().len(), 16);
        assert_eq!(summary["diagnostics_total"], 30);
        assert_eq!(summary["diagnostics_list"]["section"], "/diagnostics");
        let mut cursor = None;
        let mut ids = std::collections::BTreeSet::new();
        loop {
            let mut args = json!({"section":"/diagnostics","limit":7});
            if let Some(value) = cursor {
                args["cursor"] = value;
            }
            let page = sections(&context, "doctor", &args, data.clone()).unwrap();
            for item in page["items"].as_array().unwrap() {
                assert!(ids.insert(item["id"].as_str().unwrap().to_owned()));
            }
            if page["next_cursor"].is_null() {
                break;
            }
            cursor = Some(page["next_cursor"].clone());
        }
        assert_eq!(ids.len(), 30);
        let mut nested = json!({"agents":[{"agent":"acme","diagnostics":data["diagnostics"]}]});
        let page = sections(
            &context,
            "doctor",
            &json!({"section":"/agents/0/diagnostics","limit":1}),
            nested.clone(),
        )
        .unwrap();
        nested["agents"][0]["agent"] = json!("foreign");
        assert!(matches!(
            sections(
                &context,
                "doctor",
                &json!({"section":"/agents/0/diagnostics","cursor":page["next_cursor"]}),
                nested
            ),
            Err(Error::InvalidCursor)
        ));
    }

    #[test]
    fn collection_catalog_and_cli_declare_shared_page_inputs() {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        for tool in crate::review::mcp::tools()["tools"].as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            if !(name.ends_with("_list")
                || [
                    "status",
                    "doctor",
                    "pack_search",
                    "pack_info",
                    "policy_show",
                    "history_show",
                    "session_grant_show",
                    "docs_search",
                    "docs_read",
                ]
                .contains(&name))
            {
                continue;
            }
            let properties = &tool["inputSchema"]["properties"];
            assert_eq!(properties["cursor"]["type"], "string", "{name}");
            assert_eq!(properties["limit"]["type"], "integer", "{name}");
            assert_eq!(properties["limit"]["minimum"], 1, "{name}");
            assert!(properties["limit"]["default"].as_u64().is_some(), "{name}");
            let path: &[&str] = match name {
                "status" => &["status"],
                "doctor" => &["doctor"],
                "pack_list" => &["pack", "list"],
                "pack_search" => &["pack", "search"],
                "pack_info" => &["pack", "info"],
                "registry_list" => &["registry", "list"],
                "rule_list" => &["rule", "list"],
                "policy_show" => &["policy", "show"],
                "request_list" => &["request", "list"],
                "history_list" => &["history", "list"],
                "history_show" => &["history", "show"],
                "suggestion_list" => &["suggestion", "list"],
                "policy_draft_list" => &["policy-draft", "list"],
                "session_grant_list" | "session_grant_show" => &["session-grant", "list"],
                "docs_list" => &["docs", "list"],
                "docs_search" => &["docs", "search"],
                "docs_read" => &["docs", "read"],
                _ => panic!("collection tool needs CLI mapping: {name}"),
            };
            let mut command = &cli;
            for part in path {
                command = command
                    .find_subcommand(part)
                    .unwrap_or_else(|| panic!("missing CLI for {name}"));
            }
            for flag in ["cursor", "limit"] {
                assert!(
                    command
                        .get_arguments()
                        .any(|arg| arg.get_long() == Some(flag)),
                    "{name}: missing --{flag}"
                );
            }
        }
    }
}
