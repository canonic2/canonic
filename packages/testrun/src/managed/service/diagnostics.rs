//! Diagnostic admission bypasses device mutation queues and retains its own work.
use super::*;
use crate::managed::identity::Identity;
impl Service {
    pub(super) async fn diagnostic_call(self: &Arc<Self>, outer: &Value) -> Result<Value> {
        let (mut result, perfetto_start) = self.admit_diagnostic(outer)?;
        if let Some(started) = perfetto_start {
            result["perfettoStart"] = match started.await {
                Ok(earliest_end) => {
                    let remaining =
                        earliest_end.saturating_duration_since(std::time::Instant::now());
                    json!({"acknowledged":true,"remainingMsAtReply":remaining.as_millis() as u64})
                }
                Err(_) => json!({"acknowledged":false,"remainingMsAtReply":0}),
            };
        }
        Ok(result)
    }
    fn admit_diagnostic(
        self: &Arc<Self>,
        outer: &Value,
    ) -> Result<(
        Value,
        Option<tokio::sync::oneshot::Receiver<std::time::Instant>>,
    )> {
        let _slot = self.admission.inspect()?;
        let identity: Identity = serde_json::from_value(outer["identity"].clone())
            .map_err(|_| Error::invalid("Invalid client identity"))?;
        let workspace = identity.workspace_key();
        let name = outer["name"].as_str().unwrap_or_default();
        let args = &outer["args"];
        let _registration = if matches!(
            name,
            "diagnostics_open" | "diagnostics_start" | "diagnostics_analyze" | "diagnostics_export"
        ) {
            let registry = self.registry();
            if registry.closing || registry.draining {
                return Err(Error::new(
                    "OWNER_DRAINING",
                    "Wait for manager retirement, then retry the same diagnostic submissionId",
                )
                .details(json!({"admitted":false,"dispatched":false})));
            }
            Some(registry)
        } else {
            None
        };
        let definition = crate::diagnostics::definition(name)
            .ok_or_else(|| Error::invalid("Unknown diagnostic operation"))?;
        crate::schema::validate(&definition["inputSchema"], args)?;
        if name == "diagnostics_analyze" {
            let (result, work) = self
                .diagnostics
                .prepare_analysis(&self.root, &workspace, args)?;
            if let Some(work) = work {
                let service = self.clone();
                super::detached(move || async move {
                    service.diagnostics.run_analysis(work).await;
                    service.notify();
                });
            }
            return Ok((result, None));
        }
        if name == "diagnostics_open" {
            return self
                .diagnostics
                .open(&self.root, &workspace, args)
                .map(|result| (result, None));
        }
        if name == "diagnostics_start" {
            let pool = _registration
                .as_ref()
                .and_then(|registry| registry.pool())
                .map(|pool| pool.id.clone());
            let (result, mut work) = self.diagnostics.start_with_browser(
                &self.root,
                &workspace,
                args,
                &self.builds,
                pool.as_deref().map(|pool| (&self.browsers, pool)),
            )?;
            let perfetto_start = work
                .as_mut()
                .and_then(crate::diagnostics::Work::perfetto_start);
            if let Some(work) = work {
                let service = self.clone();
                super::detached(move || async move {
                    service
                        .diagnostics
                        .run(
                            work,
                            service.root.clone(),
                            workspace,
                            service.builds.clone(),
                        )
                        .await;
                    service.notify();
                });
            }
            return Ok((result, perfetto_start));
        }
        self.diagnostics
            .inspect(&self.root, &workspace, name, args)
            .map(|result| (result, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::{identity::Identity, wire};
    fn message(identity: &Identity, name: &str, args: Value) -> Value {
        json!({"protocol":PROTOCOL,"control":CONTROL,"requires":["diagnostic_sessions_v1"],"kind":"call","requestId":uuid::Uuid::new_v4().to_string(),"identity":identity,"client":{"kind":"mcp","connection":"acme"},"name":name,"args":args})
    }
    #[tokio::test]
    async fn diagnostics_drain_refuses_new_ownership_before_files_are_created() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Service::fixture(tmp.path().join("state"), PROTOCOL, Duration::from_secs(5));
        let identity = Identity {
            label: None,
            workspace: tmp.path().into(),
            device_set: None,
        };
        service.drain();
        let error = service
            .diagnostic_call(&message(
                &identity,
                "diagnostics_open",
                json!({"submissionId":"late","question":"Acme"}),
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code, "OWNER_DRAINING");
        assert!(!service.root.join("diagnostics").exists());
        assert!(!service.diagnostics.active());
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn diagnostics_public_manager_lane_isolated_from_target_queue_and_other_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("state");
        let service = Service::fixture(root.clone(), PROTOCOL, Duration::from_secs(5));
        let identity = Identity {
            label: Some("acme-one".into()),
            workspace: tmp.path().join("workspace"),
            device_set: None,
        };
        let _turn = service
            .queue(Some("android:Acme"))
            .turn(Some(tokio::time::Instant::now() + Duration::from_secs(5)))
            .await
            .unwrap();
        let open = json!({"submissionId":"case","question":"Acme"});
        let result = service
            .request(message(&identity, "diagnostics_open", open.clone()))
            .await
            .unwrap();
        let id = result["investigationId"].as_str().unwrap();
        assert_eq!(
            service
                .request(message(&identity, "diagnostics_open", open))
                .await
                .unwrap()["investigationId"],
            id
        );
        let input = tmp.path().join("input");
        std::fs::write(&input, b"Acme captured").unwrap();
        let start = json!({"investigationId":id,"submissionId":"copy","source":{"kind":"import","path":input,"format":"bytes"}});
        service
            .request(message(&identity, "diagnostics_start", start.clone()))
            .await
            .unwrap();
        let mut second = identity.clone();
        second.label = Some("acme-two".into());
        let status = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let value = service
                    .request(message(
                        &second,
                        "diagnostics_status",
                        json!({"investigationId":id}),
                    ))
                    .await
                    .unwrap();
                if value["settled"] == true {
                    break value;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(status["collectors"][0]["phase"], "complete");
        std::fs::remove_file(input).unwrap();
        service
            .request(message(&identity, "diagnostics_start", start))
            .await
            .unwrap();
        let foreign = Identity {
            workspace: tmp.path().join("other"),
            ..identity.clone()
        };
        assert!(
            service
                .request(message(
                    &foreign,
                    "diagnostics_status",
                    json!({"investigationId":id})
                ))
                .await
                .is_err()
        );
        assert!(service.targets().is_empty());
        assert!(!service.diagnostics.active());
        assert!(wire::CAPABILITIES.contains(&"diagnostic_sessions_v1"));
    }
}
