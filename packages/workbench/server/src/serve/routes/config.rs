//! `/_workbench/config`: the configuration as this machine resolves it,
//! every path absolute and every problem named, with implementations at
//! their proxies. The canvas shows nothing until it answers, so every answer
//! is timed in the log. `/_workbench/config-file` and `/_workbench/sizes`
//! edit the space's `workbench.yaml` from the page form and the sizes dialog.
use super::{MAX_HANDOFF, body, json_response, trouble, wrong_method};
use crate::config::edit::{self, Location};
use crate::space::Space;
use axum::{extract::Request, http::Method, response::Response};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

fn millis(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

async fn config(space: &Space) -> Response {
    let began = Instant::now();
    space.diagnostics.info("config.requested", json!({})).await;
    let view = match space.view().await {
        Ok(view) => view,
        Err(error) => {
            space
                .diagnostics
                .warn(
                    "config.failed",
                    json!({ "elapsedMs": millis(began), "message": format!("{error:#}") }),
                )
                .await;
            return trouble(&error, 500);
        }
    };
    let catalogs = millis(began);
    let view = match view {
        Some(view) => Some(space.proxied(view).await),
        None => None,
    };
    let elapsed = millis(began);
    let level = if elapsed >= 1000 { "warn" } else { "info" };
    space
        .diagnostics
        .log(level, "config.resolved", json!({ "catalogsMs": catalogs, "proxyMs": elapsed - catalogs, "elapsedMs": elapsed, "found": view.is_some() }))
        .await;
    match view {
        None => json_response(
            404,
            &json!({ "ok": false, "error": "There is no workbench.yaml at the project root." }),
        ),
        Some(mut view) => {
            if let Some(fields) = view.as_object_mut() {
                let mut out = serde_json::Map::new();
                out.insert("ok".into(), json!(true));
                out.extend(std::mem::take(fields));
                view = serde_json::Value::Object(out);
            }
            json_response(200, &view)
        }
    }
}

pub(super) async fn answer(
    space: &Arc<Space>,
    path: &str,
    method: &Method,
    request: Request,
) -> Result<Response, Request> {
    let place = space.catalog.place();
    let at = Location {
        dir: &place.dir,
        key: place.key.as_deref(),
    };
    match path {
        "/_workbench/config" => Ok(config(space).await),
        "/_workbench/config-file" => Ok(if method == Method::GET {
            match edit::form(at) {
                Ok(answer) => json_response(200, &answer),
                Err(error) => trouble(&error, 400),
            }
        } else if method == Method::POST {
            match body(request, MAX_HANDOFF)
                .await
                .and_then(|bytes| edit::save_form(at, &bytes))
            {
                Ok(answer) => json_response(200, &answer),
                Err(error) => trouble(&error, 400),
            }
        } else {
            wrong_method("GET the page form or POST its collections here.")
        }),
        "/_workbench/sizes" => Ok(if method == Method::POST {
            match body(request, MAX_HANDOFF)
                .await
                .and_then(|bytes| edit::save_sizes(at, &bytes))
            {
                Ok(answer) => json_response(200, &answer),
                Err(error) => trouble(&error, 400),
            }
        } else {
            wrong_method("POST a size change here.")
        }),
        _ => Err(request),
    }
}
