//! Screenshots: the canvas asks, the request is checked here, and the host's
//! capture helper takes the picture. A local capture is a page of this
//! workbench; a page capture is a page an implementation serves, taken by
//! the helper going there itself, since the frame can't be read across
//! origins. A saved screenshot lands in the project's handoffs folder.
use super::{
    MAX_CAPTURE, MAX_SHOT, base, body, json_response, ok, query, respond, trouble, wrong_method,
};
use crate::capture;
use crate::space::Space;
use anyhow::{Result, anyhow};
use axum::{
    extract::Request,
    http::{Method, header},
    response::Response,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn image(body: Vec<u8>, format: &str) -> Response {
    let length = body.len();
    let mut response = respond(
        200,
        body,
        if format == "jpeg" {
            "image/jpeg"
        } else {
            "image/png"
        },
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, length.into());
    response
}

async fn requested(
    space: &Space,
    event: &str,
    surface: &str,
    payload: &Value,
    details: Option<&Value>,
) {
    let scroll = details.and_then(|details| details.get("scroll"));
    let mut fields = json!({
        "surface": surface, "mirrored": !payload["mirror"].is_null(), "width": payload["width"], "height": payload["height"],
        "requestedX": payload["scroll"]["x"], "requestedY": payload["scroll"]["y"], "rendererReported": scroll.is_some(),
    });
    // What the renderer applied, only when it reported it.
    if let Some(scroll) = scroll {
        fields["appliedX"] = scroll["appliedX"].clone();
        fields["appliedY"] = scroll["appliedY"].clone();
    }
    space.diagnostics.info(event, fields).await;
}

/// A screenshot of a page an implementation serves.
async fn page(space: &Space, path: &str, request: Request, review: bool) -> Result<Response> {
    let bytes = body(request, MAX_CAPTURE).await?;
    let overlay = capture::overlay_css(space.annotations_css());
    let payload = capture::page(&bytes, &space.origins(), &overlay)?;
    let at = base(space);
    let mirrored = !payload["mirror"].is_null();
    if path == "/_workbench/capture/page/prepare" {
        if mirrored {
            space
                .host
                .capture("prepare", &at, &capture::bridged(&payload, &at), None)
                .await?;
        } else {
            space
                .host
                .capture("preparePage", &at, &payload, None)
                .await?;
        }
        return Ok(ok(json!({ "prepared": true })));
    }
    requested(space, "capture.requested", "implementation", &payload, None).await;
    let shot = if mirrored {
        space
            .host
            .capture("capture", &at, &capture::bridged(&payload, &at), None)
            .await?
    } else {
        space
            .host
            .capture("capturePage", &at, &payload, None)
            .await?
    };
    requested(
        space,
        "capture.completed",
        "implementation",
        &payload,
        shot.details.as_ref(),
    )
    .await;
    if shot.body.is_empty() {
        return Err(anyhow!("native capture returned an empty image"));
    }
    let format = payload["format"].as_str().unwrap_or("png");
    let targets = shot.targets.clone().or_else(|| {
        shot.details
            .as_ref()
            .and_then(|details| details.get("targets").cloned())
    });
    if path == "/_workbench/capture/page/image" {
        if review {
            use base64::Engine as _;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&shot.body);
            let kind = if format == "jpeg" {
                "image/jpeg"
            } else {
                "image/png"
            };
            return Ok(ok(
                json!({ "image": encoded, "type": kind, "targets": targets }),
            ));
        }
        return Ok(image(shot.body, format));
    }
    let file = capture::save(space.catalog.root(), &payload["name"], &shot.body, format)?;
    space.host.shot(&file).await;
    Ok(ok(json!({ "file": file, "targets": targets })))
}

/// A screenshot of a page of this workbench.
async fn local(space: &Space, path: &str, request: Request) -> Result<Response> {
    let bytes = body(request, MAX_CAPTURE).await?;
    let at = base(space);
    let payload = capture::local(&bytes, &at)?;
    if path == "/_workbench/capture/prepare" {
        space.host.capture("prepare", &at, &payload, None).await?;
        return Ok(ok(json!({ "prepared": true })));
    }
    requested(space, "capture.requested", "local", &payload, None).await;
    let shot = space.host.capture("capture", &at, &payload, None).await?;
    requested(
        space,
        "capture.completed",
        "local",
        &payload,
        shot.details.as_ref(),
    )
    .await;
    if shot.body.is_empty() {
        return Err(anyhow!("native capture returned an empty image"));
    }
    let format = payload["format"].as_str().unwrap_or("png");
    if path == "/_workbench/capture/image" {
        return Ok(image(shot.body, format));
    }
    let file = capture::save(space.catalog.root(), &payload["name"], &shot.body, format)?;
    space.host.shot(&file).await;
    Ok(ok(json!({ "file": file })))
}

/// A PNG or JPEG the canvas made itself.
async fn shot(space: &Space, request: Request) -> Result<Response> {
    let jpeg = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|kind| kind.split(';').next().unwrap_or_default().trim() == "image/jpeg");
    let name = query(&request, "name").map_or(Value::Null, Value::String);
    let bytes = body(request, MAX_SHOT).await?;
    if bytes.is_empty() {
        return Err(anyhow!("that shot was empty"));
    }
    let file = capture::save(
        space.catalog.root(),
        &name,
        &bytes,
        if jpeg { "jpeg" } else { "png" },
    )?;
    space.host.shot(&file).await;
    Ok(ok(json!({ "file": file })))
}

fn unavailable(error: &anyhow::Error, status: u16) -> Response {
    json_response(
        crate::refused::status_of(error, status),
        &json!({ "ok": false, "error": format!("{error:#}") }),
    )
}

pub(super) async fn answer(
    space: &Arc<Space>,
    path: &str,
    method: &Method,
    request: Request,
) -> Result<Response, Request> {
    let post = method == Method::POST;
    let routes = [
        "/_workbench/capture/page",
        "/_workbench/capture/page/image",
        "/_workbench/capture/page/prepare",
        "/_workbench/capture/warm",
        "/_workbench/capture/prepare",
        "/_workbench/capture",
        "/_workbench/capture/image",
        "/_workbench/shot",
    ];
    if !routes.contains(&path) {
        return Err(request);
    }
    if !post {
        return Ok(wrong_method(match path {
            "/_workbench/shot" => "POST a PNG or JPEG here.",
            "/_workbench/capture/warm" => "POST here to warm native capture.",
            _ => "POST a capture request here.",
        }));
    }
    Ok(match path {
        "/_workbench/capture/page"
        | "/_workbench/capture/page/image"
        | "/_workbench/capture/page/prepare" => {
            let review = query(&request, "review").as_deref() == Some("1");
            match page(space, path, request, review).await {
                Ok(response) => response,
                Err(error) => trouble(&error, 503),
            }
        }
        "/_workbench/capture/warm" => match space
            .host
            .capture("warm", &base(space), &Value::Null, None)
            .await
        {
            Ok(_) => ok(json!({ "warmed": true })),
            Err(error) => unavailable(&error, 503),
        },
        "/_workbench/shot" => match shot(space, request).await {
            Ok(response) => response,
            Err(error) => unavailable(&error, 500),
        },
        _ => match local(space, path, request).await {
            Ok(response) => response,
            Err(error) => {
                json_response(503, &json!({ "ok": false, "error": format!("{error:#}") }))
            }
        },
    })
}
