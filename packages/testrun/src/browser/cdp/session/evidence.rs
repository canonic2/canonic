//! Native semantic evidence, owned annotations and bounded streamed PDF export.
use super::*;
use crate::browser::{
    evidence::{Marker, PdfOptions},
    page::{Locator, Read},
};
use base64::Engine as _;
impl Worker {
    pub(super) async fn execute_evidence(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::PageScreenshot { page, capture } => {
                self.screenshot(id, page, capture, deadline).await
            }
            Request::PageSnapshot {
                page,
                frame,
                locator,
            } => {
                self.native_snapshot(id, page, frame, locator, deadline)
                    .await
            }
            Request::PageAnnotate { page, markers } => {
                self.native_annotate(id, page, markers, deadline).await
            }
            Request::PagePdf { page, options } => {
                self.native_pdf(id, page, options, deadline).await
            }
            Request::PreviewStart { page, options } => {
                self.preview_start(id, page, options, true, deadline).await
            }
            Request::PreviewStatus { preview } => self.preview_status(preview),
            Request::PreviewStop { preview } => self.preview_stop(preview, deadline).await,
            Request::RecordingStart { page, options } => {
                self.native_recording_start(id, page, options, deadline)
                    .await
            }
            Request::RecordingStatus { recording } => {
                self.native_recording_result(recording, false, deadline)
                    .await
            }
            Request::RecordingStop { recording } => {
                self.native_recording_result(recording, true, deadline)
                    .await
            }
            Request::CaptureCompare {
                baseline,
                actual,
                threshold,
                max_changed_ratio,
            } => {
                let (baseline, actual) = self.capture_store.pair(baseline, actual)?;
                self.capture_store.reserve(16 * 1024 * 1024)?;
                crate::browser::capture::compare(
                    self.journal.directory().into(),
                    baseline,
                    actual,
                    threshold,
                    max_changed_ratio,
                )
                .await
            }
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
    pub(super) async fn native_snapshot(
        &mut self,
        id: Uuid,
        page: Reference,
        frame: Option<Reference>,
        locator: Option<Locator>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let read = self
            .page_read(
                id,
                Request::PageRead {
                    page,
                    frame,
                    locator,
                    read: Read::Aria,
                    max_bytes: 8192,
                },
                deadline,
            )
            .await?;
        let bytes = (read["result"]["truncated"] != true).then(|| read.to_string().len());
        if let Some(bytes) = bytes {
            self.capture_store.reserve(bytes)?;
        }
        let snapshot = crate::browser::evidence::snapshot(
            self.journal.directory().into(),
            page,
            read["frame"].clone(),
            read["result"].clone(),
        )
        .await
        .inspect_err(|_| {
            if let Some(bytes) = bytes {
                self.capture_store.release_reservation(bytes);
            }
        })?;
        self.capture_store.insert(&snapshot)?;
        Ok(snapshot)
    }
    pub(super) async fn native_annotate(
        &mut self,
        id: Uuid,
        page: Reference,
        markers: Vec<Marker>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(id, page, None, deadline).await?;
        let options = crate::browser::evidence::annotation_options(scope.frame, markers)?;
        let expression = format!(
            "({})({},{})",
            crate::browser::evidence::ANNOTATE,
            crate::browser::reading::selector("chromium"),
            options
        );
        let result=self.call(id,Some(&scope.session),"Runtime.evaluate",json!({"expression":expression,"uniqueContextId":scope.unique,"returnByValue":true}),deadline).await?;
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        if result.get("exceptionDetails").is_some() {
            return Err(page::execution_error(&result));
        }
        Ok(
            json!({"page":page,"frame":scope.frame,"result":result["result"]["value"],"dispatchCompleted":true}),
        )
    }
    pub(super) async fn native_pdf(
        &mut self,
        id: Uuid,
        page: Reference,
        options: PdfOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        if self
            .shared
            .state
            .lock()
            .expect("Chrome state")
            .identity
            .as_ref()
            .is_none_or(|identity| identity["mode"] != "headless")
        {
            return Err("PDF_UNSUPPORTED_MODE".into());
        }
        let scope = self.page_scope(id, page, None, deadline).await?;
        let reply = self
            .call(
                id,
                Some(&scope.session),
                "Page.printToPDF",
                options.params(),
                deadline,
            )
            .await?;
        let handle = reply["stream"].as_str().ok_or("PDF_EXPORT_FAILED")?;
        let data = self.pdf_bytes(id, &scope.session, handle, deadline).await;
        let closed = self
            .cleanup_call(
                Some(&scope.session),
                "IO.close",
                json!({"handle":handle}),
                Instant::now() + Duration::from_secs(2),
            )
            .await;
        let bytes = data?;
        closed?;
        self.capture_store.reserve(bytes.len())?;
        if !bytes.starts_with(b"%PDF-") || !bytes.windows(5).any(|window| window == b"%%EOF") {
            return Err("INVALID_PDF".into());
        }
        self.index
            .route(scope.world, Some(Kind::World))
            .map_err(resource_error)?;
        let path = self
            .journal
            .directory()
            .join(format!("{}.pdf", Uuid::new_v4()));
        crate::artifacts::write(&path, &bytes)
            .map_err(|error| Error::caused("CAPTURE_IO", error))?;
        Ok(
            json!({"page":page,"frame":scope.frame,"backend":"chrome_cdp","source":"chrome-cdp-print-to-pdf","attachment":{"path":path,"mimeType":"application/pdf","bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))},"options":options,"complete":true,"privacy":"local-private"}),
        )
    }
    async fn pdf_bytes(
        &mut self,
        id: Uuid,
        session: &str,
        handle: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        loop {
            let chunk = self
                .call_read(
                    id,
                    Some(session),
                    "IO.read",
                    json!({"handle":handle,"size":65536}),
                    deadline,
                )
                .await?;
            let text = chunk["data"].as_str().ok_or("BACKEND_PROTOCOL")?;
            let data = if chunk["base64Encoded"] == true {
                base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .map_err(|_| Error::from("INVALID_PDF"))?
            } else {
                text.as_bytes().to_vec()
            };
            if bytes.len() + data.len() > 16 * 1024 * 1024 {
                return Err("CAPTURE_BYTE_LIMIT".into());
            }
            bytes.extend(data);
            if chunk["eof"] == true {
                return Ok(bytes);
            }
        }
    }
}
