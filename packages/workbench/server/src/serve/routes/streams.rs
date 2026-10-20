//! Implementation lenses that are not pages: a Storybook's stories under a
//! title, native window streams and iOS Simulators. A stream request only
//! says which page; the app, window and device come from the configuration,
//! so a page can't ask for any other. The streams themselves are the
//! capture part's, served by the host on a local socket this server
//! forwards to.
use super::{MAX_HANDOFF, body, ok, parse, query, text, trouble, wrong_method};
use crate::catalog::page_items;
use crate::refused::refused;
use crate::space::Space;
use anyhow::Result;
use axum::{
    body::Body,
    extract::Request,
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const WINDOW_STREAM_PATH: &str = "/_workbench/window/stream";

async fn stories(space: &Space, key: String, title: String) -> Result<Value> {
    let view = space.view().await?.unwrap_or(Value::Null);
    let stories = crate::storybook::stories(&view["implementations"], &key, &title).await?;
    Ok(json!({ "implementation": key, "title": title, "stories": stories }))
}

/// The stream's address for the canvas, from what the host started.
fn stream_answer(started: &Value, codec: &str) -> Value {
    let token = started["token"].as_str().unwrap_or_default();
    json!({
        "stream": format!("{WINDOW_STREAM_PATH}?token={}", crate::catalog::encode_component(token)),
        "source": started["source"],
        "codec": started["codec"].as_str().filter(|codec| !codec.is_empty()).unwrap_or(codec),
    })
}

async fn window(space: &Space, request: Request) -> Result<Value> {
    let ask = parse(&body(request, MAX_HANDOFF).await?)?;
    let view = space.view().await?.unwrap_or(Value::Null);
    let key = ask["implementation"].as_str().unwrap_or_default();
    let src = ask["src"].as_str().unwrap_or_default();
    let implementation = &view["implementations"][key];
    let title = view["pages"]
        .get(src)
        .map(|page| &page["windows"][key])
        .filter(|title| title.as_str().is_some_and(|title| !title.is_empty()));
    let (Some(title), true) = (title, implementation["kind"] == "window") else {
        return Err(refused(403, "that window isn’t declared by this workbench"));
    };
    let id = format!("{key}\n{src}");
    if ask["stop"].as_bool().unwrap_or(false)
        || ask["stop"].as_str().is_some_and(|stop| !stop.is_empty())
    {
        space.host.stream("stop", &json!({ "id": id })).await?;
        return Ok(json!({ "stopped": true }));
    }
    let codec = if ask["codec"] == "jpeg" {
        "jpeg"
    } else {
        "h264"
    };
    let started = space
        .host
        .stream(
            "start",
            &json!({ "app": implementation["app"], "source": title, "id": id, "codec": codec }),
        )
        .await?;
    Ok(stream_answer(&started, codec))
}

/// Whether the Simulator `udid` is one this workbench declares for `key`.
fn simulator_allowed(
    view: &Value,
    key: &str,
    udid: &str,
    device: Option<&crate::simulators::Device>,
) -> bool {
    let prefix = format!("__ios-simulator/{key}/");
    let catalogued = view["pages"]
        .as_object()
        .into_iter()
        .flatten()
        .any(|(src, page)| src.starts_with(&prefix) && page["simulator"]["udid"] == udid);
    if catalogued {
        return true;
    }
    let Some(device) = device else {
        return false;
    };
    let collections = view["collections"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    page_items(collections).iter().any(|item| {
        let wanted = &item["implementations"][key]["device"];
        wanted == device.udid.as_str() || wanted == device.name.as_str()
    })
}

async fn simulator(space: &Space, path: &str, request: Request) -> Result<Value> {
    let mut ask = parse(&body(request, MAX_HANDOFF).await?)?;
    let view = space.view().await?.unwrap_or(Value::Null);
    let key = ask["implementation"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut udid = ask["udid"].as_str().unwrap_or_default().to_owned();
    let implementation = &view["implementations"][&key];
    let simulator = implementation["kind"] == "ios-simulator";
    let devices = if simulator {
        space.devices().await?
    } else {
        Vec::new()
    };
    let device = devices
        .iter()
        .find(|device| device.udid == udid || device.name == udid);
    if !simulator || !simulator_allowed(&view, &key, &udid, device) {
        return Err(refused(
            403,
            "that Simulator isn’t declared by this workbench",
        ));
    }
    if let Some(device) = device {
        udid.clone_from(&device.udid);
    }
    ask["udid"] = json!(udid);
    match path {
        "/_workbench/simulator/stream" => {
            if ask["stop"].as_bool().unwrap_or(false) {
                space.host.stream("stop", &json!({ "id": udid })).await?;
                return Ok(json!({ "stopped": true }));
            }
            let source = if let Some(device) = device {
                json!(device.name)
            } else {
                let prefix = format!("__ios-simulator/{key}/");
                view["pages"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .find(|(src, page)| {
                        src.starts_with(&prefix) && page["simulator"]["udid"] == udid.as_str()
                    })
                    .map_or(Value::Null, |(_, page)| page["label"].clone())
            };
            let codec = if ask["codec"] == "jpeg" {
                "jpeg"
            } else {
                "h264"
            };
            let started = space
                .host
                .stream(
                    "start",
                    &json!({ "source": source, "app": "simulator", "id": udid, "codec": codec }),
                )
                .await?;
            Ok(stream_answer(&started, codec))
        }
        "/_workbench/simulator" => {
            let screen = space.sandbox.screen(&udid).await?;
            Ok(json!({ "udid": udid, "screen": screen }))
        }
        _ => space.sandbox.input(&udid, &ask).await,
    }
}

/// A window stream's frames: the request goes to the host's socket as it
/// came, upgraded to a WebSocket or streamed over HTTP.
async fn forward_stream(space: &Space, mut request: Request) -> Result<Response> {
    let socket = space.host.stream_socket().await?;
    let upgrade = request
        .headers()
        .contains_key(header::UPGRADE)
        .then(|| hyper::upgrade::on(&mut request));
    let stream = connect(&socket).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        let _done = connection.with_upgrades().await;
    });
    let target = request
        .uri()
        .path_and_query()
        .map_or(WINDOW_STREAM_PATH, |target| target.as_str())
        .to_owned();
    *request.uri_mut() = target.parse()?;
    request
        .headers_mut()
        .insert(header::HOST, header::HeaderValue::from_static("localhost"));
    let mut response = sender.send_request(request).await?;
    if response.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client) = upgrade
    {
        let server = hyper::upgrade::on(&mut response);
        tokio::spawn(async move {
            if let (Ok(client), Ok(server)) = tokio::join!(client, server) {
                let _closed = tokio::io::copy_bidirectional(
                    &mut TokioIo::new(client),
                    &mut TokioIo::new(server),
                )
                .await;
            }
        });
    }
    Ok(response.map(Body::new).into_response())
}

#[cfg(unix)]
async fn connect(socket: &std::path::Path) -> Result<tokio::net::UnixStream> {
    Ok(tokio::net::UnixStream::connect(socket).await?)
}

#[cfg(windows)]
async fn connect(
    socket: &std::path::Path,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    Ok(tokio::net::windows::named_pipe::ClientOptions::new().open(socket)?)
}

pub(super) async fn answer(
    space: &Arc<Space>,
    path: &str,
    method: &Method,
    request: Request,
) -> Result<Response, Request> {
    let post = method == Method::POST;
    Ok(match path {
        "/_workbench/stories" => match stories(
            space,
            query(&request, "implementation").unwrap_or_default(),
            query(&request, "title").unwrap_or_default(),
        )
        .await
        {
            Ok(answer) => ok(answer),
            Err(error) => trouble(&error, 500),
        },
        WINDOW_STREAM_PATH
            if request.headers().contains_key(header::UPGRADE) || method == Method::GET =>
        {
            match forward_stream(space, request).await {
                Ok(response) => response,
                Err(_) => text(403, "That window stream is not active."),
            }
        }
        WINDOW_STREAM_PATH if post => match window(space, request).await {
            Ok(answer) => ok(answer),
            Err(error) => trouble(&error, 500),
        },
        WINDOW_STREAM_PATH => wrong_method("POST a window stream request here."),
        "/_workbench/simulator"
        | "/_workbench/simulator/input"
        | "/_workbench/simulator/stream"
            if !post =>
        {
            wrong_method("POST a Simulator request here.")
        }
        "/_workbench/simulator"
        | "/_workbench/simulator/input"
        | "/_workbench/simulator/stream" => match simulator(space, path, request).await {
            Ok(answer) => ok(answer),
            Err(error) => trouble(&error, 500),
        },
        _ => return Err(request),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulators::Device;

    #[test]
    fn a_simulator_is_allowed_when_catalogued_or_named_by_a_page() {
        let view = json!({
            "pages": { "__ios-simulator/ios/AAA.html": { "simulator": { "udid": "AAA" } } },
            "collections": [{ "name": "App", "items": [{ "src": "a.html", "implementations": { "ios": { "device": "iPhone 16" } } }] }],
        });
        let device = Device {
            name: "iPhone 16".into(),
            udid: "BBB".into(),
            runtime: "iOS 18".into(),
        };
        assert!(simulator_allowed(&view, "ios", "AAA", None));
        assert!(simulator_allowed(&view, "ios", "BBB", Some(&device)));
        assert!(!simulator_allowed(&view, "ios", "CCC", None));
        assert!(!simulator_allowed(&view, "other", "AAA", None));
    }
}
