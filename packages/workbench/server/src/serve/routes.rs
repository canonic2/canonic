//! The space's own routes: configuration and its editing, diagnostics, the
//! agent view, spaces, opening sources, handoffs, screenshots, stories,
//! docs pages, exports and native streams. Each answers as the Node server
//! did, with the same statuses and messages; anything else is not a route.
mod canvas;
mod capture;
mod config;
mod docs;
mod export;
mod streams;

use crate::refused::status_of;
use crate::space::Space;
use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use std::sync::Arc;

/// Bodies the routes read, by kind.
pub(super) const MAX_SHOT: usize = 64 * 1024 * 1024;
pub(super) const MAX_CAPTURE: usize = 16 * 1024 * 1024;
pub(super) const MAX_HANDOFF: usize = 1024 * 1024;
pub(super) const MAX_LOG: usize = 16 * 1024;
pub(super) const MAX_VIEW: usize = 256 * 1024;

pub(super) fn respond(status: u16, body: impl Into<Body>, kind: &'static str) -> Response {
    let mut response = Response::new(body.into());
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    // The whole point is seeing edits, so nothing is cached.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) fn json_response(status: u16, body: &Value) -> Response {
    let text = body.to_string();
    let length = text.len();
    let mut response = respond(status, text, "application/json; charset=utf-8");
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    response
}

pub(super) fn text(status: u16, message: &str) -> Response {
    respond(status, message.to_owned(), "text/plain; charset=utf-8")
}

/// `{ ok: false, error }` with the error's own status, or `fallback`.
pub(super) fn trouble(error: &anyhow::Error, fallback: u16) -> Response {
    json_response(
        status_of(error, fallback),
        &json!({ "ok": false, "error": format!("{error:#}") }),
    )
}

/// `{ ok: true, ...fields }`.
pub(super) fn ok(fields: Value) -> Response {
    let mut body = json!({ "ok": true });
    if let (Some(out), Value::Object(fields)) = (body.as_object_mut(), fields) {
        out.extend(fields);
    }
    json_response(200, &body)
}

/// The request's body, refused past `limit` bytes.
pub(super) async fn body(request: Request, limit: usize) -> anyhow::Result<Bytes> {
    let limited = http_body_util::Limited::new(request.into_body(), limit);
    match limited.collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(error) if error.is::<http_body_util::LengthLimitError>() => {
            Err(anyhow::anyhow!("that shot is too big"))
        }
        Err(error) => Err(anyhow::anyhow!("couldn’t read the request: {error}")),
    }
}

pub(super) fn parse(bytes: &[u8]) -> anyhow::Result<Value> {
    Ok(serde_json::from_slice(bytes)?)
}

/// A browser names the page a request came from. One from another site, or
/// another of these servers, is refused; a request without an Origin is not
/// a page's at all: curl, or a test.
pub(super) fn same_origin(headers: &HeaderMap, port: u16) -> bool {
    if let Some(site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        && site != "same-origin"
        && site != "none"
    {
        return false;
    }
    match headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        None => true,
        Some(origin) => {
            origin == format!("http://127.0.0.1:{port}")
                || origin == format!("http://localhost:{port}")
        }
    }
}

/// The address every URL for the browser or the capture helper starts with.
pub(super) fn base(space: &Space) -> String {
    format!("http://127.0.0.1:{}/", space.port)
}

pub(super) fn query(request: &Request, name: &str) -> Option<String> {
    let query = request.uri().query()?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (decode(key) == name).then(|| decode(value))
    })
}

/// A query component, with `+` as a space and `%XX` escapes decoded.
pub(super) fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => out.push(b' '),
            b'%' if at + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[at + 1..at + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                {
                    Some(byte) => {
                        out.push(byte);
                        at += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The space's answer to a request, or the request back when it is not one
/// of its routes.
pub(crate) async fn answer(space: &Arc<Space>, request: Request) -> Result<Response, Request> {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    let request = match config::answer(space, &path, &method, request).await {
        Ok(response) => return Ok(response),
        Err(request) => request,
    };
    let request = match canvas::answer(space, &path, &method, request).await {
        Ok(response) => return Ok(response),
        Err(request) => request,
    };
    let request = match capture::answer(space, &path, &method, request).await {
        Ok(response) => return Ok(response),
        Err(request) => request,
    };
    let request = match streams::answer(space, &path, &method, request).await {
        Ok(response) => return Ok(response),
        Err(request) => request,
    };
    let request = match docs::answer(space, &path, &method, request).await {
        Ok(response) => return Ok(response),
        Err(request) => request,
    };
    export::answer(space, &path, &method, request).await
}

/// `405` with what the route takes, for a method it does not.
pub(super) fn wrong_method(message: &str) -> Response {
    text(405, message)
}

pub(super) fn is_get(method: &Method) -> bool {
    method == Method::GET || method == Method::HEAD
}
