//! Pinned `Page.localStorage`/`Page.sessionStorage` (`WebStorage`) on a page's
//! main frame, with keyset pages over the item names.
use super::super::{contract::Reference, session::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Item names are bounded like other Testrun keys; values follow the 512 KiB
/// inline-content bound so a set fits the 1 MiB frame and driver limits.
pub(crate) const MAX_NAME_BYTES: usize = 4096;
pub(crate) const MAX_VALUE_BYTES: usize = 512 * 1024;
/// One read value must fit an ordinary reply.
const MAX_READ_VALUE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Local,
    Session,
}
impl Kind {
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Session => "session",
        }
    }
    fn global(self) -> &'static str {
        match self {
            Self::Local => "localStorage",
            Self::Session => "sessionStorage",
        }
    }
}

/// One `WebStorage` mutation: `setItem`, `removeItem` or `clear`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Update {
    Set { name: String, value: String },
    Remove { name: String },
    Clear {},
}
impl Update {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Set { name, value } => {
                validate_name(name)?;
                if value.len() > MAX_VALUE_BYTES {
                    return Err("WEB_STORAGE_VALUE_BOUNDS".into());
                }
                Ok(())
            }
            Self::Remove { name } => validate_name(name),
            Self::Clear {} => Ok(()),
        }
    }
    /// The driver method and parameters the pinned client sends.
    pub(crate) fn driver(&self, kind: Kind) -> (&'static str, Value) {
        match self {
            Self::Set { name, value } => (
                "webStorageSetItem",
                json!({"kind":kind.wire(),"name":name,"value":value}),
            ),
            Self::Remove { name } => (
                "webStorageRemoveItem",
                json!({"kind":kind.wire(),"name":name}),
            ),
            Self::Clear {} => ("webStorageClear", json!({"kind":kind.wire()})),
        }
    }
    /// The pinned server's utility-world expression, for native Chrome.
    pub(crate) fn expression(&self, kind: Kind) -> String {
        let storage = kind.global();
        match self {
            Self::Set { name, value } => {
                format!("{storage}.setItem({}, {})", json!(name), json!(value))
            }
            Self::Remove { name } => format!("{storage}.removeItem({})", json!(name)),
            Self::Clear {} => format!("{storage}.clear()"),
        }
    }
    pub(crate) fn result(&self, page: Reference, kind: Kind) -> Value {
        let action = match self {
            Self::Set { .. } => "set",
            Self::Remove { .. } => "remove",
            Self::Clear {} => "clear",
        };
        json!({"page":page,"kind":kind,"action":action,"updated":true})
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), Error> {
    if name.len() > MAX_NAME_BYTES {
        return Err("WEB_STORAGE_NAME_BOUNDS".into());
    }
    Ok(())
}

/// A read selects one item by `name`, or pages over every item.
pub(crate) fn validate_read(
    name: Option<&str>,
    cursor: Option<&str>,
    limit: Option<u64>,
) -> Result<(), Error> {
    match name {
        Some(_) if cursor.is_some() || limit.is_some() => Err(Error::from("INVALID_ARGUMENT")),
        Some(name) => validate_name(name),
        None => Ok(()),
    }
}

/// The pinned server's `webStorageItems` expression.
pub(crate) fn items_expression(kind: Kind) -> String {
    let storage = kind.global();
    format!(
        "(() => {{ const result = []; for (let i = 0; i < {storage}.length; i++) {{ const name = {storage}.key(i); if (name !== null) result.push({{ name, value: {storage}.getItem(name) ?? '' }}); }} return result; }})()"
    )
}
/// The pinned server's `webStorageGetItem` expression; `null` means absent.
pub(crate) fn get_expression(kind: Kind, name: &str) -> String {
    format!("{}.getItem({})", kind.global(), json!(name))
}

/// One named item, as the pinned `getItem` returns `undefined` when absent.
pub(crate) fn item(page: Reference, kind: Kind, name: &str, value: &Value) -> Result<Value, Error> {
    let value = match value {
        Value::Null => None,
        Value::String(value) => Some(value),
        _ => return Err("BACKEND_PROTOCOL".into()),
    };
    if value.is_some_and(|value| value.len() > MAX_READ_VALUE_BYTES) {
        return Err("WEB_STORAGE_READ_BOUNDS".into());
    }
    Ok(
        json!({"page":page,"kind":kind,"name":name,"found":value.is_some(),"value":value,"secretBearing":true}),
    )
}

