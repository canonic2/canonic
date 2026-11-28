//! Document-scoped browser execution and bounded semantic reads.
use super::*;
use crate::browser::page::{Locator, Read};

pub(super) const WORLD: &str = "canonic.testrun.page.v1";
pub(super) const INITIALIZATION: &str = concat!(
    include_str!("../../session/page/native-timing.js"),
    "\n",
    include_str!("../../session/page/messages.js")
);

#[derive(Clone)]
pub(super) struct Scope {
    pub frame: Reference,
    pub world: Reference,
    pub session: String,
    pub unique: String,
    pub numeric: i64,
}

impl Worker {
    pub(super) async fn page_clipboard(
        &mut self,
        id: Uuid,
        page: Reference,
        origin: String,
        action: crate::browser::clipboard::Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(id, page, None, deadline).await?;
        let observed = self
            .page_expression(id, &scope, "location.origin".into(), deadline)
            .await?;
        if observed.as_str() != Some(&origin) {
            return Err("CLIPBOARD_ORIGIN_MISMATCH".into());
        }
        let context = self
            .index
            .list(Kind::Page)
            .into_iter()
            .find(|(reference, _)| *reference == page)
            .and_then(|(_, parent)| parent)
            .ok_or("INVALID_SCOPE")?;
        let context = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.call(id,None,"Browser.setPermission",json!({"permission":{"name":if matches!(action,crate::browser::clipboard::Action::Read{}){"clipboard-read"}else{"clipboard-write"},"allowWithoutSanitization":true},"setting":"granted","origin":origin,"browserContextId":context}),deadline).await?;
        let expression = format!(
            "({})({})",
            crate::browser::clipboard::SCRIPT,
            json!({"origin":origin,"action":action})
        );
        let result=self.call(id,Some(&scope.session),"Runtime.evaluate",json!({"expression":expression,"uniqueContextId":scope.unique,"returnByValue":true,"awaitPromise":true,"userGesture":true}),deadline).await?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        if result.get("exceptionDetails").is_some() {
            return Err(execution_error(&result));
        }
        let result = crate::browser::clipboard::result(result["result"]["value"].clone())?;
        Ok(json!({"page":page,"result":result,"replayed":false}))
    }
    /// The page's frame and an isolated world in its current document. A main
    /// frame replaced while the world is created (right after a `commit`-only
    /// navigation the document is still being swapped) is retried on the new
    /// frame; an explicit frame that goes stale stays stale.
    pub(super) async fn page_scope(
        &mut self,
        id: Uuid,
        page: Reference,
        frame: Option<Reference>,
        deadline: Instant,
    ) -> Result<Scope, Error> {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let before = self.index.page_frame(page, frame).ok();
            let result: Result<Scope, Error> = async {
                let scope = self.page_storage_scope(id, page, frame, deadline).await?;
                self.page_expression(id, &scope, INITIALIZATION.into(), deadline)
                    .await?;
                Ok(scope)
            }
            .await;
            match result {
                Err(error)
                    if error.code == "STALE_REFERENCE"
                        && frame.is_none()
                        && attempts < 4
                        && self
                            .index
                            .page_frame(page, None)
                            .is_ok_and(|current| Some(current) != before) => {}
                result => return result,
            }
        }
    }
    pub(super) async fn page_storage_scope(
        &mut self,
        id: Uuid,
        page: Reference,
        frame: Option<Reference>,
        deadline: Instant,
    ) -> Result<Scope, Error> {
        self.drain()?;
        self.reconcile(id, deadline).await?;
        let frame = self.index.page_frame(page, frame).map_err(resource_error)?;
        let route = self
            .index
            .route(frame, Some(Kind::Frame))
            .map_err(resource_error)?;
        let native = self
            .index
            .native(frame, Kind::Frame)
            .map_err(resource_error)?;
        let value = self
            .call(
                id,
                Some(&route.session_id),
                "Page.createIsolatedWorld",
                json!({"frameId":native,"worldName":WORLD,"grantUniveralAccess":false}),
                deadline,
            )
            .await;
        self.index
            .route(frame, Some(Kind::Frame))
            .map_err(resource_error)?;
        let value = value?;
        let numeric = value["executionContextId"]
            .as_i64()
            .ok_or("BACKEND_PROTOCOL")?;
        let world = self
            .index
            .world(frame, &route.session_id, numeric)
            .map_err(resource_error)?;
        let scope = Scope {
            numeric,
            frame,
            world,
            session: route.session_id,
            unique: self
                .index
                .route(world, Some(Kind::World))
                .map_err(resource_error)?
                .execution_unique_id
                .ok_or("INVALID_SCOPE")?,
        };
        Ok(scope)
    }

    pub(super) async fn page_expression(
        &mut self,
        id: Uuid,
        scope: &Scope,
        expression: String,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        let result = self.call_read(id, Some(&scope.session), "Runtime.evaluate",
            json!({"expression":expression,"uniqueContextId":scope.unique,"returnByValue":true,"awaitPromise":true}), deadline).await;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        let result = result?;
        if result.get("exceptionDetails").is_some() {
            return Err(execution_error(&result));
        }
        Ok(result["result"]["value"].clone())
    }

    pub(super) async fn page_query(
        &mut self,
        id: Uuid,
        scope: &Scope,
        options: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let expression = query_expression(options);
        let result = self
            .page_expression(id, scope, expression, deadline)
            .await?;
        query_result(result)
    }

    pub(super) async fn frame_enter(
        &mut self,
        id: Uuid,
        page: Reference,
        frame: Option<Reference>,
        locator: Locator,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(id, page, frame, deadline).await?;
        let group = format!("testrun-frame-{id}");
        let result = self
            .frame_element(id, page, &scope, &locator, &group, deadline)
            .await;
        let cleanup = self
            .call_read(
                id,
                Some(&scope.session),
                "Runtime.releaseObjectGroup",
                json!({"objectGroup":group}),
                deadline.max(Instant::now() + Duration::from_secs(1)),
            )
            .await;
        let result = result?;
        cleanup?;
        Ok(result)
    }

    async fn frame_element(
        &mut self,
        id: Uuid,
        page: Reference,
        scope: &Scope,
        locator: &Locator,
        group: &str,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let expression = query_expression(
            json!({"selector":locator.scoped_selector(scope.frame)?,"read":"frame"}),
        );
        let object=self.call_read(id,Some(&scope.session),"Runtime.evaluate",
            json!({"expression":expression,"uniqueContextId":scope.unique,"objectGroup":group,"returnByValue":false,"awaitPromise":true}),deadline).await?;
        if object.get("exceptionDetails").is_some() {
            return Err(execution_error(&object));
        }
        let object = object["result"]["objectId"].as_str().ok_or("NOT_A_FRAME")?;
        let node = self
            .call_read(
                id,
                Some(&scope.session),
                "DOM.describeNode",
                json!({"objectId":object,"depth":0}),
                deadline,
            )
            .await?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        let native = node["node"]["frameId"].as_str().ok_or("NOT_A_FRAME")?;
        let child = self
            .index
            .event_frame(&scope.session, native)
            .ok_or("FRAME_UNAVAILABLE")?;
        self.index
            .page_frame(page, Some(child))
            .map_err(resource_error)?;
        Ok(json!({"page":page,"frame":child}))
    }

    pub(super) async fn page_read(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Request::PageRead {
            page,
            frame,
            locator,
            read,
            max_bytes,
        } = request
        else {
            unreachable!()
        };
        let scope = self.page_scope(id, page, frame, deadline).await?;
        let selector = locator
            .as_ref()
            .map(|locator| locator.scoped_selector(scope.frame))
            .transpose()?
            .unwrap_or_else(|| "css=body".into());
        let result = self
            .page_query(
                id,
                &scope,
                json!({"selector":selector,"read":read,"maxBytes":max_bytes,"frame":scope.frame}),
                deadline,
            )
            .await?;
        let result = crate::browser::reading::with_narrowing_hint(result);
        let observation = if matches!(read, Read::Geometry) && frame.is_none() {
            Some(self.inputs.observe(
                page,
                scope.frame,
                result.clone(),
                std::time::Instant::now(),
                0,
            ))
        } else {
            None
        };
        Ok(
            json!({"page":page,"frame":scope.frame,"source":"chrome-cdp-isolated-world","read":read,
            "observedAtUnixMs":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),"result":result,
            "inputObservation":observation,"inputCoordinateSpace":"css-viewport","inputObservationTtlMs":10000}),
        )
    }
}

