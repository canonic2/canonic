//! Dedicated native Chrome screenshots, temporary redaction and verified artifacts.
use super::*;
use crate::browser::page::{Scale, Screenshot};
use base64::Engine as _;

fn capture_params(
    capture: &Screenshot,
    before: &Value,
    prepared: &Value,
    metrics: &Value,
) -> Result<(Value, bool), Error> {
    let mut clip = if let Some(clip) = capture.clip {
        json!(clip)
    } else if capture.element.is_some() {
        prepared["clip"].clone()
    } else if capture.full_page {
        json!({"x":0,"y":0,"width":metrics["cssContentSize"]["width"],"height":metrics["cssContentSize"]["height"]})
    } else {
        json!({"x":before["scroll"]["x"],"y":before["scroll"]["y"],"width":before["viewport"]["width"],"height":before["viewport"]["height"]})
    };
    let viewport = capture.is_viewport();
    // Viewport CSS scaling happens after native capture, so admission must
    // bound the physical source pixels as well as the published image.
    let factor = if matches!(capture.scale, Scale::Css) && !viewport {
        1.0
    } else {
        before["dpr"].as_f64().ok_or("GEOMETRY_UNAVAILABLE")?
    };
    let area = clip["width"].as_f64().ok_or("GEOMETRY_UNAVAILABLE")?
        * clip["height"].as_f64().ok_or("GEOMETRY_UNAVAILABLE")?
        * factor
        * factor;
    if !area.is_finite() || area <= 0. || area > 32_000_000. {
        return Err("CAPTURE_BOUNDS".into());
    }
    clip["scale"] = json!(if matches!(capture.scale, Scale::Css) {
        1.0 / before["dpr"].as_f64().ok_or("GEOMETRY_UNAVAILABLE")?
    } else {
        1.0
    });
    let mut params =
        json!({"format":capture.format,"captureBeyondViewport":!viewport,"fromSurface":true});
    // Beyond-viewport capture and explicit viewport clips can temporarily
    // resize Chrome's surface and reset emulated DPR. Keep viewport capture
    // on the current surface so its input geometry stays unchanged.
    if !viewport {
        params["clip"] = clip;
    }
    if let Some(quality) = capture.quality {
        params["quality"] = json!(quality);
    }
    Ok((params, viewport))
}

