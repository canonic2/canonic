//! Testrun's offline `docs_*` tools: MCP Kit's shared documentation
//! bundle and queries, run off the async runtime and mapped into Testrun errors.
use crate::error::{Error, Result};
use canonic_mcp_kit::{
    Control,
    docs::{Bundle, DocsError, Documentation},
};
use serde_json::Value;
use std::{
    sync::{Arc, LazyLock},
    time::Instant,
};

static GUIDES: Bundle = canonic_mcp_kit::include_docs!();
static DOCS: LazyLock<Documentation> =
    LazyLock::new(|| Documentation::new(&GUIDES, "Testrun", env!("CARGO_PKG_VERSION")));

pub(crate) fn handles(name: &str) -> bool {
    DOCS.handles(name)
}

/// The three tool definitions, in the catalog's JSON form.
pub(crate) fn tools() -> Vec<Value> {
    DOCS.tools()
        .iter()
        .map(|tool| {
            let mut tool = serde_json::to_value(tool).expect("Tool definitions serialize");
            camel_cursor(&mut tool);
            tool
        })
        .collect()
}

/// Runs a documentation query on a blocking thread until it finishes, reaches
/// `deadline` or this future is dropped. Dropping the future cancels the query
/// at its next check; aborting the future alone could not stop synchronous work.
pub(crate) async fn call(
    name: &str,
    arguments: &Value,
    deadline: Option<Instant>,
) -> Result<Value> {
    let control = deadline.map_or_else(Control::without_deadline, Control::until);
    run(name, arguments, Arc::new(control)).await
}

async fn run(name: &str, arguments: &Value, control: Arc<Control>) -> Result<Value> {
    let mut cancel = CancelOnDrop(Some(control.clone()));
    let (query, arguments) = (name.to_owned(), arguments.clone());
    let check = control.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        DOCS.call(&query, &arguments, &|| check.check())
            .unwrap_or_else(|| {
                Err(DocsError::InvalidArguments(
                    "Unknown documentation tool".into(),
                ))
            })
    })
    .await
    .map_err(|_| Error::new("INTERNAL", "Documentation query stopped"))?;
    cancel.0 = None;
    outcome
        .map(|mut value| {
            camel_cursor(&mut value);
            value
        })
        .map_err(|error| failure(name, &error, &control))
}

fn camel_cursor(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if let Some(cursor) = fields.remove("next_cursor") {
                fields.insert("nextCursor".into(), cursor);
            }
            for value in fields.values_mut() {
                camel_cursor(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                if *value == "next_cursor" {
                    *value = Value::String("nextCursor".into());
                } else {
                    camel_cursor(value);
                }
            }
        }
        _ => {}
    }
}

/// Testrun's error for each shared documentation failure; the unknown-page
/// message keeps its pointer to `docs_list`.
fn failure(name: &str, error: &DocsError, control: &Control) -> Error {
    match error {
        DocsError::Pagination(canonic_mcp_kit::page::Error::InvalidArguments(message)) => {
            Error::invalid(message.clone())
        }
        DocsError::Pagination(paging) => Error::new(paging.code(), paging.to_string()),
        DocsError::InvalidArguments(message) => Error::invalid(message.clone()),
        DocsError::NotFound { .. } => Error::new("NOT_FOUND", error.to_string()),
        DocsError::Cancelled if control.expired() => {
            Error::new("TIMEOUT", format!("{name} exceeded its deadline"))
        }
        DocsError::Cancelled => Error::new("CANCELLED", format!("{name} was cancelled")),
        DocsError::Unavailable => Error::new("INTERNAL", error.to_string()),
    }
}

/// Cancels the blocking query when the request's future is dropped early.
struct CancelOnDrop(Option<Arc<Control>>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(control) = &self.0 {
            control.cancel();
        }
    }
}

#[cfg(test)]
mod tests;