pub(super) fn query_expression(options: Value) -> String {
    format!(
        "({})({options})",
        crate::browser::reading::query_function("chromium")
    )
}

pub(super) fn query_result(result: Value) -> Result<Value, Error> {
    if let Some(code) = result["error"].as_str() {
        if code == "INVALID_DATE_VALUE" {
            let mut error = Error::from("INVALID_DATE_VALUE");
            error.partial = Some(
                json!({"hint":"Use a valid date in YYYY-MM-DD format, for example 2026-01-15, or clear the field."}),
            );
            return Err(error);
        }
        return Err(match code {
            "AMBIGUOUS_LOCATOR" => "AMBIGUOUS_LOCATOR",
            "ELEMENT_NOT_FOUND" => "ELEMENT_NOT_FOUND",
            "INVALID_SELECTOR" => "INVALID_SELECTOR",
            "STALE_REFERENCE" => "STALE_REFERENCE",
            "HIT_TARGET_CHANGED" => "HIT_TARGET_CHANGED",
            "CHECK_STATE_UNCHANGED" => "CHECK_STATE_UNCHANGED",
            "OPTION_NOT_FOUND" => "OPTION_NOT_FOUND",
            "NOT_FILE_INPUT" => "NOT_FILE_INPUT",
            "SINGLE_FILE_INPUT" => "SINGLE_FILE_INPUT",
            "FILE_ATTACH_FAILED" => "FILE_ATTACH_FAILED",
            "DIRECTORY_FIXTURE_REQUIRED" => "DIRECTORY_FIXTURE_REQUIRED",
            "DIRECTORY_INPUT_REQUIRED" => "DIRECTORY_INPUT_REQUIRED",
            "CAPTURE_BUSY" => "CAPTURE_BUSY",
            _ => "PAGE_EXECUTION_FAILED",
        }
        .into());
    }
    Ok(result)
}

