//! Independent retained observations and status, with caller-loss cancellation.
use crate::{
    error::{Error, Result},
    qa::{ReadContext, Server, journal::Archive},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{io::AsyncReadExt, net::UnixStream};

#[derive(Clone)]
pub(super) struct Handle {
    context: ReadContext,
    common: Value,
    archive: Archive,
}

/// The display a read hands to later input. Only an observation's display
/// identity (`{displayId, geometryVersion}`) qualifies; `ui_find` and
/// `ui_snapshot` also return a `display` holding the screen size, which input
/// must not adopt.
fn session_display(result: &Value) -> Option<Value> {
    result
        .get("display")
        .filter(|display| display.get("displayId").is_some())
        .cloned()
}

impl Handle {
    pub(super) fn new(server: &Server, common: &Value, archive: Archive) -> Result<Self> {
        Ok(Self {
            context: server.read_context(common)?,
            common: common.clone(),
            archive,
        })
    }
    pub(super) fn supports(&self, input: &Value) -> bool {
        matches!(
            input["kind"].as_str(),
            Some("control_status" | "control_grant")
        ) || input["kind"] == "observe"
            || input["kind"] == "status"
            || input["kind"] == "inspection"
                && input["tool"].as_str().is_some_and(ReadContext::supports)
            || input["kind"] == "playback"
                && self.context.has_media()
                && input["tool"].as_str().is_some_and(|name| {
                    crate::playback::operation(name) && !crate::playback::mutation(name)
                })
    }
    async fn result(self, input: Value) -> Result<Value> {
        super::server::validate_request(&input)?;
        if input["kind"] == "control_status" {
            return Ok(self.context.control_status());
        }
        if input["kind"] == "control_grant" {
            return self.context.control_grant(&input);
        }
        if input["kind"] == "playback" {
            let budget = super::server::probe_budget(
                &input,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            )?
            .min(Duration::from_secs(29));
            return tokio::time::timeout(
                budget,
                self.context.call(
                    input["tool"].as_str().unwrap_or(""),
                    input["arguments"].clone(),
                ),
            )
            .await
            .unwrap_or_else(|_| Err(Error::new("TIMEOUT", "Media read deadline expired")));
        }
        if input["kind"] == "status" {
            return if let Some(command) = input["commandId"].as_str() {
                self.archive.status(command)
            } else {
                self.context.status().await
            };
        }
        let (tool, mut args) = if input["kind"] == "observe" {
            (
                "screenshot".to_owned(),
                json!({"profile":input.get("profile").unwrap_or(&self.common["observationProfile"])}),
            )
        } else {
            let name = input["tool"]
                .as_str()
                .ok_or_else(|| Error::invalid("Inspection tool is required"))?;
            let args = input["arguments"]
                .as_object()
                .ok_or_else(|| Error::invalid("Inspection arguments must be an object"))?;
            if ["runId", "goalId", "sessionId", "leaseId", "commandId"]
                .iter()
                .any(|key| args.contains_key(*key))
            {
                return Err(Error::invalid(
                    "Inspection cannot replace session authority",
                ));
            }
            (name.to_owned(), Value::Object(args.clone()))
        };
        for key in ["runId", "goalId", "sessionId"] {
            args[key] = self.common[key].clone();
        }
        let definition =
            crate::catalog::definition(&tool).ok_or_else(|| Error::invalid("Unknown read tool"))?;
        crate::schema::validate(&definition["inputSchema"], &args)?;
        self.context.call(&tool, args).await
    }
    pub(super) async fn serve(self, mut socket: UnixStream, input: Value) -> Option<Value> {
        let mut byte = [0_u8; 1];
        let result = tokio::select! {
            // The client uses exactly one request per socket. EOF or extra data
            // abandons only this read; no mutation or lifecycle owner is dropped.
            _=socket.read(&mut byte)=>return None,
            result=tokio::time::timeout(Duration::from_secs(90),self.result(input))=>result.unwrap_or_else(|_|Err(Error::new("TIMEOUT","Retained read exceeded its budget"))),
        };
        let display = result.as_ref().ok().and_then(session_display);
        let response = match result {
            Ok(data) => json!({"ok":true,"data":data}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        let _ = tokio::time::timeout(Duration::from_secs(1), super::write(&mut socket, &response))
            .await;
        display
    }
    pub(super) fn prepare(&self, input: &mut Value) -> Result<()> {
        if input["kind"] == "control" {
            return self.context.plan_control(input);
        }
        let mutation = matches!(input["kind"].as_str(), Some("batch" | "application"))
            || input["kind"] == "playback"
                && input["tool"]
                    .as_str()
                    .is_some_and(crate::playback::mutation)
            || input["kind"] == "network" && input["operation"] != "status";
        if mutation {
            let lease = input["requestedLeaseId"]
                .as_str()
                .unwrap_or_else(|| self.common["leaseId"].as_str().unwrap_or(""));
            let credential = self
                .context
                .authority
                .admit(lease, std::time::Instant::now())?;
            input["leaseAtAdmission"] = json!(credential.id);
        }
        if input["kind"] == "viewer_batch" {
            self.context.authority.admit(
                input["viewerLeaseId"].as_str().unwrap_or(""),
                std::time::Instant::now(),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::session_display;
    use serde_json::json;

    #[test]
    fn only_observation_display_identity_reaches_later_input() {
        let observation = json!({"display":{"displayId":"main","geometryVersion":"960x2079"}});
        assert_eq!(
            session_display(&observation),
            Some(json!({"displayId":"main","geometryVersion":"960x2079"}))
        );
        let found = json!({"display":{"width":320,"height":693,"x":0,"y":0},"data":{}});
        assert_eq!(session_display(&found), None);
        assert_eq!(session_display(&json!({"data":{}})), None);
    }
}
