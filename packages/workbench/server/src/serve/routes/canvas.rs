//! What the canvas asks of the server besides pages: diagnostics, the view
//! agents read, the spaces it switches between, opening a page's source,
//! and handoffs. Only a page this server served may switch or open spaces,
//! since opening one can run its start commands.
use super::{
    MAX_HANDOFF, MAX_LOG, MAX_VIEW, base, body, is_get, json_response, ok, parse, same_origin,
    trouble, wrong_method,
};
use crate::refused::refused;
use crate::space::Space;
use anyhow::{Result, anyhow};
use axum::{extract::Request, http::Method, response::Response};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

async fn log(space: &Space, request: Request) -> Response {
    let entry = match body(request, MAX_LOG).await.and_then(|bytes| parse(&bytes)) {
        Ok(entry) => entry,
        Err(error) => return trouble(&error, 400),
    };
    let level = entry["level"].as_str().unwrap_or("info");
    let event = match &entry["event"] {
        Value::String(event) => event.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    space
        .diagnostics
        .log(level, &event, entry["details"].clone())
        .await;
    ok(json!({}))
}

async fn view(space: &Space, method: &Method, request: Request) -> Response {
    if is_get(method) {
        return ok(space.agent.current());
    }
    if method != Method::POST {
        return wrong_method("POST the canvas view here, or GET the current one.");
    }
    match body(request, MAX_VIEW)
        .await
        .and_then(|bytes| parse(&bytes))
        .and_then(|report| space.agent.report(&report))
    {
        Ok(()) => ok(json!({})),
        Err(error) => trouble(&error, 400),
    }
}

/// The spaces the canvas can switch between, this one marked current: the
/// host's list, or this space alone when the host shows no other.
async fn spaces(space: &Space) -> Response {
    match space.host.spaces().await {
        Ok(listed) => {
            let listed = listed.unwrap_or_else(|| {
                json!([crate::spaces::describe(
                    &space.settings.dir,
                    space.settings.key.as_deref(),
                    space.catalog.root(),
                )])
            });
            ok(json!({ "current": space.settings.id, "spaces": listed }))
        }
        Err(error) => trouble(&error, 500),
    }
}

/// This space's address, or another listed space's, started first.
async fn open_space(space: &Space, id: &str) -> Result<Value> {
    if id == space.settings.id {
        return Ok(json!({ "url": format!("{}_workbench/", base(space)) }));
    }
    let listed = space.host.spaces().await?.unwrap_or_else(|| json!([]));
    let known = listed
        .as_array()
        .is_some_and(|spaces| spaces.iter().any(|candidate| candidate["id"] == id));
    if !known {
        return Err(refused(404, "That space isn’t in the Workbench list."));
    }
    let opened = space.host.open_space(id).await?;
    Ok(json!({ "url": opened["url"] }))
}

async fn spaces_open(space: &Space, request: Request) -> Response {
    let ask = match body(request, MAX_VIEW)
        .await
        .and_then(|bytes| parse(&bytes))
    {
        Ok(ask) => ask,
        Err(error) => return trouble(&error, 400),
    };
    let id = ask["id"].as_str().unwrap_or_default().to_owned();
    match open_space(space, &id).await {
        Ok(answer) => {
            if id != space.settings.id {
                space
                    .diagnostics
                    .info("space.opened", json!({ "space": id }))
                    .await;
            }
            ok(answer)
        }
        Err(error) => trouble(&error, 400),
    }
}

/// One of the fixed operations on another space: its config, or the
/// context its canvas reports. Not a general loopback proxy.
async fn canvas_space(space: &Space, ask: &Value) -> Result<Value> {
    let id = ask["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.chars().count() <= 200);
    let operation = ask["operation"]
        .as_str()
        .filter(|operation| matches!(*operation, "config" | "context"));
    let (Some(id), Some(operation)) = (id, operation) else {
        return Err(anyhow!("Invalid space operation."));
    };
    let opened = if id == space.settings.id {
        json!({ "url": format!("{}_workbench/", base(space)) })
    } else {
        open_space(space, id).await?
    };
    let url = opened["url"].as_str().unwrap_or_default().to_owned();
    let loopback = url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:");
    if !loopback {
        return Err(anyhow!("Space server must be on loopback."));
    }
    let origin = crate::space::origin_of(&url)
        .ok_or_else(|| anyhow!("Space server must be on loopback."))?;
    let answer = if operation == "config" {
        loopback_json(&origin, "/_workbench/config", None, Duration::from_secs(30)).await?
    } else {
        loopback_json(
            &origin,
            "/_workbench/view",
            Some(&ask["body"]),
            Duration::from_secs(10),
        )
        .await?
    };
    if answer["ok"] == false {
        return Err(anyhow!(
            answer["error"]
                .as_str()
                .unwrap_or("Space request failed.")
                .to_owned()
        ));
    }
    Ok(if operation == "config" {
        json!({ "url": url, "config": answer })
    } else {
        json!({ "ok": true })
    })
}

/// GETs, or POSTs `body` as JSON to, another space’s server on loopback.
async fn loopback_json(
    origin: &str,
    path: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<Value> {
    use http_body_util::BodyExt as _;
    let authority = origin.trim_start_matches("http://").to_owned();
    let call = async {
        let stream = tokio::net::TcpStream::connect(&authority).await?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream)).await?;
        tokio::spawn(async move {
            let _done = connection.await;
        });
        let builder = hyper::Request::builder()
            .uri(path)
            .header(hyper::header::HOST, &authority);
        let request = match body {
            Some(body) => builder
                .method(hyper::Method::POST)
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .body(http_body_util::Full::new(axum::body::Bytes::from(
                    body.to_string(),
                )))?,
            None => builder
                .method(hyper::Method::GET)
                .body(http_body_util::Full::new(axum::body::Bytes::new()))?,
        };
        let response = sender.send_request(request).await?;
        let status = response.status();
        let bytes = response.into_body().collect().await?.to_bytes();
        let answer: Value = serde_json::from_slice(&bytes)?;
        if !status.is_success() {
            return Err(anyhow!(
                answer["error"]
                    .as_str()
                    .unwrap_or("Space request failed.")
                    .to_owned()
            ));
        }
        anyhow::Ok(answer)
    };
    tokio::time::timeout(timeout, call)
        .await
        .map_err(|_| anyhow!("Space request timed out."))?
}

/// The editor opens a page's design file, or one of the places its code is:
/// only paths the configuration resolves to, never one a page names.
async fn open(space: &Space, request: Request) -> Result<Value> {
    let ask = parse(&body(request, MAX_HANDOFF).await?)?;
    let view = space.view().await?.unwrap_or(Value::Null);
    let src = ask["src"].as_str().unwrap_or_default();
    let page = view["pages"]
        .get(src)
        .filter(|page| page.is_object())
        .ok_or_else(|| refused(404, "that page isn’t in the workbench"))?;
    let wanted = ask["path"].as_str().filter(|path| !path.is_empty());
    let file = match wanted {
        None => page["design"].as_str().map(str::to_owned),
        Some(wanted) => page["code"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry["path"].as_str())
            .rfind(|path| *path == wanted)
            .map(str::to_owned),
    };
    let file = file.ok_or_else(|| refused(404, "not a path this workbench knows"))?;
    if !Path::new(&file).exists() {
        return Err(refused(404, format!("not on this machine: {file}")));
    }
    space.host.open(Path::new(&file)).await?;
    Ok(json!({ "opened": file }))
}

/// A handoff: the shot is on disk already; the prompt says it in words.
async fn handoff(space: &Space, request: Request) -> Response {
    let mut file = Value::Null;
    let result: Result<()> = async {
        let mut canvas = parse(&body(request, MAX_HANDOFF).await?)?;
        file = canvas.get("file").cloned().unwrap_or(Value::Null);
        if canvas["version"] != 2 {
            super::docs::handoff(space, &mut canvas).await;
        }
        let prompt = crate::handoff::prompt(&canvas)?;
        space.host.handoff(&prompt, &canvas).await
    }
    .await;
    match result {
        Ok(()) => {
            space
                .diagnostics
                .info(
                    "handoff.completed",
                    json!({ "file": file, "target": "clipboard" }),
                )
                .await;
            ok(json!({}))
        }
        Err(error) => {
            space
                .diagnostics
                .log(
                    "error",
                    "handoff.failed",
                    json!({ "file": file, "target": "clipboard", "message": format!("{error:#}") }),
                )
                .await;
            json_response(500, &json!({ "ok": false, "error": format!("{error:#}") }))
        }
    }
}

async fn review(space: &Space, request: Request) -> Response {
    let result: Result<Value> = async {
        let mut payload = parse(&body(request, MAX_HANDOFF).await?)?;
        if !payload["src"].is_string() {
            return Err(anyhow!("Invalid artboard review."));
        }
        super::docs::handoff(space, &mut payload).await;
        Ok(payload)
    }
    .await;
    match result {
        Ok(payload) => ok(json!({ "payload": payload })),
        Err(error) => trouble(&error, 400),
    }
}

pub(super) async fn answer(
    space: &Arc<Space>,
    path: &str,
    method: &Method,
    request: Request,
) -> Result<Response, Request> {
    let post = method == Method::POST;
    Ok(match path {
        "/_workbench/log" if post => log(space, request).await,
        "/_workbench/log" => wrong_method("POST a diagnostic here."),
        crate::agent_view::VIEW_PATH => view(space, method, request).await,
        "/_workbench/spaces" if is_get(method) => spaces(space).await,
        "/_workbench/spaces" => wrong_method("GET the spaces here."),
        "/_workbench/spaces/open" if !post => wrong_method("POST a space id here."),
        "/_workbench/spaces/open" if !same_origin(request.headers(), space.port) => json_response(
            403,
            &json!({ "ok": false, "error": "Only the workbench can switch spaces." }),
        ),
        "/_workbench/spaces/open" => spaces_open(space, request).await,
        "/_workbench/canvas/space" if !post => wrong_method("POST a canvas space operation."),
        "/_workbench/canvas/space" if !same_origin(request.headers(), space.port) => json_response(
            403,
            &json!({ "ok": false, "error": "Only this canvas can access space services." }),
        ),
        "/_workbench/canvas/space" => {
            match body(request, MAX_VIEW)
                .await
                .and_then(|bytes| parse(&bytes))
            {
                Ok(ask) => match canvas_space(space, &ask).await {
                    Ok(answer) => ok(answer),
                    Err(error) => trouble(&error, 400),
                },
                Err(error) => trouble(&error, 400),
            }
        }
        "/_workbench/canvas/review" if post => review(space, request).await,
        "/_workbench/canvas/review" => wrong_method("POST an artboard review."),
        "/_workbench/open" if post => match open(space, request).await {
            Ok(answer) => ok(answer),
            Err(error) => trouble(&error, 500),
        },
        "/_workbench/open" => wrong_method("POST the page here."),
        "/_workbench/handoff" if is_get(method) => {
            ok(json!({ "available": space.settings.handoff }))
        }
        "/_workbench/handoff" if post => handoff(space, request).await,
        "/_workbench/handoff" => wrong_method("POST the canvas here."),
        _ => return Err(request),
    })
}