pub(super) fn execution_error(result: &Value) -> Error {
    let message = result["exceptionDetails"]["exception"]["description"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default();
    match message {
        "Error: canonic:AMBIGUOUS_LOCATOR" => "AMBIGUOUS_LOCATOR",
        "Error: canonic:ELEMENT_NOT_FOUND" => "ELEMENT_NOT_FOUND",
        "Error: canonic:INVALID_SELECTOR" => "INVALID_SELECTOR",
        "Error: canonic:STALE_REFERENCE" => "STALE_REFERENCE",
        "Error: canonic:REDACTION_UNAVAILABLE" => "REDACTION_UNAVAILABLE",
        "Error: canonic:CAPTURE_BOUNDS" => "CAPTURE_BOUNDS",
        "Error: canonic:ELEMENT_NOT_VISIBLE" => "ELEMENT_NOT_VISIBLE",
        _ => "PAGE_EXECUTION_FAILED",
    }
    .into()
}

pub(super) fn validate_read(
    locator: &Option<Locator>,
    read: &Read,
    max_bytes: usize,
) -> Result<(), Error> {
    if !(256..=8192).contains(&max_bytes) {
        return Err("READ_BOUNDS".into());
    }
    if let Some(locator) = locator {
        locator.selector()?;
    }
    let _ = read;
    Ok(())
}
