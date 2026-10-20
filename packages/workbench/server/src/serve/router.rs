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
    let path = request.uri().path().to_owned();
    let previews = &served.space.previews;
    if super::previews::handles(&path) {
        // The catalog configures previews; a preview can be the first thing
        // anyone asks for.
        if !previews.enabled().await {
            let _assembled = served.space.view().await;
        }
        return super::previews::handle(previews.clone(), request, &served.output).await;
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
