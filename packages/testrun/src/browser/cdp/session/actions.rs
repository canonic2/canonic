//! Native locator actions with pre-dispatch state checks and no input replay.
use super::*;
use crate::browser::page::{Action, Locator};

impl Worker {
    pub(super) async fn page_action(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Request::PageAction {
            page,
            frame,
            locator,
            action,
            force,
            postcondition,
        } = request
        else {
            unreachable!()
        };
        let scope = self.page_scope(id, page, frame, deadline).await?;
        if self.inputs.held.values().any(|held| !held.empty()) {
            return Err("INPUT_HELD".into());
        }
        let group = format!("testrun-action-{id}");
        let mut ancestors = Vec::new();
        let guards = self
            .ancestor_guards(
                id,
                page,
                &scope,
                &action,
                force,
                &group,
                &mut ancestors,
                deadline,
            )
            .await;
        if let Err(error) = guards {
            for scope in &ancestors {
                let _ = self.release_action_group(scope, &group).await;
            }
            return Err(error);
        }
        let result = self
            .locator_action(id, page, &scope, &locator, &action, force, &group, deadline)
            .await;
        let cleanup = self.release_action_group(&scope, &group).await;
        let mut ancestor_cleanup = Ok(());
        for scope in &ancestors {
            if let Err(error) = self.release_action_group(scope, &group).await {
                ancestor_cleanup = Err(error);
            }
        }
        result?;
        cleanup.and(ancestor_cleanup).map_err(|mut error| {
            error.partial.get_or_insert_with(
                || json!({"page":page,"dispatchCompleted":true,"replayed":false}),
            );
            error
        })?;
        let observed = if let Some(condition) = postcondition {
            match self
                .page_wait(
                    id,
                    Request::PageWait {
                        page,
                        frame,
                        locator: Some(locator),
                        condition,
                    },
                    deadline,
                )
                .await
            {
                Ok(value) => Some(value),
                Err(mut error) => {
                    error.partial = Some(
                        json!({"dispatchCompleted":true,"postconditionSatisfied":false,"replayed":false}),
                    );
                    return Err(error);
                }
            }
        } else {
            None
        };
        let current = if frame.is_some() {
            self.index.page_frame_after_action(page, scope.frame).ok()
        } else {
            self.index.page_frame(page, None).ok()
        };
        Ok(
            json!({"page":page,"frame":current,"documentReplaced":current.map(|current|current!=scope.frame),"source":"chrome-cdp-action","mode":if matches!(action,Action::Dispatch{..}){"scripted"}else if force{"forced"}else{"normal"},"dispatchCompleted":true,"postcondition":observed}),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "One owned action transaction carries its exact scope and cleanup group"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The locator-action interpreter retains balanced native input across its typed action variants"
    )]
    async fn locator_action(
        &mut self,
        id: Uuid,
        page: Reference,
        scope: &page::Scope,
        locator: &Locator,
        action: &Action,
        force: bool,
        group: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let selector = locator.scoped_selector(scope.frame)?;
        let object = loop {
            self.observation_ready(id, deadline)?;
            let expression = page::query_expression(json!({"selector":selector,"read":"frame"}));
            let value=self.call_read(id,Some(&scope.session),"Runtime.evaluate",json!({"expression":expression,"uniqueContextId":scope.unique,"objectGroup":group,"returnByValue":false,"awaitPromise":true}),deadline).await?;
            if value.get("exceptionDetails").is_some() {
                let error = page::execution_error(&value);
                if error.code == "ELEMENT_NOT_FOUND" && !matches!(locator, Locator::Element { .. })
                {
                    self.observation_tick(deadline).await?;
                    continue;
                }
                return Err(error);
            }
            let object = value["result"]["objectId"]
                .as_str()
                .ok_or("ELEMENT_NOT_FOUND")?
                .to_owned();
            let pointer = matches!(
                action,
                Action::Click {}
                    | Action::DoubleClick {}
                    | Action::Hover {}
                    | Action::Check { .. }
                    | Action::Drag { .. }
            );
            if pointer {
                let scrolled = self
                    .call_read(
                        id,
                        Some(&scope.session),
                        "DOM.scrollIntoViewIfNeeded",
                        json!({"objectId":object}),
                        deadline,
                    )
                    .await;
                if let Err(error) = scrolled {
                    if error.code == "BROWSER_METHOD_FAILED" {
                        self.observation_tick(deadline).await?;
                        continue;
                    }
                    return Err(error);
                }
            }
            let prepared = self
                .action_eval(
                    id,
                    scope,
                    &object,
                    json!({"stage":"prepare","action":action,"force":force}),
                    deadline,
                    false,
                )
                .await?;
            if prepared["done"] == true {
                return Ok(());
            }
            if prepared["retry"] == true {
                self.call_read(
                    id,
                    Some(&scope.session),
                    "Runtime.releaseObject",
                    json!({"objectId":object}),
                    deadline,
                )
                .await?;
                self.observation_tick(deadline).await?;
                continue;
            }
            break object;
        };
        match action {
            Action::Click {} | Action::DoubleClick {} | Action::Hover {} | Action::Check { .. } => {
                let point = self
                    .action_point(id, page, scope, &object, deadline)
                    .await?;
                self.pointer_action(id, page, point, action, deadline)
                    .await?;
            }
            Action::Drag { target } => {
                let from = self
                    .action_point(id, page, scope, &object, deadline)
                    .await?;
                self.drag_action(id, page, scope, target, from, group, deadline)
                    .await?;
            }
            _ => {
                let result = self
                    .action_eval(
                        id,
                        scope,
                        &object,
                        json!({"stage":"commit","action":action}),
                        deadline,
                        true,
                    )
                    .await?;
                match action {
                    Action::Fill { value } if result["needsInput"] == true => {
                        self.insert_text(id, page, value, deadline).await?
                    }
                    Action::Clear {} if result["needsInput"] == true => {
                        self.press_key(id, page, "Delete", deadline).await?
                    }
                    Action::Type { text } => self.type_text(id, page, text, deadline).await?,
                    Action::Press { key } => self.press_key(id, page, key, deadline).await?,
                    _ => {}
                }
            }
        }
        if self.index.route(scope.world, Some(Kind::World)).is_err() {
            return Ok(());
        }
        self.action_eval(
            id,
            scope,
            &object,
            json!({"stage":"verify","action":action}),
            deadline,
            false,
        )
        .await
        .map_err(|mut error| {
            error.partial = Some(json!({"dispatchCompleted":true,"replayed":false}));
            error
        })?;
        Ok(())
    }

