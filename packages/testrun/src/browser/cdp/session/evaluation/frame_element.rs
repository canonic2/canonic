//! Embedding elements belong to the exact parent realm and their own releasable group.
use super::super::execution_groups::Purpose;
use super::*;

impl Worker {
    pub(super) async fn evaluation_frame_element(
        &mut self,
        id: Uuid,
        frame: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        // A retained owner resolves to its current document frame.
        let document = if self.index.native(frame, Kind::FrameOwner).is_ok() {
            self.index.owner_frame(frame).map_err(resource_error)?
        } else {
            frame
        };
        let native = self
            .index
            .native(document, Kind::Frame)
            .map_err(resource_error)?;
        let parent = self
            .index
            .parent_frame(document)
            .map_err(|_| Error::from("INVALID_SCOPE"))?;
        let (_, world, session, _) = self.execution_scope(parent)?;
        let route = self
            .index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        let numeric = route.execution_numeric_id.ok_or("INVALID_SCOPE")?;
        let unique = route.execution_unique_id.ok_or("INVALID_SCOPE")?;
        let context = self
            .index
            .diagnostic_scope(&session)
            .ok_or("INVALID_SCOPE")?
            .0;
        self.execution_cleanup_capacity()?;
        self.evaluation
            .groups
            .capacity(Purpose::FrameRelationship)?;
        let allocation = self.reserve_handle_allocation(id, world, &session)?;
        let key =
            self.evaluation
                .groups
                .begin(context, session.clone(), Purpose::FrameRelationship)?;
        let result: Result<Value, Error> = async {
            let owner = self.execution_group_call(id, &key, "DOM.getFrameOwner", json!({"frameId":native}), deadline).await?;
            self.frame_element_fence(frame, parent, world)?;
            let backend = owner["backendNodeId"].as_u64().ok_or("BACKEND_PROTOCOL")?;
            let (resolved, settled) = self.execution_group_call_settled(id, &key, "DOM.resolveNode", json!({"backendNodeId":backend,"executionContextId":numeric,"objectGroup":key.1}), deadline).await;
            if let Some(owner) = self.evaluation.allocations.get_mut(&allocation) { owner.settled = settled; }
            let resolved = resolved?;
            self.frame_element_fence(frame, parent, world)?;
            if resolved["object"]["subtype"] != "node" { return Err("BACKEND_PROTOCOL".into()); }
            let object = resolved["object"]["objectId"].as_str().ok_or("BACKEND_PROTOCOL")?;
            let (wrapped, settled) = self.execution_group_call_settled(id, &key, "Runtime.callFunctionOn", json!({"uniqueContextId":unique,"functionDeclaration":"function(value){return {value};}","arguments":[{"objectId":object}],"returnByValue":false,"awaitPromise":false,"objectGroup":key.1}), deadline).await;
            if let Some(owner) = self.evaluation.allocations.get_mut(&allocation) { owner.settled = settled; }
            let wrapped = wrapped?;
            self.frame_element_fence(frame, parent, world)?;
            if wrapped.get("exceptionDetails").is_some() { return Err(evaluation_error(&wrapped)); }
            let object = wrapped["result"]["objectId"].as_str().ok_or("BACKEND_PROTOCOL")?.to_owned();
            self.evaluation.groups.transfer_settled(&key)?;
            let reference = Reference { connection_id:frame.connection_id, id:Uuid::new_v4(), generation:Uuid::new_v4() };
            self.evaluation.handles.insert(reference, Handle { reference, world, scope:parent, session:session.clone(), object, owned_group:Some(key.1.clone()) });
            Ok(json!({"childFrame":frame,"frame":parent,"page":self.index.owning_page(parent).map_err(resource_error)?,"handle":reference,"potentiallyMutating":false,"relationshipCleanupPending":false}))
        }.await;
        if result.is_err() {
            self.release_execution_group(
                &key,
                deadline.min(Instant::now() + Duration::from_secs(2)),
            )
            .await;
        }
        let pending = self.evaluation.groups.contains(&key);
        let result = result.map_err(|mut error| {
            let cause = error.partial.take();
            error.partial = Some(json!({"childFrame":frame,"frame":parent,"relationshipCleanupPending":pending,"potentiallyMutating":false,"cause":cause}));
            for field in ["scopeClosed", "callerEffectsUnknown"] {
                if let Some(value)=cause.as_ref().and_then(|cause|cause.get(field)) {
                    error.partial.as_mut().expect("Frame element failure")[field]=value.clone();
                }
            }
            error
        });
        self.finish_handle_allocation(Some(allocation), parent, result)
    }

    fn frame_element_fence(
        &self,
        frame: Reference,
        parent: Reference,
        world: Reference,
    ) -> Result<(), Error> {
        let frame = if self.index.native(frame, Kind::FrameOwner).is_ok() {
            self.index
                .owner_frame(frame)
                .map_err(|_| Error::from("DOCUMENT_CHANGED"))?
        } else {
            frame
        };
        if self.index.parent_frame(frame).ok() != Some(parent)
            || self.index.route(world, Some(Kind::World)).is_err()
        {
            return Err("DOCUMENT_CHANGED".into());
        }
        Ok(())
    }
}
