//! Every request on the space's port, in order: previews from the native
//! compiler, files served as they are (Workbench's own, the manifests and
//! the project's), the space's routes, and then any other project file,
//! including Markdown served as it is.
use super::files::Files;
use super::messages::Output;
use crate::space::Space;
use axum::{
    Router,
    extract::{Request, State},
    response::Response,
};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
struct Served {
    space: Arc<Space>,
    files: Arc<Files>,
    output: Output,
}

pub(crate) fn router(space: Arc<Space>, files: Arc<Files>, output: Output) -> Router {
    Router::new().fallback(dispatch).with_state(Served {
        space,
        files,
        output,
    })
}

/// Fails to compile, naming the cause, when a route’s future can’t move between threads.
#[expect(dead_code, reason = "a compile-time check")]
fn routes_are_send(space: &Arc<Space>, request: Request) {
    fn check<F: std::future::Future + Send>(_future: F) {}
    check(super::routes::answer(space, request));
}

async fn dispatch(State(served): State<Served>, request: Request) -> Response {
    if !served.space.settings.trace {
        return answer(served, request).await;
    }
    let started = std::time::Instant::now();
    let path = request.uri().path().to_owned();
    let mut params: std::collections::HashMap<_, _> =
        url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes())
            .into_owned()
            .collect();
    // Stable assets can inherit navigation identity from their referring
    // document without changing their cache URLs. Never log the full referrer.
    if let Some(referer) = request
        .headers()
        .get("referer")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| url::Url::parse(value).ok())
    {
        for (name, value) in referer.query_pairs() {
            if matches!(name.as_ref(), "wbTrace" | "wbPage" | "wbState" | "wbLens") {
                params
                    .entry(name.into_owned())
                    .or_insert_with(|| value.into_owned());
            }
        }
    }
    let timings = crate::trace::Timings::default();
    let mut response = crate::trace::REQUEST
        .scope(timings.clone(), answer(served.clone(), request))
        .await;
    let total = started.elapsed().as_secs_f64() * 1000.0;
    let phases = timings.values();
    let mut header = format!("total;dur={total:.3}");
    let mut details = json!({
        "route": path, "page": params.get("wbPage").or_else(|| params.get("page")).or_else(|| params.get("file")).map_or(path.trim_start_matches('/'), String::as_str),
        "lens": params.get("wbLens").or_else(|| params.get("lens")), "state": params.get("wbState").or_else(|| params.get("state")),
        "selection": params.get("wbTrace"), "status": response.status().as_u16(), "elapsedMs": total,
    });
    for (name, ms) in phases {
        use std::fmt::Write as _;
        let _ = write!(header, ", {name};dur={ms:.3}");
        details[format!("{name}Ms")] = json!(ms);
    }
    if let Ok(value) = header.parse() {
        response.headers_mut().insert("server-timing", value);
    }
    served
        .space
        .diagnostics
        .info("preview.request.completed", details)
        .await;
    response
}

async fn answer(served: Served, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let previews = &served.space.previews;
    if super::previews::handles(&path) {
        // The catalog configures previews; a preview can be the first thing
        // anyone asks for.
        if !previews.enabled().await {
            let _assembled = served.space.view().await;
        }
        return super::previews::handle(
            previews.clone(),
            request,
            &served.output,
            served.space.settings.trace,
        )
        .await;
    }
    if let Some(response) = served.files.answer(request.method(), &path) {
        if matches!(path.as_str(), "/_workbench/" | "/_workbench/index.html") {
            served
                .space
                .diagnostics
                .info("canvas.served", json!({}))
                .await;
        }
        return response;
    }
    let request = match super::routes::answer(&served.space, request).await {
        Ok(response) => return response,
        Err(request) => request,
    };
    served.files.fallback(request.method(), &path)
}
