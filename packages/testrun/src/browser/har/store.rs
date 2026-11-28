//! Session-owned immutable replay archives and retryable local publication.
use super::capture::Capture;
use super::*;
use crate::browser::contract::Reference;
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;
pub(crate) struct Reply {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body_base64: String,
    pub url: String,
    pub redirects: usize,
}
#[derive(Default)]
pub(crate) struct Store {
    items: BTreeMap<Uuid, Archive>,
    pub(super) captures: BTreeMap<Reference, Capture>,
    evidence: super::super::artifacts::Store,
}
impl Store {
    /// Whether the archive selects an entry, without preparing its reply body.
    pub(crate) fn matches(&self, id: Uuid, request: &Request) -> Result<bool, Error> {
        Ok(self.get(id)?.find(request)?.is_some())
    }
    pub(crate) fn reply(&self, id: Uuid, request: &Request) -> Result<Option<Reply>, Error> {
        self.get(id)?
            .find(request)?
            .map(|found| {
                Ok(Reply {
                    status: found.response.status,
                    // HAR content is the decoded entity, not the recorded
                    // transfer representation. Keep HEAD/304 metadata only.
                    headers: found
                        .response
                        .headers
                        .iter()
                        .filter(|header| {
                            request.method == "HEAD"
                                || found.response.status == 304
                                || !matches!(
                                    header.name.to_ascii_lowercase().as_str(),
                                    "content-length" | "content-encoding"
                                )
                        })
                        .cloned()
                        .collect(),
                    body_base64: STANDARD
                        .encode(found.response.body.as_ref().ok_or("HAR_BODY_UNAVAILABLE")?),
                    url: found.url.into(),
                    redirects: found.redirects,
                })
            })
            .transpose()
    }
    pub(crate) fn register(&mut self, path: &Path, directory: &Path) -> Result<Value, Error> {
        if self.items.len() + self.captures.len() >= 4 {
            return Err("HAR_ARCHIVE_CAPACITY".into());
        }
        let archive = Archive::open(path)?;
        let bytes = archive.encode()?;
        let id = Uuid::new_v4();
        self.evidence.reserve(id, 1, bytes.len())?;
        let format = super::super::artifacts::Format {
            prefix: "har",
            extension: "har",
            mime: "application/json",
            name: "har-1.2-replay-snapshot",
            max_bytes: MAX_INPUT,
        };
        if let Err(error)=self.evidence.stage(id,format,bytes,json!({"archiveId":id,"privacy":"private-secret-bearing","entryCount":archive.entries.len(),"normalizedForReplay":true,"sourceMetadataPreserved":true})){
            self.evidence.release(id);return Err(error);
        }
        self.items.insert(id, archive);
        self.finalize(directory, id).map_err(|mut error| {
            error.partial =
                Some(json!({"archiveId":id,"registered":true,"publicationPending":true}));
            error
        })
    }
    pub(crate) fn get(&self, id: Uuid) -> Result<&Archive, Error> {
        self.items.get(&id).ok_or("UNKNOWN_HAR_ARCHIVE".into())
    }
    fn reports(&self, id: Uuid) -> Vec<Value> {
        self.evidence
            .list()
            .as_array()
            .expect("Evidence list")
            .iter()
            .filter(|item| item["archiveId"] == json!(id))
            .cloned()
            .collect()
    }
    fn pending(&self, id: Uuid) -> bool {
        self.evidence
            .pending_summary()
            .as_array()
            .expect("Pending evidence")
            .iter()
            .any(|item| item["captureId"] == json!(id))
    }
    pub(crate) fn finalize(&mut self, directory: &Path, id: Uuid) -> Result<Value, Error> {
        self.get(id)?;
        if self.pending(id) {
            self.evidence.finalize(directory, id)?;
        }
        let reports = self.reports(id);
        let replaces = reports
            .iter()
            .find_map(|report| report.get("replacesArchiveId").filter(|id| !id.is_null()))
            .cloned();
        let mut result =
            json!({"archiveId":id,"registered":true,"publicationPending":false,"reports":reports});
        if let Some(replaces) = replaces {
            result["replacesArchiveId"] = replaces;
        }
        Ok(result)
    }
    pub(crate) fn list(&self) -> Value {
        json!({"archives":self.items.iter().map(|(id,archive)|json!({"archiveId":id,"entries":archive.entries.len(),"publicationPending":self.pending(*id),"reports":self.reports(*id)})).collect::<Vec<_>>(),"captures":self.captures.values().map(Capture::summary).collect::<Vec<_>>(),"maxArchives":4})
    }
    pub(crate) fn remove(&mut self, id: Uuid) -> Result<Value, Error> {
        self.get(id)?;
        if self
            .captures
            .values()
            .any(|capture| capture.replaces == Some(id))
        {
            return Err("HAR_ARCHIVE_IN_USE".into());
        }
        if self.pending(id) {
            return Err("HAR_PUBLICATION_PENDING".into());
        }
        for report in self.reports(id) {
            let evidence_id = serde_json::from_value(report["evidenceId"].clone())
                .map_err(|_| Error::from("HAR_EVIDENCE_PROTOCOL"))?;
            self.evidence.get(evidence_id)?.remove()?;
            self.evidence.forget(evidence_id);
        }
        self.items.remove(&id);
        Ok(json!({"archiveId":id,"removed":true}))
    }
    pub(crate) fn finalize_all(&mut self, directory: &Path) -> Result<(), Error> {
        let contexts = self.captures.keys().copied().collect::<Vec<_>>();
        for context in contexts {
            self.captures
                .get_mut(&context)
                .expect("Capture owner")
                .stopping = true;
            self.capture_publish(directory, context)?;
        }
        self.evidence.finalize_all(directory)
    }
    pub(crate) fn capture_start(
        &mut self,
        context: Reference,
        page: Option<Reference>,
        (content, mode, export): (CaptureContent, CaptureMode, Option<Export>),
        replaces: Option<Uuid>,
        filter: Option<super::super::routing::Matcher>,
    ) -> Result<Value, Error> {
        if self.captures.contains_key(&context) {
            return Err("HAR_CAPTURE_ACTIVE".into());
        }
        if self.items.len() + self.captures.len() >= 4 {
            return Err("HAR_ARCHIVE_CAPACITY".into());
        }
        if let Some(id) = replaces {
            self.get(id)?;
        }
        let mut capture = Capture::new(context, content);
        capture.page = page;
        capture.mode = mode;
        capture.export = export;
        capture.filter = filter.as_ref().map(|filter| filter.compile()).transpose()?;
        capture.replaces = replaces;
        self.evidence.reserve(capture.id, 1, MAX_INPUT)?;
        let summary = capture.summary();
        self.captures.insert(context, capture);
        Ok(summary)
    }
    pub(crate) fn capturing(&self, context: Reference) -> bool {
        self.captures
            .get(&context)
            .is_some_and(|capture| !capture.stopping)
    }
    pub(crate) fn capture_bodies(&self, context: Reference) -> bool {
        self.captures
            .get(&context)
            .is_some_and(|capture| !capture.stopping && capture.content != CaptureContent::Omit)
    }
    /// `page` is the request frame's owned page; pass `None` for worker or unattributed requests.
    pub(crate) fn capture_accepts(
        &self,
        context: Reference,
        page: Option<Reference>,
        url: &str,
    ) -> bool {
        self.captures
            .get(&context)
            .is_some_and(|capture| capture.accepts(url, page))
    }
    pub(crate) fn capture_status(&self, context: Reference) -> Result<Value, Error> {
        self.captures
            .get(&context)
            .map(Capture::summary)
            .ok_or("HAR_CAPTURE_INACTIVE".into())
    }
    pub(crate) fn capture_stop(&mut self, context: Reference) -> Result<Vec<Uuid>, Error> {
        let capture = self
            .captures
            .get_mut(&context)
            .ok_or("HAR_CAPTURE_INACTIVE")?;
        capture.stopping = true;
        Ok(capture.pending_bodies())
    }
    pub(crate) fn capture_request(
        &mut self,
        context: Reference,
        (id, owner): (Uuid, RequestOwner),
        request: Value,
        has_body: bool,
        body: Option<&[u8]>,
    ) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.request(id, owner, request, has_body, body);
        }
    }
    /// Start a full-mode socket entry in the context's capture, if it selects it.
    pub(crate) fn capture_socket_start(
        &mut self,
        context: Reference,
        key: String,
        page: Option<Reference>,
        url: &str,
    ) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.socket_start(key, page, url);
        }
    }
    /// Apply a socket event to whichever capture recorded that socket.
    pub(crate) fn capture_socket_event(
        &mut self,
        key: &str,
        event: &super::capture::SocketEvent<'_>,
    ) {
        for capture in self.captures.values_mut() {
            if capture.socket_event(key, event) {
                return;
            }
        }
    }
    /// Whether an active capture records full-mode facts.
    pub(crate) fn capture_full(&self, context: Reference) -> bool {
        self.captures
            .get(&context)
            .is_some_and(|capture| !capture.stopping && capture.mode == CaptureMode::Full)
    }
    pub(crate) fn capture_details(&mut self, context: Reference, id: Uuid, details: &Value) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.details(id, details);
        }
    }
    pub(crate) fn capture_pages(&self, context: Reference) -> Vec<Reference> {
        self.captures
            .get(&context)
            .map(Capture::pages)
            .unwrap_or_default()
    }
    pub(crate) fn capture_page_details(
        &mut self,
        context: Reference,
        page: Reference,
        value: &Value,
    ) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.page_details(page, value);
        }
    }
    pub(crate) fn capture_pending_details(&self, context: Reference) -> Vec<Uuid> {
        self.captures
            .get(&context)
            .map(Capture::pending_details)
            .unwrap_or_default()
    }
    pub(crate) fn capture_response(&mut self, context: Reference, id: Uuid, response: Value) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.response(id, response);
        }
    }
    pub(crate) fn capture_finish(&mut self, context: Reference, id: Uuid, failed: bool) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.finish(id, failed);
        }
    }
    pub(crate) fn capture_body(
        &mut self,
        context: Reference,
        id: Uuid,
        bytes: Option<&[u8]>,
        reason: &str,
    ) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.body(id, false, bytes, reason);
        }
    }
    pub(crate) fn capture_gap(&mut self, count: u64) {
        for capture in self.captures.values_mut() {
            capture.source_gap(count);
        }
    }
    pub(crate) fn capture_context_gap(&mut self, context: Reference, count: u64) {
        if let Some(capture) = self.captures.get_mut(&context) {
            capture.source_gap(count);
        }
    }
    pub(crate) fn capture_publish(
        &mut self,
        directory: &Path,
        context: Reference,
    ) -> Result<Value, Error> {
        let capture = self.captures.get(&context).ok_or("HAR_CAPTURE_INACTIVE")?;
        if !capture.stopping {
            return Err("HAR_CAPTURE_ACTIVE".into());
        }
        let id = capture.id;
        let archive = capture.archive()?;
        let (bytes, format) = if capture.content == CaptureContent::Attach {
            (
                archive.encode_attached()?,
                super::super::artifacts::Format {
                    prefix: "har",
                    extension: "zip",
                    mime: "application/zip",
                    name: "har-1.2-capture-attached",
                    max_bytes: MAX_INPUT,
                },
            )
        } else {
            (
                archive.encode()?,
                super::super::artifacts::Format {
                    prefix: "har",
                    extension: "har",
                    mime: "application/json",
                    name: "har-1.2-capture",
                    max_bytes: MAX_INPUT,
                },
            )
        };
        self.evidence.stage(id,format,bytes,json!({"archiveId":id,"captureId":id,"replacesArchiveId":capture.replaces,"content":capture.content,"privacy":"private-secret-bearing","entryCount":archive.entries.len(),"normalizedForReplay":true}))?;
        let export = capture
            .export
            .clone()
            .map(|export| (export, capture.content));
        self.items.insert(id, archive);
        self.captures.remove(&context);
        let mut result = self.finalize(directory, id).map_err(|mut error| {
            error.partial =
                Some(json!({"archiveId":id,"registered":true,"publicationPending":true}));
            error
        })?;
        if let Some((export, content)) = export {
            // The private archive is already registered; an export failure keeps it.
            let written = export
                .write(self.items.get(&id).expect("Published archive"), content)
                .map_err(|mut error| {
                    error.partial =
                        Some(json!({"archiveId":id,"registered":true,"exportFailed":true}));
                    error
                })?;
            result["export"] = written;
        }
        Ok(result)
    }
}
