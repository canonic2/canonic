//! Native Chrome Lighthouse, using its bundled worker and official report generator.
use super::*;
use crate::browser::{artifacts::Format, audits};

impl Worker {
    #[expect(
        clippy::too_many_lines,
        reason = "One audit transaction retains the frontend and reports through failure cleanup and original metric restoration"
    )]
    pub(super) async fn page_audit(
        &mut self,
        id: Uuid,
        page: Reference,
        options: audits::Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        if self.developers.audit.is_some() {
            return Err("AUDIT_CONFLICT".into());
        }
        self.developers.evidence.capacity_for(2, 32 * 1024 * 1024)?;
        let scope = self.page_scope(id, page, None, deadline).await?;
        let geometry = self
            .page_expression(
                id,
                &scope,
                "({width:innerWidth,height:innerHeight,dpr:devicePixelRatio})".into(),
                deadline,
            )
            .await?;
        self.inputs.invalidate(page);
        let opened = self
            .developer_open(id, page, Some("lighthouse".into()), deadline)
            .await?;
        let developer: Reference = serde_json::from_value(opened["developer"].clone())
            .map_err(|_| Error::from("BACKEND_PROTOCOL"))?;
        if opened["frontendOwned"] != true {
            self.developer_close(developer, deadline).await?;
            return Err("AUDIT_FRONTEND_CONFLICT".into());
        }
        let frontend = self.developer_session(developer)?;
        let run = Uuid::new_v4();
        if let Err(error) = self.developers.evidence.reserve(run, 2, 32 * 1024 * 1024) {
            self.developer_close(developer, deadline).await?;
            return Err(error);
        }
        self.developers.audit = Some((run, page));
        self.publish();
        let mut reports = Vec::new();
        let mut result=async{
        let mut input=json!(options);input["id"]=json!(run);
        let response=self.call(id,Some(&frontend),"Runtime.evaluate",json!({"expression":format!("({})({input})",audits::SCRIPT),"awaitPromise":true,"returnByValue":true}),deadline).await?;
        if response.get("exceptionDetails").is_some(){return Err(audit_error(&response));}
        let mut result=response["result"]["value"].clone();
        for (field,extension,mime,format)in[("json","json","application/json","lighthouse-result-json"),("html","html","text/html","lighthouse-report-html")]{
            let bytes=self.read_audit_report(&frontend,run,field,deadline).await?;
            self.developers.evidence.stage(run,Format{prefix:"lighthouse",extension,mime,name:format,max_bytes:16*1024*1024},bytes,json!({"page":page,"runId":run,"privacy":"private-audit-report","redacted":false,"complete":true}))?;
        }
        let finalized=self.developers.evidence.finalize(self.journal.directory(),run)?;
        reports=finalized["reports"].as_array().ok_or("BACKEND_PROTOCOL")?.clone();
        result["reports"]=json!(reports);result["page"]=json!(page);result["runId"]=json!(run);
        if !result["runtimeError"].is_null(){let mut error=Error::from("AUDIT_RUNTIME_ERROR");error.partial=Some(result);return Err(error);}
        Ok(result)
    }.await;
        let cleanup_deadline = Instant::now() + Duration::from_secs(5);
        if let Err(error) = &mut result {
            let progress = self.responsive_call(Some(&frontend),"Runtime.evaluate",json!({"expression":"globalThis[Symbol.for('canonic.testrun.developer.audit-progress.v1')]","returnByValue":true}),Lane::Ordinary,cleanup_deadline).await.ok();
            if error.partial.is_none() {
                error.partial = Some(json!({}));
            }
            if let Some(partial) = &mut error.partial {
                partial["progress"] = progress
                    .map(|reply| reply["result"]["value"].clone())
                    .unwrap_or(Value::Null);
                partial["developerState"] =
                    self.developer_status(developer).ok().unwrap_or(Value::Null);
            }
        }
        let expression = format!(
            "globalThis[Symbol.for('canonic.testrun.developer.reports.v1')]?.delete({});null",
            json!(run)
        );
        let _ = self
            .responsive_call(
                Some(&frontend),
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true}),
                Lane::Ordinary,
                cleanup_deadline,
            )
            .await;
        let frontend_closed = self.developer_close(developer, cleanup_deadline).await;
        self.developers.evidence.release(run);
        self.developers.audit = None;
        let restoration = if let Ok(route) = self.index.route(page, Some(Kind::Page)) {
            self.cleanup_call(Some(&route.session_id),"Emulation.setDeviceMetricsOverride",json!({"width":geometry["width"],"height":geometry["height"],"deviceScaleFactor":geometry["dpr"],"mobile":false}),cleanup_deadline).await.map(|_|())
        } else {
            Ok(())
        };
        self.inputs.invalidate(page);
        self.publish();
        if frontend_closed.is_err() || restoration.is_err() {
            let mut error = Error::from("AUDIT_CLEANUP_PENDING");
            error.partial = Some(
                json!({"page":page,"runId":run,"reports":reports,"dispatchCompleted":true,"restorationComplete":false,"originalError":result.as_ref().err().map(|error|json!({"code":error.code,"details":error.partial})),"frontendCleanup":frontend_closed.err().map(|error|error.code),"metricsRestoration":restoration.err().map(|error|error.code)}),
            );
            return Err(error);
        }
        result.map_err(|mut error: Error| {
            if error.partial.is_none() {
                error.partial = Some(
                    json!({"page":page,"runId":run,"reports":reports,"potentiallyMutating":true}),
                );
            }
            error
        })
    }
    async fn read_audit_report(
        &mut self,
        session: &str,
        run: Uuid,
        field: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        let mut offset = 0;
        let mut bytes = Vec::new();
        loop {
            let expression = format!(
                "(()=>{{const value=globalThis[Symbol.for('canonic.testrun.developer.reports.v1')]?.get({})?.[{}];if(typeof value!=='string')throw Error('Missing owned report');let end=Math.min(value.length,{}+32768);if(end<value.length&&end>0&&value.charCodeAt(end-1)>=0xD800&&value.charCodeAt(end-1)<=0xDBFF)end--;return {{chunk:value.slice({},end),next:end,eof:end===value.length}};}})()",
                json!(run),
                json!(field),
                offset,
                offset
            );
            let result = self
                .responsive_call(
                    Some(session),
                    "Runtime.evaluate",
                    json!({"expression":expression,"returnByValue":true}),
                    Lane::Ordinary,
                    deadline,
                )
                .await?;
            if result.get("exceptionDetails").is_some() {
                return Err("AUDIT_REPORT_LOST".into());
            }
            let value = &result["result"]["value"];
            let chunk = value["chunk"].as_str().ok_or("BACKEND_PROTOCOL")?;
            if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                return Err("EVIDENCE_BOUNDS".into());
            }
            bytes.extend_from_slice(chunk.as_bytes());
            if value["eof"] == true {
                return Ok(bytes);
            }
            let next = value["next"].as_u64().ok_or("BACKEND_PROTOCOL")?;
            if next <= offset {
                return Err("BACKEND_PROTOCOL".into());
            }
            offset = next;
        }
    }
}
fn audit_error(reply: &Value) -> Error {
    let text = reply["exceptionDetails"]["exception"]["description"]
        .as_str()
        .unwrap_or("Lighthouse execution failed");
    let code = [
        "AUDIT_CONFLICT",
        "AUDIT_ENGINE_MISMATCH",
        "AUDIT_URL_UNSUPPORTED",
        "EVIDENCE_BOUNDS",
        "EVIDENCE_CAPACITY",
    ]
    .into_iter()
    .find(|code| text.starts_with(&format!("Error: canonic:{code}")))
    .unwrap_or("AUDIT_FAILED");
    let mut error = Error::from(code);
    error.partial = Some(
        json!({"native":true,"exception":crate::browser::diagnostics::text(text).0,"dispatchCompleted":true}),
    );
    error
}
