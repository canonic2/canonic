//! Single-use file chooser callbacks on the reserved response lane.
use super::*;

impl Worker {
    pub(super) fn chooser_event(&mut self, event: &Event) -> Result<(), Error> {
        if event.method != "Page.fileChooserOpened" {
            return Ok(());
        }
        let session = event.session_id.as_deref().ok_or("BACKEND_PROTOCOL")?;
        let frame = self
            .index
            .event_frame(
                session,
                event.params["frameId"].as_str().ok_or("BACKEND_PROTOCOL")?,
            )
            .ok_or("FRAME_UNAVAILABLE")?;
        let page = self.index.owning_page(frame).map_err(resource_error)?;
        if self.transfers.choosers.len() >= 64 {
            return Err("CALLBACK_CAPACITY".into());
        }
        let reference = self.transfer_reference();
        self.transfers.choosers.insert(
            reference,
            transfers::Chooser {
                page,
                frame,
                session: session.into(),
                node: event.params["backendNodeId"]
                    .as_i64()
                    .ok_or("BACKEND_PROTOCOL")?,
                multiple: event.params["mode"] == "selectMultiple",
                expires: Instant::now() + Duration::from_secs(30),
            },
        );
        self.record_activity(
            page,
            crate::browser::activity::Event::FileChooser,
            reference,
        );
        Ok(())
    }
    pub(super) async fn resolve_chooser(
        &mut self,
        id: Uuid,
        reference: Reference,
        fixtures: Vec<Reference>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if !self.transfers.responding.insert(reference) {
            return Err("CALLBACK_ALREADY_RESOLVING".into());
        }
        let result = self
            .resolve_chooser_once(id, reference, fixtures, deadline)
            .await;
        self.transfers.responding.remove(&reference);
        result
    }
    async fn resolve_chooser_once(
        &mut self,
        id: Uuid,
        reference: Reference,
        fixtures: Vec<Reference>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        self.observation_ready(id, deadline)?;
        let chooser = self
            .transfers
            .choosers
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        if chooser.expires <= Instant::now() {
            return Err("CALLBACK_EXPIRED".into());
        }
        if !chooser.multiple && fixtures.len() > 1 {
            return Err("SINGLE_FILE_INPUT".into());
        }
        let frame = chooser.frame;
        let session = chooser.session.clone();
        let node = chooser.node;
        let directory = self.directory_parameters(&fixtures).await?;
        let (payloads, summaries) = if directory.is_some() {
            (Vec::new(), Vec::new())
        } else {
            crate::browser::transfer::payloads(fixtures, |fixture| self.fixture_bytes(fixture))
                .await?
        };
        self.index
            .route(frame, Some(Kind::Frame))
            .map_err(resource_error)?;
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        let group = format!("testrun-chooser-{}", reference.id);
        let result=async{
            let world=connection.call(Some(&session),"Page.createIsolatedWorld",json!({"frameId":self.index.native(frame,Kind::Frame).map_err(resource_error)?,"worldName":page::WORLD,"grantUniveralAccess":false}),Lane::Ordinary,deadline).await.map_err(protocol_error)?;
            self.drain()?;self.index.route(frame,Some(Kind::Frame)).map_err(resource_error)?;
            connection.call(Some(&session),"Runtime.evaluate",json!({"expression":format!("(()=>{{{};return true;}})()",crate::browser::reading::selector("chromium")),"contextId":world["executionContextId"],"returnByValue":true}),Lane::Ordinary,deadline).await.map_err(protocol_error)?;
            let object=connection.call(Some(&session),"DOM.resolveNode",json!({"backendNodeId":node,"executionContextId":world["executionContextId"],"objectGroup":group}),Lane::Ordinary,deadline).await.map_err(protocol_error)?;
            self.drain()?;self.index.route(frame,Some(Kind::Frame)).map_err(resource_error)?;
            if let Some((path,summary))=&directory{
                let kind=connection.call(Some(&session),"Runtime.callFunctionOn",json!({"objectId":object["object"]["objectId"],"functionDeclaration":"function(){return this.webkitdirectory===true;}","returnByValue":true}),Lane::Ordinary,deadline).await.map_err(protocol_error)?;
                if kind["result"]["value"]!=true{return Err("DIRECTORY_INPUT_REQUIRED".into());}
                self.transfers.choosers.remove(&reference);
                self.journal.record(json!({"operationId":id,"stage":"dispatch_admitted","method":"DOM.setFileInputFiles"})).await?;
                self.responsive_call(Some(&session),"DOM.setFileInputFiles",json!({"backendNodeId":node,"files":[path]}),Lane::Ordinary,deadline).await?;
                return Ok(json!({"chooser":reference,"fixtures":[summary],"attached":true,"serverUploadVerified":false}));
            }
            self.transfers.choosers.remove(&reference);
            self.journal.record(json!({"operationId":id,"stage":"dispatch_admitted","method":"Runtime.callFunctionOn"})).await?;
            let function="function(payloads){const injected=globalThis[Symbol.for('canonic.testrun.selector.v3')];if(this.webkitdirectory&&payloads.length)return {error:'DIRECTORY_INPUT_REQUIRED'};const result=injected.setInputFiles(this,payloads);return result?{error:'FILE_ATTACH_FAILED'}:{attached:true};}";
            let mut params = json!({"objectId":object["object"]["objectId"],"functionDeclaration":function,"arguments":[{}],"returnByValue":true,"awaitPromise":true});
            params["arguments"][0]["value"] = Value::Array(payloads);
            let result=self.responsive_call_raw(Some(&session),"Runtime.callFunctionOn",params,Lane::Ordinary,deadline,true).await?.map_err(protocol_error)?;
            if result.get("exceptionDetails").is_some(){return Err(page::execution_error(&result));}
            page::query_result(result["result"]["value"].clone())?;
            Ok(json!({"chooser":reference,"fixtures":summaries,"attached":true,"serverUploadVerified":false}))
        }.await;
        let released = connection
            .call(
                Some(&session),
                "Runtime.releaseObjectGroup",
                json!({"objectGroup":group}),
                Lane::Control,
                Instant::now() + Duration::from_secs(2),
            )
            .await;
        result.and_then(|value| released.map(|_| value).map_err(protocol_error))
    }
}
