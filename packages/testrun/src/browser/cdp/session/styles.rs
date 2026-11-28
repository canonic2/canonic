//! Computed CSS and native cascade inspection within an exact owned document.
use super::*;
impl Worker {
    pub(super) async fn page_css(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Request::PageCss { developer, .. } = &request else {
            unreachable!()
        };
        let developer = *developer;
        if !self.developers.css_busy.insert(developer) {
            return Err("DEVELOPER_BUSY".into());
        }
        let result = self.inspect_css(id, request, deadline).await;
        self.developers.css_busy.remove(&developer);
        result
    }
    async fn inspect_css(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Request::PageCss {
            developer,
            page,
            frame,
            locator,
            offset,
            limit,
        } = request
        else {
            unreachable!()
        };
        if self.developer_status(developer)?["page"] != json!(page) {
            return Err("INVALID_SCOPE".into());
        }
        let session = self.developer_session(developer)?;
        let scope = self.page_scope(id, page, frame, deadline).await?;
        let target = self
            .index
            .targets()
            .into_iter()
            .find(|(_, _, _, session)| session.as_deref() == Some(&scope.session))
            .map(|(_, _, target, _)| target)
            .ok_or("CSS_TARGET_UNAVAILABLE")?;
        let group = format!("testrun:css:{id}");
        self.execution_cleanup_capacity()?;
        let expression = super::page::query_expression(
            json!({"selector":locator.scoped_selector(scope.frame)?,"read":"frame"}),
        );
        let node:Result<i64,Error>=async{
            let object=self.call_read(id,Some(&scope.session),"Runtime.evaluate",json!({"expression":expression,"uniqueContextId":scope.unique,"objectGroup":group,"returnByValue":false,"awaitPromise":true}),deadline).await?;
            if object.get("exceptionDetails").is_some(){return Err(super::page::execution_error(&object));}
            let object=object["result"]["objectId"].as_str().ok_or("CSS_NODE_UNAVAILABLE")?;
            let result=self.call_read(id,Some(&scope.session),"DOM.describeNode",json!({"objectId":object,"depth":0}),deadline).await?;
            result["node"]["backendNodeId"].as_i64().ok_or("CSS_NODE_UNAVAILABLE".into())
        }.await;
        self.queue_execution_group(scope.session.clone(), group);
        self.release_evaluation().await;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        let node = node?;
        let expression = format!(
            "({})({},{},{},{})",
            include_str!("../../reading/css.js"),
            json!(target),
            node,
            offset,
            limit
        );
        let reply = self
            .call_read(
                id,
                Some(&session),
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
                deadline,
            )
            .await?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        if self.developer_session(developer).ok().as_ref() != Some(&session) {
            return Err("DEVELOPER_SESSION_LOST".into());
        }
        if reply.get("exceptionDetails").is_some() {
            let description = reply["exceptionDetails"]["exception"]["description"]
                .as_str()
                .unwrap_or("Native CSS model failed");
            let code = [
                "CSS_TARGET_UNAVAILABLE",
                "CSS_MODEL_UNAVAILABLE",
                "CSS_NODE_UNAVAILABLE",
                "CSS_RESULT_BOUNDS",
            ]
            .into_iter()
            .find(|code| description.starts_with(&format!("Error: canonic:{code}")))
            .unwrap_or("CSS_MODEL_FAILED");
            let mut error = Error::from(code);
            error.partial = Some(
                json!({"developer":developer,"page":page,"exception":crate::browser::diagnostics::text(description).0}),
            );
            return Err(error);
        }
        let result = reply["result"]["value"].clone();
        if !result["items"].is_array() || result.to_string().len() > 65536 {
            return Err("CSS_RESULT_BOUNDS".into());
        }
        Ok(json!({"developer":developer,"page":page,"frame":scope.frame,"result":result}))
    }
    pub(super) async fn page_styles(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Request::PageStyles {
            page,
            frame,
            locator,
            properties,
            max_bytes,
        } = request
        else {
            unreachable!()
        };
        let scope = self.page_scope(id, page, frame, deadline).await?;
        let result=self.page_query(id,&scope,json!({"selector":locator.scoped_selector(scope.frame)?,"read":"styles","properties":properties,"maxBytes":max_bytes}),deadline).await?;
        Ok(
            json!({"page":page,"frame":scope.frame,"read":"styles","source":"chrome-cdp-isolated-world","result":result}),
        )
    }
}
