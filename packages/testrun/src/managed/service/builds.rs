//! Build admission and device-stage composition inside the manager. Builds
//! are namespaced by the client's workspace; their device stages queue on the
//! target like any other client's requests.
use super::*;
use crate::builds::{Prepared, Request, Ticket, prepare};
use crate::managed::identity::Identity;
use futures_util::FutureExt;

struct BuildAdmission {
    service: Arc<Service>,
    operation: String,
}
impl Drop for BuildAdmission {
    fn drop(&mut self) {
        let mut registry = self.service.registry();
        registry.end_operation(&self.operation);
        registry.finish();
        drop(registry);
        self.service.notify();
    }
}

impl Service {
    pub(super) async fn build_call(self: &Arc<Self>, outer: Value) -> Result<Value> {
        let identity: Identity = serde_json::from_value(outer["identity"].clone())
            .map_err(|_| Error::invalid("Invalid client identity"))?;
        let workspace = identity.workspace_key();
        let name = outer["name"].as_str().unwrap_or_default();
        if name == "build_log" {
            return self.builds.log(&self.root, &workspace, &outer["args"]);
        }
        if !matches!(name, "build_start" | "project_start") {
            return self.builds.query(
                &self.root,
                &workspace,
                &outer["args"],
                name == "build_cancel",
            );
        }
        let request: Request = serde_json::from_value(outer["args"].clone())
            .map_err(|_| Error::invalid("Invalid build request"))?;
        // Resolve managed adapter prerequisites before admitting asynchronous work.
        // Custom commands remain exact argv and are checked when spawned.
        let request_id = outer["requestId"]
            .as_str()
            .ok_or_else(|| Error::invalid("Build requires request identity"))?;
        if let Some(record) = self
            .builds
            .replay(&self.root, &workspace, request_id, &request)?
        {
            return Ok(record);
        }
        let prepared = prepare(&request)?;
        let (mut record, ticket) = self
            .builds
            .begin(&self.root, &workspace, request_id, &request)?;
        if let Some(ticket) = ticket {
            match self.builds.identify(&ticket, &prepared) {
                Ok(value) => record = value,
                Err(error) => {
                    self.builds.finish(&ticket, Err(error.clone()));
                    return Err(error);
                }
            }
            let kind = if outer["client"]["kind"] == "mcp" {
                Kind::Mcp
            } else {
                Kind::Cli
            };
            let admitted = self.open_then(|registry| {
                registry.admit(None)?;
                registry.begin_operation(
                    &ticket.id,
                    "build",
                    identity.label.as_deref(),
                    kind,
                    None,
                );
                Ok(())
            });
            let admitted = match admitted {
                Ok(()) => BuildAdmission {
                    service: self.clone(),
                    operation: ticket.id.clone(),
                },
                Err(error) => {
                    self.builds.finish(&ticket, Err(error.clone()));
                    return Err(error);
                }
            };
            let service = self.clone();
            super::detached(move || async move {
                let _admitted = admitted;
                let result = std::panic::AssertUnwindSafe(
                    service.build_workflow(&ticket, &prepared, &request, &outer),
                )
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Error::new(
                        "INTERNAL",
                        "Build workflow stopped unexpectedly; inspect stage outcomes",
                    ))
                });
                service.builds.finish(&ticket, result);
                service.notify();
            });
        }
        Ok(record)
    }

    async fn build_workflow(
        self: &Arc<Self>,
        ticket: &Ticket,
        prepared: &Prepared,
        request: &Request,
        outer: &Value,
    ) -> Result<Value> {
        let artifact = self.builds.build(ticket, prepared).await?;
        if prepared.task != "build" {
            return Ok(artifact);
        }
        if *ticket.cancelled.borrow() {
            return Err(Error::new(
                "CANCELLED",
                "Build cancelled before device stages",
            ));
        }
        let mut result = json!({"artifact":artifact});
        if !request.install {
            return Ok(result);
        }
        let mut stages = vec![
            (
                "connect",
                json!({"targetId":request.target,"boot":request.boot}),
            ),
            (
                "install",
                json!({"targetId":request.target,"path":artifact["path"],"expectedSha256":artifact["sha256"],"replace":request.replace}),
            ),
        ];
        if request.launch {
            let mut action = json!({"action":"app_launch","appId":artifact["identity"]["appId"]});
            if !prepared.profile.launch.args.is_empty() {
                action["args"] = json!(prepared.profile.launch.args);
            }
            if !prepared.launch_env.is_empty() {
                action["env"] = json!(prepared.launch_env);
            }
            stages.push((
                "launch",
                json!({"targetId":request.target,"actions":[action],"observeAfterEach":false}),
            ));
        }
        for (stage, args) in stages {
            if *ticket.cancelled.borrow() {
                return Err(Error::new(
                    "CANCELLED",
                    "Workflow cancelled before the next device stage; completed stages remain",
                ));
            }
            self.builds
                .update(ticket, stage, json!({"state":"running"}))?;
            let mut call = outer.clone();
            call["name"] = json!(match stage {
                "install" => "app_install",
                "launch" => "act",
                _ => "connect",
            });
            call["args"] = args;
            call["requestId"] = json!(format!("{}-{stage}", ticket.id));
            // Each stage queues on the target like any client's request.
            let value = Box::pin(self.call(call)).await?;
            self.builds
                .update(ticket, stage, json!({"state":"completed","result":value}))?;
            result[stage] = value;
        }
        Ok(result)
    }
}
