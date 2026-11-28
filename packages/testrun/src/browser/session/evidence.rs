//! Semantic capture, annotation replacement and immutable comparison for Playwright-owned pages.
use super::*;
impl Host {
    pub(super) async fn execute_evidence(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::PagePdf { .. } => Err("PDF_UNSUPPORTED".into()),
            Request::PageSnapshot {
                page,
                frame,
                locator,
            } => {
                let read = self
                    .page_read(
                        page,
                        frame,
                        locator,
                        crate::browser::page::Read::Aria,
                        8192,
                        deadline,
                    )
                    .await?;
                let bytes = (read["result"]["truncated"] != true).then(|| read.to_string().len());
                if let Some(bytes) = bytes {
                    self.capture_store
                        .lock()
                        .expect("Capture records")
                        .reserve(bytes)?;
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
                        self.capture_store
                            .lock()
                            .expect("Capture records")
                            .release_reservation(bytes);
                    }
                })?;
                self.capture_store
                    .lock()
                    .expect("Capture records")
                    .insert(&snapshot)?;
                Ok(snapshot)
            }
            Request::PageAnnotate { page, markers } => {
                let scope = self.page_scope(page, None, deadline).await?;
                let options =
                    crate::browser::evidence::annotation_options(scope.reference, markers)?;
                let engine = self.state.lock().expect("Browser owner").engine;
                let expression = format!(
                    "options=>({})({},options)",
                    crate::browser::evidence::ANNOTATE,
                    crate::browser::reading::selector(engine.name())
                );
                let result = self
                    .evaluate(&scope.wire, &expression, options, deadline)
                    .await?;
                if self.frame_current(&scope.wire, deadline).await? != scope.reference {
                    return Err("DOCUMENT_CHANGED".into());
                }
                Ok(
                    json!({"page":page,"frame":scope.reference,"result":result,"dispatchCompleted":true}),
                )
            }
            Request::CaptureCompare {
                baseline,
                actual,
                threshold,
                max_changed_ratio,
            } => {
                let (baseline, actual) = self
                    .capture_store
                    .lock()
                    .expect("Capture records")
                    .pair(baseline, actual)?;
                self.capture_store
                    .lock()
                    .expect("Capture records")
                    .reserve(16 * 1024 * 1024)?;
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
}
