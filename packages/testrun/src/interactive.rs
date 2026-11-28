//! Native CLI retention and private local requests over the same owned QA server as MCP.
use crate::{
    cli::{Action, Mobile},
    error::{Error, Result},
};
use serde_json::Value;
#[cfg(unix)]
use serde_json::json;

#[cfg(unix)]
pub(crate) use local::request;
#[cfg(unix)]
pub(crate) async fn serve_managed(
    input: &Mobile,
    owner: &crate::managed::Owner,
    server: &mut crate::qa::Server,
) -> Result<Value> {
    local::serve_with_owner(input, Some(owner), server).await
}
#[cfg(not(unix))]
pub(crate) async fn request(_path: &std::path::Path, _input: &Value) -> Result<Value> {
    Err(Error::new(
        "UNSUPPORTED_HOST",
        "Managed iOS control requires a Unix host",
    ))
}
#[cfg(not(unix))]
pub(crate) async fn serve_managed(
    _input: &Mobile,
    _owner: &crate::managed::Owner,
    _server: &mut crate::qa::Server,
) -> Result<Value> {
    Err(Error::new(
        "UNSUPPORTED_HOST",
        "Managed iOS control requires a Unix host",
    ))
}

pub(crate) async fn execute(input: &Mobile) -> Option<Result<Value>> {
    if !matches!(
        input.command,
        Action::Session { .. }
            | Action::Batch { .. }
            | Action::Status {
                endpoint: Some(_),
                ..
            }
            | Action::Close { .. }
    ) {
        return None;
    }
    Some(dispatch(input).await)
}

#[cfg(not(unix))]
async fn dispatch(_input: &Mobile) -> Result<Value> {
    Err(Error::new(
        "UNSUPPORTED_HOST",
        "Retained CLI sessions require Unix sockets",
    ))
}

#[cfg(unix)]
async fn dispatch(input: &Mobile) -> Result<Value> {
    match &input.command {
        // A foreground controller retains several QA futures. Keep its state
        // out of the CLI entry future on the process's main thread.
        Action::Session { .. } => Box::pin(local::serve(input)).await,
        Action::Batch {
            endpoint,
            command_id,
            no_observe,
            observe_after_each,
            observation_profile,
            steps,
        } => {
            let steps: Value = serde_json::from_str(steps)
                .map_err(|_| Error::invalid("Batch steps must be a JSON array"))?;
            let command_id = command_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let mut request = json!({"kind":"batch","commandId":command_id,"steps":steps,"observe":!no_observe,"observeAfterEach":observe_after_each});
            if let Some(profile) = observation_profile {
                request["observationProfile"] = json!(profile);
            }
            local::request(endpoint, &request).await
        }
        Action::Status {
            endpoint: Some(endpoint),
            command_id,
            ..
        } => local::request(endpoint, &json!({"kind":"status","commandId":command_id})).await,
        Action::Close { endpoint } => local::request(endpoint, &json!({"kind":"close"})).await,
        _ => Err(Error::new(
            "INTERNAL",
            "Unsupported retained-session request",
        )),
    }
}

#[cfg(unix)]
pub(crate) mod local;
