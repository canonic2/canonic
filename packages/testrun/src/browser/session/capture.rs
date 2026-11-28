//! Private dedicated screenshots with decoded bounds and observed geometry.
use super::*;
use crate::browser::{
    lifecycle::Resource,
    page::{Format, Scale, Screenshot},
};
use base64::Engine as _;

impl Host {
    pub(super) async fn screenshot(
        &self,
        page: Reference,
        capture: Screenshot,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let observation_started = std::time::Instant::now();
        let output = crate::browser::capture::output_path(&capture)?;
        let input_epoch = self.input_epoch.load(Ordering::SeqCst);
        let scope = self.page_scope(page, None, deadline).await?;
        let before = self
            .evaluate(&scope.wire, super::page::GEOMETRY, Value::Null, deadline)
            .await?;
        let (params, factor) = self
            .capture_parameters(&scope.wire, scope.reference, &capture, &before, deadline)
            .await?;
        let started = super::page::now_ms();
        let (reply, source) = self
            .screenshot_reply(page, &scope, &capture, params, deadline)
            .await?;
        let completed = super::page::now_ms();
        let bytes = Self::capture_bytes(reply.ok_or("MISSING_RESULT")?)?;
        let after = self
            .evaluate(&scope.wire, super::page::GEOMETRY, Value::Null, deadline)
            .await?;
        let current = self.frame_current(&scope.wire, deadline).await?;
        if current != scope.reference {
            return Err("DOCUMENT_CHANGED".into());
        }
        let stable = before == after;
        // Masks are resolved by the driver during capture, never projected
        // from our preflight geometry. Firefox can settle a fractional visual
        // viewport offset while its layout/scroll/DPR stay unchanged. Retain
        // both measurements and disable our transform in that case.
        let layout_stable = ["url", "document", "viewport", "dpr", "scroll"]
            .iter()
            .all(|key| before[*key] == after[*key]);
        if !layout_stable && !capture.masks.is_empty() {
            return Err(Error::caused(
                "REDACTION_UNAVAILABLE",
                std::io::Error::other(format!(
                    "Capture geometry changed: before={before}; after={after}"
                )),
            ));
        }
        let directory = self.journal.directory().to_path_buf();
        let identity = self.state.lock().expect("Browser owner").identity.clone();
        let scale = if matches!(capture.scale, Scale::Css) {
            1.0
        } else {
            factor
        };
        let byte_count = bytes.len();
        self.capture_store
            .lock()
            .expect("Capture records")
            .reserve(byte_count)?;
        let result=tokio::task::spawn_blocking(move||{
            let format=match capture.format{Format::Png=>image::ImageFormat::Png,Format::Jpeg=>image::ImageFormat::Jpeg};
            let reader=image::ImageReader::with_format(std::io::Cursor::new(&bytes),format);
            let (width,height)=reader.into_dimensions().map_err(|error|Error::caused("INVALID_CAPTURE",error))?;
            if u64::from(width)*u64::from(height)>32_000_000{return Err(Error::from("CAPTURE_BOUNDS"));}
            image::load_from_memory_with_format(&bytes,format).map_err(|error|Error::caused("INVALID_CAPTURE",error))?;
            let (capture_id,attachment)=crate::browser::capture::write_screenshot(&directory,output,capture.format,&bytes)?;
            let mapped=stable&&capture.is_viewport()
                &&before["viewport"]["width"].as_f64().unwrap_or(0.0)*scale==f64::from(width)
                &&before["viewport"]["height"].as_f64().unwrap_or(0.0)*scale==f64::from(height);
            Ok(json!({"captureId":capture_id,"page":page,"frame":current,"backend":"playwright","identity":identity,
                "attachment":attachment,
                "width":width,"height":height,"geometryBefore":before,"geometryAfter":after,"geometryStable":stable,
                "transform":if mapped{Some(json!({"cssToPixels":scale,"documentOrigin":before["scroll"],"padding":{"right":0,"bottom":0}}))}else{None},
                "coordinateInputAvailable":false,"source":source,"options":capture,"privacy":"local-private","complete":true,
                "timing":{"requestUnixMs":started,"replyUnixMs":completed,"sourceTimestamp":null,"acquisitionWithinRequestReply":true}}))
        }).await.map_err(|_|Error::from("CAPTURE_IO")).flatten();
        let mut result = result.inspect_err(|error| {
            self.capture_store
                .lock()
                .expect("Capture records")
                .release_unpublished_screenshot(byte_count, error);
        })?;
        self.attach_input_observation(&mut result, page, current, observation_started, input_epoch);
        self.capture_store
            .lock()
            .expect("Capture records")
            .insert(&result)?;
        Ok(result)
    }

