//! App deployment reuses the target's connected device actor or, on a target
//! that is not connected, takes temporary exclusive leases for the call.
use super::*;
use std::path::Path;

impl Caller {
    pub(super) async fn application(
        &self,
        directory: &Path,
        name: &str,
        mut args: Value,
    ) -> Result<Value> {
        let operation = if name.ends_with("app_install") {
            "install"
        } else {
            "uninstall"
        };
        let selected = match self.selected(directory) {
            Ok(c) => Some(c),
            Err(e) if e.code == "NOT_CONNECTED" => None,
            Err(e) => return Err(e),
        };
        if !args["targetId"].is_string() {
            args["targetId"] = json!(
                selected
                    .as_ref()
                    .ok_or_else(|| Error::invalid(
                        "Select a target or supply targetId before managing apps"
                    ))?
                    .target
            );
        }
        let request: crate::applications::Request = serde_json::from_value(args)
            .map_err(|_| Error::invalid("Invalid app management arguments"))?;
        crate::applications::validate(&request.target_id)?;
        // The service already queued this call on the pooled target.
        let reference = format!(
            "app-{}",
            self.request_id
                .as_ref()
                .map_or_else(|| uuid::Uuid::new_v4().simple().to_string(), hash)
        );
        let connected = selected
            .filter(|c| {
                c.target == request.target_id
                    || self
                        .service
                        .as_ref()
                        .and_then(|service| service.held_target(&request.target_id))
                        .as_deref()
                        == Some(c.target.as_str())
            })
            .map(|connection| Ok::<_, Error>((connection.held()?, connection)))
            .transpose()?
            .filter(|(held, _)| *held)
            .map(|(_, connection)| connection);
        // A connected target keeps its app records with its own; an operation
        // without a connection records in the pool, which status reads.
        let root = match &connected {
            Some(_) => directory.join("applications"),
            None => self.pool_records()?.join("applications"),
        };
        storage::directory(&root)?;
        // A compatible device actor keeps its target lease throughout the mutation.
        if let Some(connection) = connected {
            return Self::send(&connection,json!({"kind":"application","requestedLeaseId":self.input_lease,"operation":operation,"actionId":reference,"request":request})).await
                .map_err(|error|if matches!(error.code.as_str(),"TRANSPORT_IO"|"SESSION_ENDPOINT_LOST") {Error::new("APP_OUTCOME_UNKNOWN","App request reply was interrupted; inspect status before another mutation").details(json!({"cause":error,"actionId":reference}))}else{error});
        }
        let _owner =
            ControlLease::acquire(&format!("managed:{}", request.target_id)).map_err(app_busy)?;
        let _target = ControlLease::acquire(&request.target_id).map_err(app_busy)?;
        crate::applications::execute(&root, &reference, operation, request).await
    }
}
fn app_busy(error: Error) -> Error {
    if error.code == "TARGET_BUSY" {
        Error::new("DEVICE_IN_USE", "Another Testrun manager holds the device")
            .details(json!({"blockingState":"external_owner"}))
    } else {
        error
    }
}