/// One page of items in name order. Storage is bounded per origin by the
/// browser's quota, so the complete item list is sliced in memory.
pub(crate) fn page(
    page: Reference,
    kind: Kind,
    items: &Value,
    cursor: Option<String>,
    limit: Option<u64>,
) -> Result<Value, Error> {
    let items = items.as_array().ok_or("BACKEND_PROTOCOL")?;
    let rows = items
        .iter()
        .map(|item| {
            let name = item["name"].as_str().ok_or("BACKEND_PROTOCOL")?;
            item["value"].as_str().ok_or("BACKEND_PROTOCOL")?;
            Ok((crate::pagination::identity_key(name), item))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let (items, cursor) = super::keyset(
        "browser_send.page_web_storage_read",
        &json!({"page":page,"kind":kind}),
        rows,
        |item| json!({"name":item["name"],"value":item["value"]}),
        cursor.as_deref(),
        limit,
        "browser_send page_web_storage_read with name",
    )?;
    let next = cursor.as_ref().map(|cursor| {
        let mut next =
            json!({"operation":"page_web_storage_read","page":page,"kind":kind,"cursor":cursor});
        if let Some(limit) = limit {
            next["limit"] = json!(limit);
        }
        next
    });
    Ok(
        json!({"page":page,"kind":kind,"items":items,"nextCursor":cursor,"nextRequest":next,"secretBearing":true}),
    )
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let kind = json!({"enum":["local","session"]});
    let name = json!({"type":"string","maxLength":MAX_NAME_BYTES});
    let mut read = json!({"operation":{"const":"page_web_storage_read"},"page":reference,"kind":kind,"name":name});
    read.as_object_mut().expect("read properties").extend(
        crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD)
            .as_object()
            .expect("page properties")
            .clone(),
    );
    // set takes name and value, remove takes name, clear takes neither;
    // `Update` refuses other combinations.
    let update = json!({"type":"object","properties":{"action":{"enum":["set","remove","clear"]},"name":name,"value":{"type":"string","maxLength":MAX_VALUE_BYTES}},"required":["action"],"additionalProperties":false});
    vec![
        json!({"type":"object","properties":read,"required":["operation","page","kind"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"page_web_storage_update"},"page":reference,"kind":kind,"update":update},"required":["operation","page","kind","update"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn reference() -> Reference {
        Reference {
            connection_id: Uuid::nil(),
            id: Uuid::nil(),
            generation: Uuid::nil(),
        }
    }

    #[test]
    fn web_storage_updates_match_the_pinned_driver_and_server_expressions() {
        let set: Update =
            serde_json::from_value(json!({"action":"set","name":"Acme \"key\"","value":"v"}))
                .unwrap();
        assert_eq!(
            set.driver(Kind::Session),
            (
                "webStorageSetItem",
                json!({"kind":"session","name":"Acme \"key\"","value":"v"})
            )
        );
        assert_eq!(
            set.expression(Kind::Local),
            r#"localStorage.setItem("Acme \"key\"", "v")"#
        );
        let clear: Update = serde_json::from_value(json!({"action":"clear"})).unwrap();
        assert_eq!(clear.expression(Kind::Session), "sessionStorage.clear()");
        assert!(serde_json::from_value::<Update>(json!({"action":"clear","name":"x"})).is_err());
        let large = Update::Set {
            name: "Acme".into(),
            value: "a".repeat(MAX_VALUE_BYTES + 1),
        };
        assert_eq!(
            large.validate().unwrap_err().code,
            "WEB_STORAGE_VALUE_BOUNDS"
        );
        assert_eq!(
            validate_read(Some("Acme"), None, Some(10))
                .unwrap_err()
                .code,
            "INVALID_ARGUMENT"
        );
    }

    #[test]
    fn web_storage_items_page_in_name_order_and_resume_after_the_cursor() {
        let items = json!([
            {"name":"c","value":"3"},{"name":"a","value":"1"},{"name":"b","value":"2"}
        ]);
        let first = page(reference(), Kind::Local, &items, None, Some(2)).unwrap();
        assert_eq!(
            first["items"],
            json!([{"name":"a","value":"1"},{"name":"b","value":"2"}])
        );
        let cursor = first["nextCursor"].as_str().unwrap().to_owned();
        assert_eq!(first["nextRequest"]["cursor"], cursor);
        // An item inserted before the cursor is not repeated or skipped after.
        let grown = json!([
            {"name":"c","value":"3"},{"name":"a","value":"1"},{"name":"b","value":"2"},{"name":"aa","value":"x"}
        ]);
        let second = page(
            reference(),
            Kind::Local,
            &grown,
            Some(cursor.clone()),
            Some(2),
        )
        .unwrap();
        assert_eq!(second["items"], json!([{"name":"c","value":"3"}]));
        assert!(second["nextCursor"].is_null());
        // A cursor belongs to one page and storage kind.
        assert_eq!(
            page(reference(), Kind::Session, &items, Some(cursor), Some(2))
                .unwrap_err()
                .code,
            "invalid_cursor"
        );
        assert_eq!(
            page(reference(), Kind::Local, &items, None, Some(0))
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
        let absent = item(reference(), Kind::Local, "Acme", &Value::Null).unwrap();
        assert_eq!(absent["found"], false);
        assert!(absent["value"].is_null());
    }
}
