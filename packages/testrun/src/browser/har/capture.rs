//! Explicit private capture retention; missing payloads never become empty replies.
use super::*;
use crate::browser::contract::Reference;
use std::collections::BTreeMap;
use uuid::Uuid;
mod sockets;
pub(crate) use sockets::SocketEvent;

pub(super) struct Capture {
    pub id: Uuid,
    pub context: Reference,
    /// Optional page selection; unattributed and worker requests never match it.
    pub page: Option<Reference>,
    pub content: CaptureContent,
    pub mode: CaptureMode,
    pub export: Option<super::Export>,
    bodies: bool,
    pub stopping: bool,
    pub replaces: Option<Uuid>,
    pub filter: Option<super::super::routing::Compiled>,
    entries: BTreeMap<Uuid, Record>,
    order: Vec<Uuid>,
    page_details: BTreeMap<Reference, Value>,
    /// Backend socket key → (entry, first and last message times in ms).
    sockets: BTreeMap<String, (Uuid, Option<f64>, Option<f64>)>,
    metadata_bytes: usize,
    metadata_nodes: usize,
    payload_bytes: usize,
    dropped: u64,
    gaps: u64,
    completed: usize,
}
struct Record {
    value: Value,
    complete: bool,
    page: Option<Reference>,
}
impl Capture {
    pub fn new(context: Reference, content: CaptureContent) -> Self {
        Self {
            id: Uuid::new_v4(),
            context,
            page: None,
            content,
            mode: CaptureMode::Minimal,
            export: None,
            bodies: content != CaptureContent::Omit,
            stopping: false,
            replaces: None,
            filter: None,
            entries: BTreeMap::new(),
            order: Vec::new(),
            page_details: BTreeMap::new(),
            sockets: BTreeMap::new(),
            metadata_bytes: 0,
            metadata_nodes: 0,
            payload_bytes: 0,
            dropped: 0,
            gaps: 0,
            completed: 0,
        }
    }
    /// Admit a request only when its frame's page satisfies the page selection.
    pub fn accepts(&self, url: &str, page: Option<Reference>) -> bool {
        self.page.is_none_or(|selected| page == Some(selected)) && self.accepts_url(url)
    }
    fn accepts_url(&self, url: &str) -> bool {
        !self.stopping
            && (url.starts_with("http://") || url.starts_with("https://"))
            && self
                .filter
                .as_ref()
                .is_none_or(|filter| filter.is_match(url))
    }
    pub fn request(
        &mut self,
        id: Uuid,
        owner: RequestOwner,
        mut request: Value,
        has_body: bool,
        body: Option<&[u8]>,
    ) {
        if self.stopping || self.entries.contains_key(&id) {
            return;
        }
        if !request["url"]
            .as_str()
            .is_some_and(|url| self.accepts_url(url))
        {
            return;
        }
        let has_body = has_body || request.get("postData").is_some();
        if let Some(request) = request.as_object_mut() {
            request.remove("postData");
        }
        if Request::new(
            request["url"].as_str().unwrap_or(""),
            request["method"].as_str().unwrap_or(""),
            request["headers"].clone(),
            None,
        )
        .is_err()
        {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let Ok(mut metadata_bytes) = super::super::schema::encoded_len(&request, 65536) else {
            self.dropped = self.dropped.saturating_add(1);
            return;
        };
        let attribution = match owner {
            RequestOwner::ServiceWorker(worker) => {
                Some(json!({"_serviceWorkerRef":format!("worker@{}",worker.id)}))
            }
            RequestOwner::Page(_) | RequestOwner::Unattributed => None,
        };
        let mut nodes = nodes(&request) + 32;
        if let Some(attribution) = &attribution {
            metadata_bytes += super::super::schema::encoded_len(attribution, 65536)
                .expect("Bounded worker attribution");
            nodes += self::nodes(attribution);
        }
        if self.order.len() >= MAX_ENTRIES
            || self.metadata_nodes + nodes > 90000
            || self.metadata_bytes + self.payload_bytes + metadata_bytes > MAX_BYTES
        {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let observed = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("Current UTC timestamp");
        let mut value = json!({"startedDateTime":observed,"_testrunStartedDateTimeSource":"observer-clock","request":request,"response":{"status":0,"headers":[],"content":{"size":-1,"_testrunBodyUnavailable":true,"_testrunUnavailableReason":"response-not-observed"}},"_testrunCaptureComplete":false});
        value["_testrunRequestId"] = json!(id);
        if let Some(Value::Object(attribution)) = attribution {
            value
                .as_object_mut()
                .expect("Capture entry")
                .extend(attribution);
        }
        if has_body {
            value["request"]["postData"] = unavailable("request-body-not-observed");
        }
        self.metadata_bytes += metadata_bytes;
        self.metadata_nodes += nodes;
        self.order.push(id);
        self.entries.insert(
            id,
            Record {
                value,
                complete: false,
                page: owner.page(),
            },
        );
        if has_body {
            self.body(id, true, body, "request-body-not-observed");
        }
    }
    pub fn response(&mut self, id: Uuid, mut response: Value) {
        let Some(record) = self.entries.get_mut(&id) else {
            return;
        };
        if !response["status"]
            .as_u64()
            .is_some_and(|status| (100..=599).contains(&status))
            || headers(&response["headers"]).is_err()
        {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let metadata = super::super::schema::encoded_len(&response, 65536);
        let Ok(metadata) = metadata else {
            self.dropped = self.dropped.saturating_add(1);
            return;
        };
        let nodes = nodes(&response);
        if self.metadata_nodes + nodes > 90000
            || self.metadata_bytes + self.payload_bytes + metadata > MAX_BYTES
        {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.metadata_bytes += metadata;
        self.metadata_nodes += nodes;
        let no_body = record.value["request"]["method"] == "HEAD"
            || matches!(response["status"].as_u64(), Some(204 | 205 | 304));
        response["content"] = if no_body {
            json!({"size":0,"text":""})
        } else {
            unavailable("response-body-not-collected")
        };
        record.value["response"] = response;
    }
    pub fn finish(&mut self, id: Uuid, failed: bool) {
        if let Some(record) = self.entries.get_mut(&id) {
            if !record.complete {
                self.completed += 1;
            }
            record.complete = true;
            record.value["_testrunCaptureComplete"] = json!(true);
            record.value["_testrunRequestFailed"] = json!(failed);
        }
    }
    pub fn body(&mut self, id: Uuid, request: bool, bytes: Option<&[u8]>, reason: &str) {
        let Some(record) = self.entries.get_mut(&id) else {
            return;
        };
        let content = if !self.bodies {
            unavailable("content-omitted-by-policy")
        } else if let Some(bytes) = bytes {
            if bytes.len() > MAX_BODY
                || self.metadata_bytes + self.payload_bytes + bytes.len() > MAX_BYTES
            {
                unavailable("capture-payload-bound")
            } else {
                self.payload_bytes += bytes.len();
                json!({"size":bytes.len(),"encoding":"base64","text":STANDARD.encode(bytes)})
            }
        } else {
            unavailable(reason)
        };
        if request {
            record.value["request"]["postData"] = content;
        } else {
            record.value["response"]["content"] = content;
        }
    }
    /// Merge bounded full-mode facts; minimal captures and unknown keys ignore them.
    pub fn details(&mut self, id: Uuid, details: &Value) {
        if self.mode != CaptureMode::Full {
            return;
        }
        let Some(record) = self.entries.get_mut(&id) else {
            return;
        };
        let Some(details) = details.as_object() else {
            return;
        };
        for key in super::full::DETAIL_KEYS {
            let Some(value) = details.get(key).filter(|value| !value.is_null()) else {
                continue;
            };
            let value = match key {
                "securityDetails" => match super::full::security(value) {
                    // Some backends report `{}` for plain HTTP; no fields means no TLS facts.
                    details if details.as_object().is_some_and(serde_json::Map::is_empty) => {
                        continue;
                    }
                    details => details,
                },
                "timing" if value.is_object() => json!(
                    [
                        "startTime",
                        "domainLookupStart",
                        "domainLookupEnd",
                        "connectStart",
                        "secureConnectionStart",
                        "connectEnd",
                        "requestStart",
                        "responseStart"
                    ]
                    .into_iter()
                    .filter_map(|field| value[field]
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .map(|value| (field.to_owned(), json!(value))))
                    .collect::<serde_json::Map<_, _>>()
                ),
                "serverIPAddress" | "httpVersion" => match value.as_str() {
                    Some(text) if text.len() <= 64 => json!(text),
                    _ => continue,
                },
                "requestCookieHeader" => match value.as_str() {
                    Some(text) if text.len() <= 8192 => json!(text),
                    _ => continue,
                },
                _ if value.is_number() => value.clone(),
                _ => continue,
            };
            record.value["_testrunDetails"][key] = value;
        }
    }
    /// Distinct attributed pages of a full-mode capture, in first-entry order.
    pub fn pages(&self) -> Vec<Reference> {
        if self.mode != CaptureMode::Full {
            return Vec::new();
        }
        let mut pages = Vec::new();
        for page in self
            .order
            .iter()
            .filter_map(|id| self.entries.get(id).and_then(|record| record.page))
        {
            if !pages.contains(&page) {
                pages.push(page);
            }
        }
        pages
    }
    /// Record a page's title and load timings; invalid shapes are ignored.
    pub fn page_details(&mut self, page: Reference, value: &Value) {
        if self.mode == CaptureMode::Full
            && let Some(details) = super::full::page_details(value)
        {
            self.page_details.insert(page, details);
        }
    }
    /// Completed responses whose full-mode facts the driver collects at stop.
    pub fn pending_details(&self) -> Vec<Uuid> {
        if self.mode != CaptureMode::Full {
            return Vec::new();
        }
        self.order
            .iter()
            .filter(|id| {
                self.entries.get(id).is_some_and(|record| {
                    record.complete
                        && record.value["response"]["status"]
                            .as_u64()
                            .is_some_and(|status| status >= 100)
                })
            })
            .copied()
            .collect()
    }
    pub fn source_gap(&mut self, count: u64) {
        self.gaps = self.gaps.saturating_add(count);
    }
    pub fn pending_bodies(&self) -> Vec<Uuid> {
        if !self.bodies {
            return Vec::new();
        }
        self.order
            .iter()
            .filter(|id| {
                self.entries.get(id).is_some_and(|record| {
                    record.complete
                        && record.value["_testrunRequestFailed"] != true
                        && record.value["response"]["status"]
                            .as_u64()
                            .is_some_and(|status| status >= 200)
                        && record.value["response"]["content"]["_testrunUnavailableReason"]
                            == "response-body-not-collected"
                })
            })
            .copied()
            .collect()
    }
    pub fn summary(&self) -> Value {
        json!({"captureId":self.id,"context":self.context,"replacesArchiveId":self.replaces,"content":self.content,"mode":self.mode,"exportConfigured":self.export.is_some(),"urlFilterActive":self.filter.is_some(),"page":self.page,"phase":if self.stopping{"stopping"}else{"collecting"},"entries":self.entries.len(),"completedEntries":self.completed,"payloadBytes":self.payload_bytes,"droppedEntries":self.dropped,"sourceGaps":self.gaps,"privacy":"private-secret-bearing"})
    }
    pub fn archive(&self) -> Result<Archive, Error> {
        let full = self.mode == CaptureMode::Full;
        let mut pages: Vec<(Reference, Value)> = Vec::new();
        let entries = self
            .order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .map(|record| {
                let mut value = record.value.clone();
                if !full {
                    if let Some(entry) = value.as_object_mut() {
                        entry.remove("_testrunDetails");
                    }
                    return value;
                }
                let id = record.page.map(|page| format!("page@{}", page.id));
                if let (Some(page), Some(id)) = (record.page, &id)
                    && !pages.iter().any(|(known, _)| *known == page)
                {
                    let started = value["startedDateTime"].as_str().unwrap_or("");
                    pages.push((
                        page,
                        super::full::page(id, started, self.page_details.get(&page)),
                    ));
                }
                if value["_resourceType"] == "websocket" {
                    // Socket entries are recorded in their final pinned shape.
                    if let Some(id) = &id {
                        value["pageref"] = json!(id);
                    }
                } else {
                    super::full::entry(&mut value, id.as_deref());
                }
                value
            })
            .collect::<Vec<_>>();
        let mut value = json!({"log":{"version":"1.2","creator":{"name":"Testrun","version":env!("CARGO_PKG_VERSION")},"entries":entries,"_testrunCapture":self.summary()}});
        if full {
            value["log"]["pages"] =
                json!(pages.into_iter().map(|(_, page)| page).collect::<Vec<_>>());
        }
        Archive::parse(&value, |_| Err("HAR_CAPTURE_ATTACHMENT_UNAVAILABLE".into()))
    }
}
fn unavailable(reason: &str) -> Value {
    json!({"size":-1,"_testrunBodyUnavailable":true,"_testrunUnavailableReason":reason})
}
fn nodes(value: &Value) -> usize {
    match value {
        Value::Array(values) => 1 + values.iter().map(nodes).sum::<usize>(),
        Value::Object(values) => 1 + values.values().map(nodes).sum::<usize>(),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(bodies: bool) -> Capture {
        Capture::new(
            Reference {
                connection_id: Uuid::new_v4(),
                id: Uuid::new_v4(),
                generation: Uuid::new_v4(),
            },
            if bodies {
                CaptureContent::Embed
            } else {
                CaptureContent::Omit
            },
        )
    }
    fn request() -> Value {
        json!({"url":"https://example.com/acme?private=secret","method":"POST","headers":[]})
    }
    #[test]
    fn filters_exclude_requests_before_metadata_and_payload_budgets() {
        use crate::browser::routing::Matcher;
        for filter in [
            Matcher::Exact {
                url: "https://example.com/acme?private=secret".into(),
            },
            Matcher::Glob {
                pattern: "**/acme?private=*".into(),
            },
            Matcher::Regex {
                pattern: r"/ACME\?private=secret$".into(),
                flags: "i".into(),
            },
        ] {
            let mut capture = fixture(true);
            capture.filter = Some(filter.compile().unwrap());
            let excluded = Uuid::new_v4();
            let payload = vec![0; MAX_BODY + 1];
            for _ in 0..MAX_ENTRIES + 1 {
                capture.request(excluded, RequestOwner::Unattributed, json!({"url":"https://example.com/excluded","method":"invalid method","headers":[{"name":"large","value":"x".repeat(65537)}]}), true, Some(&payload));
            }
            capture.response(excluded, json!({"status":999,"headers":"invalid"}));
            capture.finish(excluded, false);
            capture.body(excluded, false, Some(&payload), "");
            assert_eq!(capture.summary()["entries"], 0);
            assert_eq!(capture.summary()["droppedEntries"], 0);
            assert_eq!(capture.metadata_bytes, 0);
            assert_eq!(capture.payload_bytes, 0);
            assert!(!capture.accepts("https://example.com/acme?other=secret", None));
            let selected = Uuid::new_v4();
            capture.request(
                selected,
                RequestOwner::Unattributed,
                request(),
                true,
                Some(b"selected"),
            );
            capture.response(selected, json!({"status":200,"headers":[]}));
            capture.finish(selected, false);
            assert_eq!(capture.pending_bodies(), vec![selected]);
            assert_eq!(capture.summary()["entries"], 1);
            assert_eq!(capture.summary()["payloadBytes"], 8);
            assert_eq!(capture.summary()["urlFilterActive"], true);
            assert!(!capture.summary().to_string().contains("private="));
            capture.stopping = true;
            assert!(!capture.accepts("https://example.com/acme?private=secret", None));
        }
    }
    #[test]
    fn worker_source_identity_survives_encoding_and_consumes_metadata_capacity() {
        let worker = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let id = Uuid::new_v4();
        let mut plain = fixture(false);
        plain.request(id, RequestOwner::Unattributed, request(), false, None);
        let mut captured = fixture(false);
        captured.request(
            id,
            RequestOwner::ServiceWorker(worker),
            request(),
            false,
            None,
        );
        assert!(captured.metadata_bytes > plain.metadata_bytes);
        assert!(captured.metadata_nodes > plain.metadata_nodes);
        for mode in [CaptureMode::Full, CaptureMode::Minimal] {
            captured.mode = mode;
            let archive: Value =
                serde_json::from_slice(&captured.archive().unwrap().encode().unwrap()).unwrap();
            assert_eq!(
                archive["log"]["entries"][0]["_serviceWorkerRef"],
                format!("worker@{}", worker.id)
            );
            assert!(archive["log"]["entries"][0]["pageref"].is_null());
            assert_eq!(
                archive["log"]["entries"][0]["_testrunCaptureComplete"],
                false
            );
        }
        let mut boundary = fixture(false);
        boundary.metadata_bytes = MAX_BYTES - plain.metadata_bytes;
        boundary.request(
            id,
            RequestOwner::ServiceWorker(worker),
            request(),
            false,
            None,
        );
        assert!(boundary.entries.is_empty());
        assert_eq!(boundary.summary()["droppedEntries"], 1);
        boundary.request(id, RequestOwner::Unattributed, request(), false, None);
        assert_eq!(
            boundary.entries.len(),
            1,
            "The original metadata still fits; attribution cannot evade its additional budget"
        );
    }
    #[test]
    fn full_details_ignore_null_and_unknown_values_and_minimal_ignores_all() {
        let mut minimal = fixture(false);
        let id = Uuid::new_v4();
        minimal.request(id, RequestOwner::Unattributed, request(), false, None);
        minimal.details(id, &json!({"serverIPAddress":"127.0.0.1"}));
        assert!(minimal.entries[&id].value.get("_testrunDetails").is_none());
        let mut full = fixture(false);
        full.mode = CaptureMode::Full;
        full.request(id, RequestOwner::Unattributed, request(), false, None);
        full.details(id, &json!({"securityDetails":{"keyExchange":"ignored"},"serverIPAddress":"x".repeat(65),"unknown":1,"transferSize":"12","serverPort":443}));
        assert_eq!(
            full.entries[&id].value["_testrunDetails"],
            json!({"serverPort":443})
        );
        full.response(id, json!({"status":200,"headers":[]}));
        full.finish(id, false);
        let archive: Value =
            serde_json::from_slice(&full.archive().unwrap().encode().unwrap()).unwrap();
        let entry = &archive["log"]["entries"][0];
        assert!(entry.get("_securityDetails").is_none(), "{entry}");
        assert!(entry.get("serverIPAddress").is_none(), "{entry}");
        assert_eq!(entry["_serverPort"], 443);
        assert!(archive["log"]["pages"].as_array().unwrap().is_empty());
    }
    #[test]
    fn full_pages_use_read_titles_and_timings_and_keep_unknowns_for_unread_pages() {
        let mut full = fixture(false);
        full.mode = CaptureMode::Full;
        let read = Reference {
            connection_id: full.context.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let closed = Reference {
            id: Uuid::new_v4(),
            ..read
        };
        for page in [read, closed, read] {
            full.request(
                Uuid::new_v4(),
                RequestOwner::Page(page),
                request(),
                false,
                None,
            );
        }
        assert_eq!(full.pages(), vec![read, closed]);
        full.page_details(closed, &json!({"title":7}));
        full.page_details(
            read,
            &json!({"title":"Acme","onContentLoad":1700.0,"onLoad":0}),
        );
        let archive: Value =
            serde_json::from_slice(&full.archive().unwrap().encode().unwrap()).unwrap();
        let pages = archive["log"]["pages"].as_array().unwrap();
        assert_eq!(pages[0]["title"], "Acme");
        assert_eq!(
            pages[0]["pageTimings"],
            json!({"onContentLoad":1700.0,"onLoad":-1.0})
        );
        assert!(pages[0].get("_testrunTitleUnavailable").is_none());
        assert_eq!(pages[1]["title"], "");
        assert_eq!(pages[1]["_testrunTitleUnavailable"], true);
        assert!(fixture(false).pages().is_empty());
    }
    #[test]
    fn page_selection_admits_only_attributed_requests_of_the_selected_page() {
        let mut capture = fixture(true);
        let selected = Reference {
            connection_id: capture.context.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let sibling = Reference {
            id: Uuid::new_v4(),
            ..selected
        };
        let url = "https://example.com/acme?private=secret";
        assert!(capture.accepts(url, None));
        assert!(capture.accepts(url, Some(sibling)));
        capture.page = Some(selected);
        assert!(capture.accepts(url, Some(selected)));
        assert!(!capture.accepts(url, Some(sibling)));
        assert!(!capture.accepts(url, None));
        assert!(!capture.accepts(
            url,
            Some(Reference {
                generation: Uuid::new_v4(),
                ..selected
            })
        ));
        assert!(!capture.accepts("data:text/plain,Acme", Some(selected)));
        assert_eq!(capture.summary()["page"], json!(selected));
        capture.stopping = true;
        assert!(!capture.accepts(url, Some(selected)));
    }
    #[test]
    fn explicit_capture_preserves_private_binary_bytes_and_never_invents_missing_bodies() {
        let mut capture = fixture(true);
        let id = Uuid::new_v4();
        capture.request(
            id,
            RequestOwner::Unattributed,
            request(),
            true,
            Some(b"Acme request"),
        );
        capture.response(id, json!({"status":201,"headers":[]}));
        capture.finish(id, false);
        assert_eq!(capture.pending_bodies(), vec![id]);
        capture.body(id, false, Some(&[0, 255, 128]), "");
        assert!(capture.pending_bodies().is_empty());
        let archive = capture.archive().unwrap();
        let encoded: Value = serde_json::from_slice(&archive.encode().unwrap()).unwrap();
        assert_eq!(encoded["log"]["entries"][0]["_testrunRequestId"], json!(id));
        let query = Request::new(
            "https://example.com/acme?private=secret",
            "POST",
            json!([]),
            Some(b"Acme request".to_vec()),
        )
        .unwrap();
        assert_eq!(
            archive
                .find(&query)
                .unwrap()
                .unwrap()
                .response
                .body
                .as_deref(),
            Some(&[0, 255, 128][..])
        );
        let mut omitted = fixture(false);
        let id = Uuid::new_v4();
        omitted.request(
            id,
            RequestOwner::Unattributed,
            request(),
            true,
            Some(b"secret"),
        );
        omitted.response(id, json!({"status":200,"headers":[]}));
        omitted.finish(id, false);
        let encoded = String::from_utf8(omitted.archive().unwrap().encode().unwrap()).unwrap();
        assert!(!encoded.contains("c2VjcmV0"));
        assert!(encoded.contains("_testrunBodyUnavailable"));
    }
    #[test]
    fn capture_limits_and_source_loss_remain_visible_in_the_archive() {
        let mut capture = fixture(true);
        for _ in 0..MAX_ENTRIES + 1 {
            capture.request(
                Uuid::new_v4(),
                RequestOwner::Unattributed,
                request(),
                false,
                None,
            );
        }
        capture.source_gap(3);
        capture.stopping = true;
        capture.request(
            Uuid::new_v4(),
            RequestOwner::Unattributed,
            request(),
            false,
            None,
        );
        assert_eq!(capture.summary()["entries"], MAX_ENTRIES);
        assert_eq!(capture.summary()["droppedEntries"], 1);
        assert_eq!(capture.summary()["sourceGaps"], 3);
        let archive = capture.archive().unwrap();
        assert_eq!(archive.entries.len(), MAX_ENTRIES);
    }
}
