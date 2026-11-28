//! Parsed private trace models live only in explicitly owned DevTools frontends.
use super::*;
const BRIDGE: &str = include_str!("../../reading/performance.js");
struct Analysis {
    id: Uuid,
    developer: Reference,
    evidence: Uuid,
    ready: bool,
}
#[derive(Default)]
pub(super) struct Performance {
    analysis: Option<Analysis>,
    pub(super) busy: Option<Reference>,
}
impl Worker {
    fn performance_key(&self) -> String {
        format!(
            "testrun:performance:{}",
            self.shared.state.lock().expect("Chrome owner").connection
        )
    }
    pub(super) fn performance_status(&self) -> Value {
        json!({"analysis":self.performance.analysis.as_ref().map(|a|json!({"analysisId":a.id,"developer":a.developer,"evidenceId":a.evidence,"ready":a.ready&&self.developer_session(a.developer).is_ok(),"scope":"owned-browser-process"}))})
    }
    fn performance_owner(&self, developer: Reference, id: Uuid) -> Result<(), Error> {
        let analysis = self
            .performance
            .analysis
            .as_ref()
            .ok_or("UNKNOWN_PERFORMANCE_ANALYSIS")?;
        if analysis.id != id {
            return Err("UNKNOWN_PERFORMANCE_ANALYSIS".into());
        }
        if analysis.developer != developer {
            return Err("FOREIGN_PERFORMANCE_ANALYSIS".into());
        }
        Ok(())
    }
    pub(super) async fn close_performance_model(
        &mut self,
        developer: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        let Some(analysis) = self
            .performance
            .analysis
            .as_ref()
            .filter(|a| a.developer == developer)
        else {
            return Ok(());
        };
        let id = analysis.id;
        if self.developer_status(developer)?["closed"] != true {
            let session = self
                .developer_session(developer)
                .map_err(|_| Error::from("PERFORMANCE_CLEANUP_PENDING"))?;
            let expression = format!(
                "({BRIDGE})({},'release',{},null)",
                json!(self.performance_key()),
                json!(id)
            );
            let reply = self
                .responsive_call(
                    Some(&session),
                    "Runtime.evaluate",
                    json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
                    Lane::Ordinary,
                    deadline,
                )
                .await?;
            if reply["result"]["value"] != true {
                let description = reply["exceptionDetails"]["exception"]["description"]
                    .as_str()
                    .unwrap_or("");
                if !description.starts_with("Error: canonic:UNKNOWN_PERFORMANCE_ANALYSIS") {
                    return Err("PERFORMANCE_CLEANUP_PENDING".into());
                }
            }
        }
        self.performance.analysis = None;
        Ok(())
    }
    async fn performance_bridge(
        &mut self,
        id: Uuid,
        developer: Reference,
        action: &str,
        analysis: Uuid,
        argument: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let session = self.developer_session(developer)?;
        let expression = format!(
            "({BRIDGE})({},{},{},{})",
            json!(self.performance_key()),
            json!(action),
            json!(analysis),
            argument
        );
        let reply = self
            .call(
                id,
                Some(&session),
                "Runtime.evaluate",
                json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
                deadline,
            )
            .await?;
        if self.developer_session(developer).ok().as_ref() != Some(&session) {
            return Err("DEVELOPER_SESSION_LOST".into());
        }
        if reply.get("exceptionDetails").is_some() {
            let description = reply["exceptionDetails"]["exception"]["description"]
                .as_str()
                .unwrap_or("");
            let code = [
                "PERFORMANCE_MODEL_CAPACITY",
                "UNKNOWN_PERFORMANCE_ANALYSIS",
                "PERFORMANCE_TRACE_BOUNDS",
                "PERFORMANCE_TRACE_METADATA_UNAVAILABLE",
                "PERFORMANCE_MODEL_PENDING",
                "UNKNOWN_INSIGHT_SET",
                "UNKNOWN_PERFORMANCE_INSIGHT",
                "PERFORMANCE_RESULT_BOUNDS",
            ]
            .into_iter()
            .find(|code| description.starts_with(&format!("Error: canonic:{code}")))
            .unwrap_or("PERFORMANCE_MODEL_FAILED");
            return Err(code.into());
        }
        let value = reply["result"]["value"].clone();
        if matches!(action, "initialize" | "append" | "release") {
            if value != true {
                return Err("PERFORMANCE_MODEL_FAILED".into());
            }
        } else if value.get("data").is_none() || !value["truncated"].is_boolean() {
            return Err("PERFORMANCE_MODEL_FAILED".into());
        }
        if value.to_string().len() > 65536 {
            return Err("PERFORMANCE_RESULT_BOUNDS".into());
        }
        Ok(value)
    }
    async fn performance_load(
        &mut self,
        id: Uuid,
        developer: Reference,
        evidence: Uuid,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.performance.analysis.is_some() {
            return Err("PERFORMANCE_MODEL_CAPACITY".into());
        }
        if self.developer_status(developer)?["frontendOwned"] != true {
            return Err("PERFORMANCE_OWNED_FRONTEND_REQUIRED".into());
        }
        let artifact = self.developers.evidence.get(evidence)?;
        if artifact.summary["format"] != "chrome-trace-event-json"
            || artifact.summary["complete"] != true
        {
            return Err("PERFORMANCE_EVIDENCE_FORMAT".into());
        }
        let bytes = String::from_utf8(artifact.read()?)
            .map_err(|_| Error::from("PERFORMANCE_EVIDENCE_FORMAT"))?;
        let analysis = Uuid::new_v4();
        self.performance.analysis = Some(Analysis {
            id: analysis,
            developer,
            evidence,
            ready: false,
        });
        self.performance.busy = Some(developer);
        let result: Result<Value, Error> = async {
            self.performance_bridge(id, developer, "initialize", analysis, Value::Null, deadline)
                .await?;
            let mut start = 0;
            while start < bytes.len() {
                let mut end = (start + 65536).min(bytes.len());
                while !bytes.is_char_boundary(end) {
                    end -= 1;
                }
                self.performance_bridge(
                    id,
                    developer,
                    "append",
                    analysis,
                    json!(&bytes[start..end]),
                    deadline,
                )
                .await?;
                start = end;
            }
            self.performance_bridge(id, developer, "finish", analysis, Value::Null, deadline)
                .await
        }
        .await;
        self.performance.busy = None;
        match result {
            Ok(summary) => {
                self.performance
                    .analysis
                    .as_mut()
                    .ok_or("UNKNOWN_PERFORMANCE_ANALYSIS")?
                    .ready = true;
                Ok(
                    json!({"analysisId":analysis,"developer":developer,"evidenceId":evidence,"summary":summary,"backend":"bundled-native-DevTools-trace-model","scope":"owned-browser-process","contextIsSelectionOnly":true}),
                )
            }
            Err(mut error) => {
                error.partial = Some(
                    json!({"analysisId":analysis,"developer":developer,"evidenceId":evidence,"modelRetained":true,"nextAction":"performance_release","recollected":false}),
                );
                Err(error)
            }
        }
    }
    pub(super) async fn performance_operation(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.performance.analysis.as_ref().is_some_and(|a| {
            self.developer_status(a.developer)
                .is_ok_and(|status| status["closed"] == true)
        }) {
            self.performance.analysis = None;
        }
        match request {
            Request::PerformanceLoad {
                developer,
                evidence_id,
            } => {
                self.performance_load(id, developer, evidence_id, deadline)
                    .await
            }
            Request::PerformanceQuery {
                developer,
                analysis_id,
                query,
            } => {
                query.validate()?;
                self.performance_owner(developer, analysis_id)?;
                if !self.performance.analysis.as_ref().is_some_and(|a| a.ready) {
                    return Err("PERFORMANCE_MODEL_PENDING".into());
                }
                self.performance.busy = Some(developer);
                let result = self
                    .performance_bridge(id, developer, "query", analysis_id, json!(query), deadline)
                    .await;
                self.performance.busy = None;
                result.map(|result|json!({"analysisId":analysis_id,"result":result,"private":true,"backend":"bundled-native-DevTools-trace-model"}))
            }
            Request::PerformanceRelease {
                developer,
                analysis_id,
            } => {
                self.performance_owner(developer, analysis_id)?;
                self.performance.busy = Some(developer);
                let result = self.close_performance_model(developer, deadline).await;
                self.performance.busy = None;
                result.map(
                    |()| json!({"analysisId":analysis_id,"released":true,"evidencePreserved":true}),
                )
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
}
