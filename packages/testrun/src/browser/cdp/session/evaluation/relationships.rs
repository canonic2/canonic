//! Live element relationships keep source-world fences and exact context/page ownership.
use super::super::execution_groups::Purpose;
use super::*;

impl Worker {
    pub(super) async fn evaluation_handle_frame(
        &mut self,
        id: Uuid,
        reference: Reference,
        owner: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.require_handle(reference)?;
        let handle = self
            .evaluation
            .handles
            .get(&reference)
            .ok_or("STALE_REFERENCE")?
            .clone();
        self.index
            .native(handle.scope, Kind::Frame)
            .map_err(|_| Error::from("INVALID_SCOPE"))?;
        let page = self
            .index
            .owning_page(handle.scope)
            .map_err(resource_error)?;
        self.index
            .route(handle.world, Some(Kind::World))
            .map_err(resource_error)?;
        let context = self
            .index
            .diagnostic_scope(&handle.session)
            .ok_or("INVALID_SCOPE")?
            .0;
        let key = self.evaluation.groups.begin(
            context,
            handle.session.clone(),
            Purpose::FrameRelationship,
        )?;
        let result = async {
            let extracted = self.execution_group_call(id, &key, "Runtime.callFunctionOn", json!({"objectId":handle.object,"functionDeclaration":"function(){return this.value;}","returnByValue":false,"awaitPromise":false,"objectGroup":key.1}), deadline).await?;
            self.index.route(handle.world, Some(Kind::World)).map_err(resource_error)?;
            if extracted.get("exceptionDetails").is_some() { return Err(evaluation_error(&extracted)); }
            if extracted["result"]["subtype"] != "node" { return Err("INVALID_SCOPE".into()); }
            let object = extracted["result"]["objectId"].as_str().ok_or("BACKEND_PROTOCOL")?;
            let object = if owner {
                // Match the pinned Chromium delegate, including Document nodes.
                let document = self.execution_group_call(id, &key, "Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(){const node=this;const doc=node;if(doc.documentElement&&doc.documentElement.ownerDocument===doc)return doc.documentElement;return node.ownerDocument?node.ownerDocument.documentElement:null;}","returnByValue":false,"awaitPromise":true,"userGesture":true,"objectGroup":key.1}), deadline).await?;
                self.index.route(handle.world, Some(Kind::World)).map_err(resource_error)?;
                if document.get("exceptionDetails").is_some() { return Err(evaluation_error(&document)); }
                if document["result"]["subtype"] == "null" { return Ok(None); }
                document["result"]["objectId"].as_str().ok_or("BACKEND_PROTOCOL")?.to_owned()
            } else { object.to_owned() };
            let described = self.execution_group_call(id, &key, "DOM.describeNode", json!({"objectId":object,"depth":0}), deadline).await?;
            self.index.route(handle.world, Some(Kind::World)).map_err(resource_error)?;
            let node = &described["node"];
            if !owner && !matches!(node["nodeName"].as_str(), Some("IFRAME" | "FRAME")) { return Ok(None); }
            let Some(native) = node["frameId"].as_str() else { return Ok(None); };
            Ok(if owner { self.index.context_frame(context, native) } else { self.index.event_frame(&handle.session, native) })
        }.await;
        self.release_execution_group(&key, deadline.min(Instant::now() + Duration::from_secs(2)))
            .await;
        let pending = self.evaluation.groups.contains(&key);
        let result = result.and_then(|frame: Option<Reference>| {
            self.index
                .route(handle.world, Some(Kind::World))
                .map_err(|_| Error::from("DOCUMENT_CHANGED"))?;
            if let Some(frame) = frame {
                self.index
                    .native(frame, Kind::Frame)
                    .map_err(|_| Error::from("DOCUMENT_CHANGED"))?;
            }
            Ok(frame)
        });
        let frame: Option<Reference> = result.map_err(|mut error: Error| {
            let cause=error.partial.take();
            let mut partial=json!({"handle":reference,"relationshipCleanupPending":pending,"potentiallyMutating":owner,"cause":cause});
            for field in ["scopeClosed","callerEffectsUnknown"] {
                if let Some(value)=cause.as_ref().and_then(|cause| cause.get(field)) {partial[field]=value.clone();}
            }
            error.partial=Some(partial);
            error
        })?;
        let related_page = frame.and_then(|frame| self.index.owning_page(frame).ok());
        Ok(
            json!({"handle":reference,"frame":frame,"page":related_page,"sourcePage":page,"relationship":if owner {"owner-frame"} else {"content-frame"},"relationshipCleanupPending":pending,"potentiallyMutating":owner}),
        )
    }
}