    pub(super) async fn action_eval(
        &mut self,
        id: Uuid,
        scope: &page::Scope,
        object: &str,
        options: Value,
        deadline: Instant,
        mutating: bool,
    ) -> Result<Value, Error> {
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        let function = format!(
            "function(options){{return ({}) ({},this,options);}}",
            include_str!("actions.js"),
            crate::browser::reading::selector("chromium")
        );
        let params = json!({"objectId":object,"functionDeclaration":function,"arguments":[{"value":options}],"returnByValue":true,"awaitPromise":true});
        let result = if mutating {
            self.call(
                id,
                Some(&scope.session),
                "Runtime.callFunctionOn",
                params,
                deadline,
            )
            .await?
        } else {
            self.call_read(
                id,
                Some(&scope.session),
                "Runtime.callFunctionOn",
                params,
                deadline,
            )
            .await?
        };
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        if result.get("exceptionDetails").is_some() {
            return Err(page::execution_error(&result));
        }
        page::query_result(result["result"]["value"].clone())
    }

    async fn release_action_group(
        &mut self,
        scope: &page::Scope,
        group: &str,
    ) -> Result<(), Error> {
        if self.index.route(scope.world, Some(Kind::World)).is_err() {
            return Ok(());
        }
        if let Some(connection) = &self.connection {
            let deadline = Instant::now() + Duration::from_secs(2);
            let stopped=connection.call(Some(&scope.session),"Runtime.evaluate",json!({"expression":format!("(()=>{{const injected={};const hit=injected.testrunInterceptor?.stop();injected.testrunInterceptor=null;return !hit||hit==='done';}})()",crate::browser::reading::selector("chromium")),"uniqueContextId":scope.unique,"returnByValue":true}),Lane::Ordinary,deadline).await;
            let released = connection
                .call(
                    Some(&scope.session),
                    "Runtime.releaseObjectGroup",
                    json!({"objectGroup":group}),
                    Lane::Control,
                    deadline,
                )
                .await;
            if stopped.is_err() || released.is_err() {
                self.drain()?;
                if self.index.route(scope.world, Some(Kind::World)).is_err() {
                    return Ok(());
                }
            }
            released.map_err(|_| Error::from("ACTION_CLEANUP_PENDING"))?;
            let stopped = stopped.map_err(|_| Error::from("ACTION_CLEANUP_PENDING"))?;
            if stopped["result"]["value"] == false {
                let mut error = Error::from("HIT_TARGET_CHANGED");
                error.partial = Some(json!({"dispatchCompleted":true,"replayed":false}));
                return Err(error);
            }
        }
        Ok(())
    }
}
