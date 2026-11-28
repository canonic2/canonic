//! Viewer handoff on a connected device, shared by every client; public
//! results omit private lease credentials and grants.
use super::Caller;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;
impl Caller {
    pub(super) async fn input_lease_for(
        &self,
        directory: &Path,
        name: &str,
        args: &Value,
    ) -> Result<Option<String>> {
        let application = matches!(name, "app_install" | "app_uninstall");
        if name != "act"
            && !crate::playback::mutation(name)
            && !crate::network::operation(name)
            && !application
        {
            return Ok(None);
        }
        // Existing receipts resolve their recorded result before new admission.
        if self.request_id.as_ref().is_some_and(|id| {
            let command = if crate::playback::mutation(name) {
                format!("media-{}", super::hash(id))
            } else if crate::network::operation(name) {
                format!("net-{}", super::hash(id))
            } else {
                id.clone()
            };
            directory
                .join("receipts")
                .join(format!("{}.json", super::hash(command)))
                .exists()
        }) {
            return Ok(None);
        }
        if application {
            let Ok(connection) = self.selected(directory) else {
                return Ok(None);
            };
            if args["targetId"]
                .as_str()
                .is_some_and(|target| target != connection.target)
            {
                return Ok(None);
            }
        }
        let connection = self.selected(directory)?;
        // Unfinished cleanup refuses input with its blocker at dispatch.
        if name == "act" && connection.admit_work().is_err() {
            return Ok(None);
        }
        let status = Self::send(&connection, json!({"kind":"control_status"})).await?;
        if status["owner"] != "agent" || status["state"] != "active" {
            return Err(Error::new(
                "CONTROL_NOT_OWNED",
                "Agent input is released or belongs to a viewer; explicitly acquire control before new input",
            ));
        }
        Ok(Some(
            status["leaseId"]
                .as_str()
                .ok_or_else(|| {
                    Error::new(
                        "OWNER_PROTOCOL_UNSUPPORTED",
                        "Controller does not provide input lease fencing",
                    )
                })?
                .into(),
        ))
    }
    pub(super) async fn control(&self, directory: &Path, name: &str, args: Value) -> Result<Value> {
        let connection = self.selected(directory)?;
        connection.admit_work()?;
        let input = if name == "control_status" {
            json!({"kind":"control_status"})
        } else {
            json!({"kind":"control","operation":if name == "control_acquire" {"acquire"} else {"release"},"arguments":args})
        };
        Self::send(&connection, input).await
    }
}