    async fn screenshot_reply(
        &self,
        page: Reference,
        scope: &Resource,
        capture: &Screenshot,
        mut params: Value,
        deadline: Instant,
    ) -> Result<(Option<Value>, &'static str), Error> {
        let connection = self.connection()?;
        if let Some(locator) = &capture.element {
            let element = self.element(scope, locator, deadline).await?;
            let reply = connection
                .retained(&element, "screenshot", params, deadline, false)
                .await;
            let cleanup = self.dispose_element(&element).await;
            if cleanup.is_err() {
                self.set_failure("HANDLE_CLEANUP_PENDING");
            }
            cleanup?;
            return Ok((reply?, "playwright-element-screenshot"));
        }

        params["fullPage"] = json!(capture.full_page);
        if let Some(clip) = capture.clip {
            params["clip"] = json!(clip);
        }
        let reply = connection
            .retained(
                &self.wire(page, Kind::Page)?,
                "screenshot",
                params,
                deadline,
                false,
            )
            .await?;
        Ok((reply, "playwright-page-screenshot"))
    }

    fn attach_input_observation(
        &self,
        result: &mut Value,
        page: Reference,
        current: Reference,
        observation_started: std::time::Instant,
        input_epoch: u64,
    ) {
        let geometry = &result["geometryAfter"];
        if !result["transform"].is_null()
            && geometry["pointerLocked"] != true
            && geometry["visualViewport"]["scale"]
                .as_f64()
                .is_none_or(|scale| (scale - 1.0).abs() < 0.000001)
        {
            let reference = self.inputs.lock().expect("Browser input").observe(
                page,
                current,
                geometry.clone(),
                observation_started,
                input_epoch,
            );
            result["inputObservation"] = json!(reference);
            result["coordinateInputAvailable"] = json!(true);
            result["inputCoordinateSpace"] = json!("css-viewport");
            result["inputObservationTtlMs"] = json!(10000);
        }
    }

    async fn capture_parameters(
        &self,
        frame: &crate::browser::objects::ObjectRef,
        reference: Reference,
        capture: &Screenshot,
        before: &Value,
        deadline: Instant,
    ) -> Result<(Value, f64), Error> {
        let mut params = json!({"type":capture.format,"scale":capture.scale,"caret":"hide","animations":if capture.disable_animations{"disabled"}else{"allow"},"omitBackground":capture.transparent});
        if let Some(quality) = capture.quality {
            params["quality"] = json!(quality);
        }
        let mut masks = Vec::new();
        for locator in &capture.masks {
            let count = self
                .connection()?
                .retained(
                    frame,
                    "queryCount",
                    json!({"selector":locator.scoped_selector(reference)?}),
                    deadline,
                    false,
                )
                .await?
                .ok_or("MISSING_RESULT")?;
            if count["value"].as_u64().unwrap_or(0) == 0 {
                return Err("REDACTION_UNAVAILABLE".into());
            }
            masks.push(json!({"frame":{"guid":frame.guid()},"selector":locator.scoped_selector(reference)?}));
        }
        params["mask"] = json!(masks);
        let factor = if matches!(capture.scale, Scale::Device) {
            before["dpr"].as_f64().ok_or("GEOMETRY_UNAVAILABLE")?
        } else {
            1.0
        };
        let area = if let Some(clip) = capture.clip {
            clip.width * clip.height
        } else if capture.full_page {
            before["document"]["width"]
                .as_f64()
                .ok_or("GEOMETRY_UNAVAILABLE")?
                * before["document"]["height"]
                    .as_f64()
                    .ok_or("GEOMETRY_UNAVAILABLE")?
        } else {
            before["viewport"]["width"]
                .as_f64()
                .ok_or("GEOMETRY_UNAVAILABLE")?
                * before["viewport"]["height"]
                    .as_f64()
                    .ok_or("GEOMETRY_UNAVAILABLE")?
        };
        if !area.is_finite() || area * factor * factor > 32_000_000.0 {
            return Err("CAPTURE_BOUNDS".into());
        }
        Ok((params, factor))
    }

    fn capture_bytes(reply: Value) -> Result<Vec<u8>, Error> {
        let encoded = reply["binary"].as_str().ok_or("MISSING_RESULT")?;
        if encoded.len() > 24 * 1024 * 1024 {
            return Err("CAPTURE_BYTE_LIMIT".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| Error::from("INVALID_CAPTURE"))?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("CAPTURE_BYTE_LIMIT".into());
        }
        Ok(bytes)
    }
}
