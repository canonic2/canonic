//! Adopt actual main-world result nodes into the document's inspection world.
use super::execution_groups::{Key, Purpose};
use super::*;
use crate::browser::{developer_tools as tools, page::Locator};

impl Worker {
    pub(super) async fn developer_result_read(
        &mut self,
        id: Uuid,
        handle: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        self.fence_evaluation();
        self.fence_execution_groups();
        self.evaluation
            .groups
            .capacity(Purpose::DeveloperTransfer)?;
        let (frame, page) = self.evaluation_handle_document(handle)?;
        let evaluated = self
            .tool_evaluate(
                id,
                handle,
                tools::result_expression(),
                json!(frame),
                false,
                deadline,
            )
            .await?;
        let mut projected = tools::result_projection(evaluated, handle, page, frame)?;
        if projected["annotations"]
            .as_array()
            .is_some_and(Vec::is_empty)
        {
            return Ok(projected);
        }
        let result = self
            .transfer_result_elements(id, handle, page, frame, &mut projected, deadline)
            .await;
        self.release_execution_groups(deadline.min(Instant::now() + Duration::from_secs(2)))
            .await;
        let (key,inspection_world)=result.map_err(|mut error| {
            let cause=error.partial.take();
            error.partial=Some(json!({"handle":handle,"frame":frame,"dispatchCompleted":true,"potentiallyMutating":true,"originalToolReplayed":false,"projectionCompleted":false,"cause":cause}));
            error
        })?;
        let current = self
            .drain()
            .and_then(|()| self.evaluation_handle_document(handle))
            .and_then(|current| {
                if current != (frame, page) {
                    return Err("STALE_REFERENCE".into());
                }
                self.index
                    .route(inspection_world, Some(Kind::World))
                    .map_err(resource_error)?;
                Ok(())
            });
        current.map_err(|mut error:Error|{error.partial=Some(json!({"handle":handle,"frame":frame,"dispatchCompleted":true,"potentiallyMutating":true,"originalToolReplayed":false,"projectionCompleted":false}));error})?;
        projected["projectionCleanupPending"] = json!(self.evaluation.groups.contains(&key));
        crate::browser::schema::encoded_len(&projected, 65536)
            .map_err(|_| Error::from("DEVELOPER_TOOL_RESULT_BOUNDS"))?;
        Ok(projected)
    }
    async fn transfer_result_elements(
        &mut self,
        id: Uuid,
        handle: Reference,
        page: Reference,
        frame: Reference,
        projected: &mut Value,
        deadline: Instant,
    ) -> Result<(Key, Reference), Error> {
        let source = self.evaluation_handle_world(handle)?;
        let route = self
            .index
            .route(source, Some(Kind::World))
            .map_err(resource_error)?;
        let context = self
            .index
            .diagnostic_scope(&route.session_id)
            .map(|(context, _, _)| context)
            .ok_or("INVALID_SCOPE")?;
        let unique = route.execution_unique_id.ok_or("INVALID_SCOPE")?;
        let scope = self.page_scope(id, page, Some(frame), deadline).await?;
        if scope.session != route.session_id {
            return Err("INVALID_SCOPE".into());
        }
        self.result_worlds_live(source, &scope)?;
        let key = self.evaluation.groups.begin(
            context,
            scope.session.clone(),
            Purpose::DeveloperTransfer,
        )?;
        let count = projected["annotations"]
            .as_array()
            .ok_or("DEVELOPER_TOOL_PROTOCOL")?
            .len();
        for index in 0..count {
            let locator: Locator =
                serde_json::from_value(projected["annotations"][index]["locator"].clone())
                    .map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))?;
            let Locator::Element { id: element, .. } = locator else {
                return Err("DEVELOPER_TOOL_PROTOCOL".into());
            };
            let token = format!("{}.{}.{element}", frame.id, frame.generation);
            let node=self.execution_group_call(id,&key,"Runtime.callFunctionOn",json!({"functionDeclaration":"function(token){const value=globalThis[Symbol.for('canonic.testrun.elements.v1')]?.get(token)?.deref();if(!value)throw Error('canonic:STALE_REFERENCE');return value;}","uniqueContextId":unique,"arguments":[{"value":token}],"returnByValue":false,"awaitPromise":false,"objectGroup":key.1}),deadline).await?;
            self.result_worlds_live(source, &scope)?;
            if node.get("exceptionDetails").is_some() {
                return Err("STALE_REFERENCE".into());
            }
            let object = node["result"]["objectId"]
                .as_str()
                .ok_or("DEVELOPER_TOOL_ELEMENT_SCOPE")?;
            let described = self
                .execution_group_call(
                    id,
                    &key,
                    "DOM.describeNode",
                    json!({"objectId":object,"depth":0}),
                    deadline,
                )
                .await?;
            self.result_worlds_live(source, &scope)?;
            let resolved=self.execution_group_call(id,&key,"DOM.resolveNode",json!({"backendNodeId":described["node"]["backendNodeId"],"executionContextId":scope.numeric,"objectGroup":key.1}),deadline).await?;
            self.result_worlds_live(source, &scope)?;
            let object = resolved["object"]["objectId"]
                .as_str()
                .ok_or("DEVELOPER_TOOL_ELEMENT_SCOPE")?;
            let adopted = self
                .adopt_result_element(id, &key, object, &scope, deadline)
                .await?;
            self.result_worlds_live(source, &scope)?;
            projected["annotations"][index]["locator"] = adopted;
        }
        Ok((key, scope.world))
    }
    fn result_worlds_live(&self, source: Reference, scope: &page::Scope) -> Result<(), Error> {
        self.index
            .route(source, Some(Kind::World))
            .map_err(resource_error)?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        self.index
            .native(scope.frame, Kind::Frame)
            .map_err(resource_error)?;
        Ok(())
    }
    async fn adopt_result_element(
        &mut self,
        id: Uuid,
        key: &Key,
        object: &str,
        scope: &page::Scope,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let expression = format!(
            "function(frame){{if(!(this instanceof HTMLElement)||this.ownerDocument!==document||!this.isConnected)throw Error('canonic:DEVELOPER_TOOL_ELEMENT_SCOPE');return ({}).retain(this,frame);}}",
            crate::browser::reading::REFERENCES
        );
        let reply=self.execution_group_call(id,key,"Runtime.callFunctionOn",json!({"functionDeclaration":expression,"objectId":object,"arguments":[{"value":scope.frame}],"returnByValue":true,"awaitPromise":false,"objectGroup":key.1}),deadline).await?;
        if reply.get("exceptionDetails").is_some() {
            return Err("DEVELOPER_TOOL_ELEMENT_SCOPE".into());
        }
        let locator: Locator = serde_json::from_value(reply["result"]["value"].clone())
            .map_err(|_| Error::from("DEVELOPER_TOOL_PROTOCOL"))?;
        if !matches!(&locator,Locator::Element {frame,..} if *frame==scope.frame) {
            return Err("DEVELOPER_TOOL_PROTOCOL".into());
        }
        Ok(json!(locator))
    }
}
