//! Read-only ancestor-frame visibility for scoped DOM conditions.
use super::*;

impl Worker {
    pub(super) async fn frame_visible(
        &mut self,
        id: Uuid,
        page: Reference,
        frame: Reference,
        deadline: Instant,
    ) -> Result<bool, Error> {
        let mut child = frame;
        for depth in 0..32 {
            let parent = self
                .index
                .list(Kind::Frame)
                .into_iter()
                .find(|(frame, _)| *frame == child)
                .and_then(|(_, parent)| parent)
                .ok_or("INVALID_SCOPE")?;
            if self.index.route(parent, Some(Kind::Page)).is_ok() {
                return Ok(true);
            }
            let scope = self.page_scope(id, page, Some(parent), deadline).await?;
            let group = format!("testrun-visible-{id}-{depth}");
            let result=async {
                let owner=self.call_read(id,Some(&scope.session),"DOM.getFrameOwner",json!({"frameId":self.index.native(child,Kind::Frame).map_err(resource_error)?}),deadline).await?;
                let object=self.call_read(id,Some(&scope.session),"DOM.resolveNode",json!({"backendNodeId":owner["backendNodeId"],"executionContextId":scope.numeric,"objectGroup":group}),deadline).await?;
                let function=format!("function(){{const injected={};return injected.elementState(this,'visible',true).matches;}}",crate::browser::reading::selector("chromium"));
                let value=self.call_read(id,Some(&scope.session),"Runtime.callFunctionOn",json!({"objectId":object["object"]["objectId"],"functionDeclaration":function,"returnByValue":true}),deadline).await?;
                self.index.route(scope.world,Some(Kind::World)).map_err(resource_error)?;
                if value.get("exceptionDetails").is_some(){return Err(page::execution_error(&value));}
                value["result"]["value"].as_bool().ok_or_else(||Error::from("BACKEND_PROTOCOL"))
            }.await;
            let cleanup = self
                .connection
                .as_ref()
                .ok_or("BACKEND_LOST")?
                .call(
                    Some(&scope.session),
                    "Runtime.releaseObjectGroup",
                    json!({"objectGroup":group}),
                    Lane::Control,
                    Instant::now() + Duration::from_secs(2),
                )
                .await
                .map_err(protocol_error);
            let visible = result?;
            cleanup?;
            if !visible {
                return Ok(false);
            }
            child = parent;
        }
        Err("FRAME_DEPTH_BOUNDS".into())
    }
}