impl Worker {
    pub(super) async fn screenshot(
        &mut self,
        id: Uuid,
        page: Reference,
        capture: Screenshot,
        deadline: Instant,
    ) -> Result<Value, Error> {
        capture.validate()?;
        let output = crate::browser::capture::output_path(&capture)?;
        let scope = self.page_scope(id, page, None, deadline).await?;
        let observed = std::time::Instant::now();
        let transaction = Uuid::new_v4();
        let options = json!({"id":transaction,"element":capture.element.as_ref().map(|locator|locator.scoped_selector(scope.frame)).transpose()?,"masks":capture.masks.iter().map(|locator|locator.scoped_selector(scope.frame)).collect::<Result<Vec<_>,_>>()?,"disableAnimations":capture.disable_animations});
        let prepared = self
            .page_expression(
                id,
                &scope,
                format!(
                    "({})({}, {})",
                    include_str!("capture.js"),
                    crate::browser::reading::selector("chromium"),
                    options
                ),
                deadline,
            )
            .await;
        let prepared = match prepared.and_then(page::query_result) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.cleanup_capture(&scope, transaction).await?;
                return Err(error);
            }
        };
        let before = match self.capture_geometry(id, &scope, deadline).await {
            Ok(before) => before,
            Err(error) => {
                self.cleanup_capture(&scope, transaction).await?;
                return Err(error);
            }
        };
        let started = crate::browser::capture::now_ms();
        let result = self
            .capture_transaction(id, &scope, &capture, &before, &prepared, deadline)
            .await;
        let cleanup = self.cleanup_capture(&scope, transaction).await;
        let (bytes, after) = result?;
        cleanup?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        if !capture.masks.is_empty()
            && ["url", "viewport", "dpr", "scroll", "document"]
                .iter()
                .any(|key| before[*key] != after[*key])
        {
            let mut error = Error::from("REDACTION_UNAVAILABLE");
            error.partial = Some(
                json!({"geometryBefore":before,"geometryAfter":after,"artifactPublished":false}),
            );
            return Err(error);
        }
        let byte_count = bytes.len();
        self.capture_store.reserve(byte_count)?;
        let result = crate::browser::capture::publish(
            self.journal.directory(),
            page,
            scope.frame,
            &capture,
            output,
            bytes,
            before,
            after.clone(),
            "chrome_cdp",
            "chrome-cdp-capture-screenshot",
            started,
        )
        .await;
        let mut result = result.inspect_err(|error| {
            self.capture_store
                .release_unpublished_screenshot(byte_count, error);
        })?;
        result["identity"] = json!(self.shared.state.lock().expect("Chrome identity").identity);
        if result["transform"].is_object() && after["pointerLocked"] != true {
            let observation = self.inputs.observe(page, scope.frame, after, observed, 0);
            result["inputObservation"] = json!(observation);
            result["coordinateInputAvailable"] = json!(true);
            result["inputCoordinateSpace"] = json!("css-viewport");
            result["inputObservationTtlMs"] = json!(10000);
        }
        self.capture_store.insert(&result)?;
        Ok(result)
    }
    async fn capture_transaction(
        &mut self,
        id: Uuid,
        scope: &page::Scope,
        capture: &Screenshot,
        before: &Value,
        prepared: &Value,
        deadline: Instant,
    ) -> Result<(Vec<u8>, Value), Error> {
        let metrics = self
            .call_read(
                id,
                Some(&scope.session),
                "Page.getLayoutMetrics",
                json!({}),
                deadline,
            )
            .await?;
        let (params, viewport) = capture_params(capture, before, prepared, &metrics)?;
        let session = scope.session.clone();
        if capture.transparent {
            self.call(
                id,
                Some(&session),
                "Emulation.setDefaultBackgroundColorOverride",
                json!({"color":{"r":0,"g":0,"b":0,"a":0}}),
                deadline,
            )
            .await?;
        }
        let captured = self
            .call(
                id,
                Some(&session),
                "Page.captureScreenshot",
                params,
                deadline,
            )
            .await;
        let background = if capture.transparent {
            self.cleanup_call(
                Some(&session),
                "Emulation.setDefaultBackgroundColorOverride",
                json!({}),
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .map(|_| ())
        } else {
            Ok(())
        };
        let captured = captured?;
        background?;
        let encoded = captured["data"].as_str().ok_or("INVALID_CAPTURE")?;
        if encoded.len() > 24 * 1024 * 1024 {
            return Err("CAPTURE_BYTE_LIMIT".into());
        }
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| Error::from("INVALID_CAPTURE"))?;
        if viewport && matches!(capture.scale, Scale::Css) {
            let width = before["viewport"]["width"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or("GEOMETRY_UNAVAILABLE")?;
            let height = before["viewport"]["height"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or("GEOMETRY_UNAVAILABLE")?;
            bytes = crate::browser::capture::css_viewport(
                bytes,
                capture.format,
                capture.quality,
                width,
                height,
                before["dpr"].as_f64().ok_or("GEOMETRY_UNAVAILABLE")?,
            )
            .await?;
        }
        let after = self.capture_geometry(id, scope, deadline).await?;
        Ok((bytes, after))
    }
    async fn cleanup_capture(&mut self, scope: &page::Scope, id: Uuid) -> Result<(), Error> {
        if self.index.route(scope.world, Some(Kind::World)).is_err() {
            return Ok(());
        }
        self.responsive_call(Some(&scope.session),"Runtime.evaluate",json!({"expression":format!("({})(null,{})",include_str!("capture.js"),json!({"id":id,"cleanup":true})),"uniqueContextId":scope.unique,"returnByValue":true}),Lane::Ordinary,Instant::now()+Duration::from_secs(2)).await.map(|_|()).map_err(|_|Error::from("CAPTURE_CLEANUP_PENDING"))
    }
    async fn capture_geometry(
        &mut self,
        id: Uuid,
        scope: &page::Scope,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.page_query(
            id,
            scope,
            json!({"selector":"css=body","read":"geometry","maxBytes":8192}),
            deadline,
        )
        .await
    }
}
