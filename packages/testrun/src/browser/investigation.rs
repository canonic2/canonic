//! Borrow settled browser evidence without submitting, cancelling or pinning browser work.
use super::{
    contract::{Reference, Request},
    lifecycle::State,
};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) fn snapshot(state: &State, id: Uuid, request: &Request) -> Result<Value> {
    let operation = state.operations.get(&id).ok_or_else(|| Error::new(
        "BROWSER_OPERATION_EXPIRED", "Select a retained operation on this exact browser connection; do not replay the original action"))?;
    let canonical =
        serde_json::to_vec(request).map_err(|_| Error::invalid("Invalid browser request"))?;
    if canonical.len() > 65536 {
        return Err(Error::new(
            "BROWSER_REQUEST_BOUNDS",
            "Select a native request of at most 64 KiB; large request bodies are not investigation metadata",
        ));
    }
    if operation.fingerprint != format!("{:x}", Sha256::digest(&canonical)) {
        return Err(Error::new(
            "BROWSER_OPERATION_MISMATCH",
            "Supply the original browser_send request; collection never submits it",
        ));
    }
    if !operation.settled {
        return Err(Error::new(
            "BROWSER_OPERATION_RUNNING",
            "Poll browser_status for this operation until settled, then retry the diagnostic submission",
        ));
    }
    let request: Value = serde_json::from_slice(&canonical).unwrap();
    let mut scopes = Vec::new();
    scopes_of(&request, state, &mut scopes);
    let value = json!({"schemaVersion":1,"targetId":super::runtime::target(state.connection),
        "engine":state.engine,"backend":state.engine.backend(),"sessionId":state.owner,"sessionGeneration":state.owner_generation,
        "browserIdentity":state.identity,"request":request,"operation":operation,"scopes":scopes,
        "rendererProcessId":null,"processGeneration":null,"observationComplete":false,
        "ownership":"immutable operation result copy; referenced files/models remain browser-owned and are not pinned or copied",
        "history":"selected settled result only; source cursor loss, native filtering and unsupported operations retain their original meaning"});
    Ok(value)
}

fn scopes_of(request: &Value, state: &State, scopes: &mut Vec<Value>) {
    // Only typed request authority fields, never evaluation arguments or result data.
    for key in [
        "context",
        "page",
        "scope",
        "worker",
        "snapshot",
        "handle",
        "profile",
        "developer",
        "frame",
        "httpContext",
        "callback",
    ] {
        if let Ok(reference) = serde_json::from_value::<Reference>(request[key].clone()) {
            let current = state.resource(reference, None).ok();
            scopes.push(json!({"field":key,"reference":reference,"currentlyLive":current.is_some(),
                "kind":current.map(|r|r.kind),"meaning":"original request reference; current liveness does not establish historical document/process identity"}));
        }
    }
    if let Some(requests) = request["requests"].as_array() {
        for child in requests.iter().take(8) {
            scopes_of(child, state, scopes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{
        contract::{ContextOptions, Engine, LaunchOptions, Mode},
        lifecycle::{Kind, Outcome},
        objects::ObjectTable,
    };
    #[test]
    fn diagnostic_browser_snapshot_verifies_request_settlement_generations_and_selected_only_reads()
    {
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            let mut state = State::new(
                Uuid::new_v4(),
                Uuid::new_v4(),
                &LaunchOptions {
                    media_fixtures: Default::default(),
                    engine,
                    mode: Mode::Headless,
                    proxy: None,
                    context: ContextOptions::default(),
                },
            );
            let mut objects = ObjectTable::default();
            let root = objects.lookup("", "Root").unwrap();
            let frame = state
                .issue(
                    Kind::Frame,
                    objects.create(&root, "frame", "Frame").unwrap(),
                    None,
                )
                .unwrap();
            let request: Request = serde_json::from_value(
                json!({"operation":"evaluate","scope":frame,"expression":"() => 42"}),
            )
            .unwrap();
            let id = Uuid::new_v4();
            state
                .begin(
                    id,
                    format!(
                        "{:x}",
                        Sha256::digest(serde_json::to_vec(&request).unwrap())
                    ),
                )
                .unwrap();
            assert_eq!(
                snapshot(&state, id, &request).unwrap_err().code,
                "BROWSER_OPERATION_RUNNING"
            );
            let op = state.operations.get_mut(&id).unwrap();
            op.settled = true;
            op.outcome = Outcome::Succeeded;
            op.result = Some(json!({"value":"private-selected"}));
            let unrelated = Uuid::new_v4();
            state.begin(unrelated, "other".into()).unwrap();
            state.operations.get_mut(&unrelated).unwrap().result =
                Some(json!({"private-neighbor":"must-not-copy"}));
            let captured = snapshot(&state, id, &request).unwrap();
            assert_eq!(captured["engine"], json!(engine));
            assert_eq!(captured["scopes"][0]["reference"], json!(frame));
            assert_eq!(captured["scopes"][0]["currentlyLive"], true);
            assert!(!captured.to_string().contains("must-not-copy"));
            for _ in 0..200 {
                let other = Uuid::new_v4();
                state.begin(other, "unrelated".into()).unwrap();
                state.operations.get_mut(&other).unwrap().result =
                    Some(json!({"private-neighbor":"must-not-copy"}));
            }
            assert_eq!(snapshot(&state, id, &request).unwrap(), captured);
            let wire = state.resources[&frame.id].wire.clone();
            state.navigate(&wire);
            assert_eq!(
                snapshot(&state, id, &request).unwrap()["scopes"][0]["currentlyLive"],
                false
            );
            assert_eq!(
                captured["scopes"][0]["reference"]["generation"],
                json!(frame.generation)
            );
            let other: Request = serde_json::from_value(json!({"operation":"page_list"})).unwrap();
            assert_eq!(
                snapshot(&state, id, &other).unwrap_err().code,
                "BROWSER_OPERATION_MISMATCH"
            );
            assert_eq!(
                snapshot(&state, Uuid::new_v4(), &request).unwrap_err().code,
                "BROWSER_OPERATION_EXPIRED"
            );
            assert_eq!(state.operations.len(), 202);
            assert!(!state.operations[&id].cancel_requested);
        }
    }
}
