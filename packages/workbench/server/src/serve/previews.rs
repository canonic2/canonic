//! HTTP adapter for the previews service: `/_workbench/previews/` and
//! `*.workbench.ts(x)` page addresses, answered from the native compiler.
//!
//! - `/rspack/events`: built, docs-changed and reload events for frames;
//! - `/rspack/module/<slug>.js`: the module a warm host imports;
//! - `/rspack/<profile>/<generation>/<file>`: built output from memory;
//! - `/descriptor?file=` and `/page?file=`: a preview's descriptor and page;
//! - `/docs/index?request=` and `/docs/bundle?request=`: a docs lens;
//! - `/problems`: the project configuration's problems.
use crate::compiler::Previews;
use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

pub(crate) const PREFIX: &str = "/_workbench/previews";

/// Whether a request is for the previews service.
pub(crate) fn handles(path: &str) -> bool {
    path.starts_with(&format!("{PREFIX}/"))
        || (!path.starts_with("/_workbench/")
            && (path.ends_with(".workbench.ts") || path.ends_with(".workbench.tsx")))
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |offset: usize| {
            bytes
                .get(at + offset)
                .and_then(|byte| (*byte as char).to_digit(16))
        };
        match bytes[at] {
            b'%' => {
                if let (Some(high), Some(low)) = (hex(1), hex(2)) {
                    out.push(u8::try_from(high * 16 + low).unwrap_or(b'?'));
                    at += 3;
                } else {
                    out.push(b'%');
                    at += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_path(value: &str) -> String {
    decode(&value.replace('+', "%2B"))
}

fn param(query: Option<&str>, name: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (decode(key) == name).then(|| decode(value))
    })
}

fn json_response(status: StatusCode, value: &Value) -> Response {
    let mut response = (status, value.to_string()).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn failure(error: &anyhow::Error) -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        &json!({ "error": format!("{error:#}") }),
    )
}

fn text(status: StatusCode, content_type: &'static str, body: impl Into<Body>) -> Response {
    let mut response = Response::new(body.into());
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn not_found(message: &'static str) -> Response {
    text(StatusCode::NOT_FOUND, "text/plain; charset=utf-8", message)
}

pub(crate) fn mime(name: &str) -> &'static str {
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

fn request_param(query: Option<&str>) -> Value {
    param(query, "request")
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null)
}

/// One event stream per open canvas, with a heartbeat.
fn events(previews: &Arc<Previews>) -> Response {
    let receiver = previews.subscribe();
    let stream = futures::stream::unfold((receiver, true), |(mut receiver, first)| async move {
        if first {
            return Some((
                Ok::<_, std::convert::Infallible>(": connected\n\n".to_owned()),
                (receiver, false),
            ));
        }
        let next = tokio::time::timeout(Duration::from_secs(15), receiver.recv()).await;
        let chunk = match next {
            Ok(Ok(message)) => format!("data: {message}\n\n"),
            // A slow reader missed events; the next build brings it current.
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) | Err(_) => {
                ": heartbeat\n\n".to_owned()
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
        };
        Some((Ok(chunk), (receiver, false)))
    });
    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    response
}

async fn output(previews: &Arc<Previews>, rest: &str) -> Response {
    let Some((profile, rest)) = rest.split_once('/') else {
        return not_found("Unknown preview profile");
    };
    match previews.output(profile, rest).await {
        Some((body, hot)) => {
            let mut response = text(StatusCode::OK, mime(rest), body);
            // Scripts and assets have content-hashed names; stylesheets keep
            // stable names for CSS hot updates, and hot updates change every build.
            let cache = if hot || mime(rest) == "text/css" {
                "no-cache"
            } else {
                "public, max-age=31536000, immutable"
            };
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
            response
        }
        None => not_found("Not found"),
    }
}

/// Answers one request and reports its timing: always for the routes that
/// build, and for any request that took half a second or more.
pub(crate) async fn handle(
    previews: Arc<Previews>,
    request: Request,
    output: &super::messages::Output,
) -> Response {
    let started = std::time::Instant::now();
    let path = request.uri().path().to_owned();
    // A page address names its definition by its path.
    let file = param(request.uri().query(), "file")
        .or_else(|| (!path.starts_with(PREFIX)).then(|| decode_path(path.trim_start_matches('/'))));
    let response = answer(previews, request).await;
    let elapsed = started.elapsed().as_millis();
    let route = path.strip_prefix(PREFIX).unwrap_or("/page").to_owned();
    let traced = [
        "/page",
        "/descriptor",
        "/render",
        "/docs/bundle",
        "/portable/preview",
        "/portable/docs",
    ]
    .contains(&route.as_str());
    if traced || elapsed >= 500 {
        let details = json!({
            "route": route, "file": file, "status": response.status().as_u16(),
            "queueMs": 0, "workMs": elapsed, "elapsedMs": elapsed,
        });
        let _sent = output
            .diagnostic("preview.request.completed", details)
            .await;
    }
    response
}

async fn answer(previews: Arc<Previews>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    let query = query.as_deref();
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return text(
            StatusCode::METHOD_NOT_ALLOWED,
            "text/plain; charset=utf-8",
            "GET a Workbench preview here.",
        );
    }
    if !previews.enabled().await {
        return not_found("Workbench previews are unavailable.");
    }
    let Some(rest) = path.strip_prefix(PREFIX) else {
        let relative = decode_path(path.trim_start_matches('/'));
        return match previews.page(&relative).await {
            Ok(page) => text(StatusCode::OK, "text/html; charset=utf-8", page),
            Err(error) => failure(&error),
        };
    };
    let rest = decode_path(rest);
    if rest == "/rspack/events" {
        return events(&previews);
    }
    if let Some(slug) = rest
        .strip_prefix("/rspack/module/")
        .and_then(|name| name.strip_suffix(".js"))
    {
        return match previews.module(slug).await {
            Ok(Some(source)) => text(StatusCode::OK, "text/javascript", source),
            Ok(None) => not_found("Unknown preview"),
            Err(error) => failure(&error),
        };
    }
    if let Some(output_path) = rest.strip_prefix("/rspack/") {
        return output(&previews, output_path).await;
    }
    let file = param(query, "file").unwrap_or_default();
    match rest.as_str() {
        "/descriptor" => match previews.descriptor(&file).await {
            Ok(descriptor) => json_response(StatusCode::OK, &descriptor),
            Err(error) => failure(&error),
        },
        "/page" => match previews.page(&file).await {
            Ok(page) => text(StatusCode::OK, "text/html; charset=utf-8", page),
            Err(error) => failure(&error),
        },
        "/docs/index" => match previews.docs_index(&request_param(query)).await {
            Ok(answer) => json_response(StatusCode::OK, &answer),
            Err(error) => failure(&error),
        },
        "/docs/bundle" => match previews.docs_bundle(&request_param(query)).await {
            Ok(answer) => json_response(StatusCode::OK, &answer),
            Err(error) => failure(&error),
        },
        "/problems" => json_response(StatusCode::OK, &json!(previews.problems().await)),
        "/portable/preview" => match previews.portable_preview(&file).await {
            Ok(answer) => json_response(StatusCode::OK, &answer),
            Err(error) => failure(&error),
        },
        "/portable/docs" => match previews.portable_docs(&request_param(query)).await {
            Ok(answer) => json_response(StatusCode::OK, &answer),
            Err(error) => failure(&error),
        },
        "/render" => {
            let inputs = param(query, "inputs")
                .map_or(Ok(json!({})), |raw| serde_json::from_str::<Value>(&raw))
                .ok()
                .filter(Value::is_object);
            let Some(inputs) = inputs else {
                return failure(&anyhow::anyhow!("Astro inputs must be an object."));
            };
            let state = param(query, "state").unwrap_or_else(|| "default".into());
            match previews.render(&file, &state, &inputs).await {
                Ok(answer) => json_response(StatusCode::OK, &answer),
                Err(error) => failure(&error),
            }
        }
        other if other.starts_with("/preview/") => {
            let rest = &other["/preview/".len()..];
            match rest.split_once('/').and_then(|(name, asset)| {
                previews
                    .variant_asset(name, asset)
                    .map(|body| (asset.to_owned(), body))
            }) {
                Some((asset, body)) => text(StatusCode::OK, mime(&asset), body),
                None => not_found("Preview asset not found"),
            }
        }
        _ => not_found("Preview asset not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_addresses_and_parameters() {
        assert!(handles("/_workbench/previews/descriptor"));
        assert!(handles("/src/button.workbench.ts"));
        assert!(!handles("/_workbench/config"));
        assert!(!handles("/_workbench/ui/x.workbench.ts"));
        assert_eq!(
            param(Some("file=src%2Fbutton.workbench.ts&x=1"), "file").as_deref(),
            Some("src/button.workbench.ts")
        );
        assert_eq!(decode_path("a%20b+c.md"), "a b+c.md");
        assert_eq!(mime("chunks/app.css"), "text/css");
    }
}
