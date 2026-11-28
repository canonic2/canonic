//! Bounded HAR replay data, separate from browser routing and file acquisition.
use super::{routing::Header, session::Error};
use axum::http;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::collections::BTreeSet;
mod capture;
mod files;
mod full;
pub(crate) use capture::SocketEvent;
pub(crate) use full::{PAGE_DETAILS, http_version};
mod store;
pub(crate) use files::Export;
use serde::{Deserialize, Serialize};
pub(crate) use store::Store;

/// A captured request's observed source, without granting dispatch authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestOwner {
    Page(super::contract::Reference),
    ServiceWorker(super::contract::Reference),
    Unattributed,
}
impl RequestOwner {
    pub(crate) fn page(self) -> Option<super::contract::Reference> {
        match self {
            Self::Page(page) => Some(page),
            Self::ServiceWorker(_) | Self::Unattributed => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotFound {
    #[default]
    Abort,
    /// Send the request to the network.
    Continue,
    /// Pass the request to the next route handler, as for `route_fallback`.
    Fallback,
}
/// Capture body retention, matching the pinned `content` option names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureContent {
    /// Record metadata only; bodies carry `content-omitted-by-policy`.
    #[default]
    Omit,
    /// Store bodies inline as base64 in the published `.har`.
    Embed,
    /// Publish a ZIP whose `har.har` references one member per distinct body.
    Attach,
}
impl CaptureContent {
    /// `har_capture_start` default: pinned `routeFromHAR({update})` uses
    /// `attach` for a re-record; a standalone capture keeps `omit`.
    pub(crate) fn for_capture(content: Option<Self>, replaces: Option<uuid::Uuid>) -> Self {
        content.unwrap_or(if replaces.is_some() {
            Self::Attach
        } else {
            Self::Omit
        })
    }
}
/// Capture field coverage, matching the pinned `mode` option names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Fields needed for replay.
    #[default]
    Minimal,
    /// Adds pages, cookies, query strings, sizes, timings, HTTP version, server address and TLS details.
    Full,
}
pub(crate) const OPERATIONS: [&str; 9] = [
    "har_register",
    "har_list",
    "har_finalize",
    "har_remove",
    "page_har_set",
    "context_har_set",
    "har_capture_start",
    "har_capture_status",
    "har_capture_stop",
];
pub(crate) fn capabilities(available: bool) -> Value {
    json!({"available":available,"operations":OPERATIONS,"replay":"callback-or-automatic-scoped-rule","automaticRouting":available,"routingScope":"context-or-page-and-child-frames","maxAutomaticPages":64,"maxAutomaticScopes":64,"maxAutomaticCallbacks":64,"maxParallelDriverReplies":4,"capture":available,"captureScope":"owned-context-network-events","capturePageSelection":"optional-owned-page-frame-requests","captureMode":["minimal","full"],"capturePageTitles":"current-document-at-stop","captureContent":["omit","embed","attach"],"captureAttachments":"zip-members-for-attach","captureBodies":"explicit-content-with-unavailable-reasons","capturePublication":"retained-local-finalization","update":available,"updateMode":"re-record-new-immutable-archive","maxArchives":4,"maxEntries":MAX_ENTRIES,"maxInputBytes":MAX_INPUT,"maxPayloadBytes":MAX_BYTES,"maxBodyBytes":MAX_BODY,"maxRedirectEntries":64,"bodyFormat":"decoded-har-payload","framing":"decoded-payload-with-head-304-metadata","attachments":["private-relative-files","zip-members"],"navigationRedirects":"automatic-rules-only","unknownRequestBytes":"explicit-refusal","missingEntryDefault":"abort","networkContinuation":"explicit-notFound-continue","handlerFallback":"explicit-notFound-fallback"})
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let id = json!({"type":"string","format":"uuid"});
    vec![
        object(
            json!({"operation":{"const":"har_capture_start"},"context":reference,"page":{"anyOf":[reference,{"type":"null"}]},"content":{"enum":["omit","embed","attach"],"description":"Default attach with replacesArchiveId, else omit"},"mode":{"enum":["minimal","full"],"default":"minimal"},"path":{"type":["string","null"],"maxLength":4096,"description":"New absolute .har or .zip file in a private directory to also receive the archive at stop"},"resourcesDir":{"type":["string","null"],"maxLength":4096,"description":"Private directory for attached bodies of a .har path"},"urlFilter":{"anyOf":[super::routing::Matcher::schema(),{"type":"null"}]},"replacesArchiveId":{"type":["string","null"],"format":"uuid"}}),
            &["operation", "context"],
        ),
        object(
            json!({"operation":{"const":"har_capture_status"},"context":reference}),
            &["operation", "context"],
        ),
        object(
            json!({"operation":{"const":"har_capture_stop"},"context":reference}),
            &["operation", "context"],
        ),
        object(
            json!({"operation":{"const":"page_har_set"},"page":reference,"archiveId":{"anyOf":[id,{"type":"null"}]},"urlFilter":{"anyOf":[super::routing::Matcher::schema(),{"type":"null"}]},"notFound":{"enum":["abort","continue","fallback"],"default":"abort"}}),
            &["operation", "page", "archiveId"],
        ),
        object(
            json!({"operation":{"const":"context_har_set"},"context":reference,"archiveId":{"anyOf":[id,{"type":"null"}]},"urlFilter":{"anyOf":[super::routing::Matcher::schema(),{"type":"null"}]},"notFound":{"enum":["abort","continue","fallback"],"default":"abort"}}),
            &["operation", "context", "archiveId"],
        ),
        object(
            json!({"operation":{"const":"har_register"},"path":{"type":"string","minLength":1,"maxLength":4096}}),
            &["operation", "path"],
        ),
        object(json!({"operation":{"const":"har_list"}}), &["operation"]),
        object(
            json!({"operation":{"const":"har_finalize"},"archiveId":id}),
            &["operation", "archiveId"],
        ),
        object(
            json!({"operation":{"const":"har_remove"},"archiveId":id}),
            &["operation", "archiveId"],
        ),
    ]
}

pub(crate) const MAX_ENTRIES: usize = 256;
pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_INPUT: usize = 32 * 1024 * 1024;
pub(crate) const MAX_BODY: usize = super::network::MAX_BODY;

pub(crate) fn capture_headers(value: &Value) -> Value {
    if let Some(fields) = value.as_object() {
        json!(
            fields
                .iter()
                .filter_map(|(name, value)| value
                    .as_str()
                    .map(|value| json!({"name":name,"value":value})))
                .collect::<Vec<_>>()
        )
    } else {
        value.clone()
    }
}

#[derive(Clone)]
pub(crate) struct Request {
    pub url: String,
    pub method: String,
    pub headers: Vec<Header>,
    pub body: Option<Vec<u8>>,
}
impl Request {
    pub(crate) fn new(
        url_text: &str,
        method: &str,
        fields: Value,
        body: Option<Vec<u8>>,
    ) -> Result<Self, Error> {
        if method.len() > 32
            || http::Method::from_bytes(method.as_bytes()).is_err()
            || body.as_ref().is_some_and(|body| body.len() > MAX_BODY)
        {
            return Err("HAR_REQUEST_BOUNDS".into());
        }
        Ok(Self {
            url: url(&json!(url_text))?,
            method: method.into(),
            headers: headers(&fields)?,
            body,
        })
    }
}
pub(crate) struct Response {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Option<Vec<u8>>,
}
struct Entry {
    request: Request,
    response: Response,
    template: Value,
    unknown_post_data: bool,
}
pub(crate) struct Archive {
    entries: Vec<Entry>,
    /// WebSocket entries with their original positions: kept, never replayed.
    sockets: Vec<(usize, Value)>,
    log: Value,
}
pub(crate) struct Found<'a> {
    pub response: &'a Response,
    pub url: &'a str,
    pub redirects: usize,
}

fn headers(value: &Value) -> Result<Vec<Header>, Error> {
    let input = value.as_array().ok_or("HAR_INVALID_HEADERS")?;
    if input.len() > 64 {
        return Err("HAR_INVALID_HEADERS".into());
    }
    let headers = input
        .iter()
        .map(|header| {
            Ok(Header {
                name: header["name"].as_str().ok_or("HAR_INVALID_HEADERS")?.into(),
                value: header["value"]
                    .as_str()
                    .ok_or("HAR_INVALID_HEADERS")?
                    .into(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let mut size = 0;
    for header in &headers {
        size += header.name.len() + header.value.len();
        if header.name.len() > 256
            || size > 8192
            || http::HeaderName::from_bytes(header.name.as_bytes()).is_err()
            || http::HeaderValue::from_str(&header.value).is_err()
        {
            return Err("HAR_INVALID_HEADERS".into());
        }
    }
    Ok(headers)
}
fn url(value: &Value) -> Result<String, Error> {
    let text = value.as_str().ok_or("HAR_INVALID_URL")?;
    let parsed = url::Url::parse(text).map_err(|_| Error::from("HAR_INVALID_URL"))?;
    if text.len() > 8192
        || !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("HAR_INVALID_URL".into());
    }
    Ok(text.into())
}
/// Pinned `content: "attach"`: a socket entry's messages become a `.jsonl`
/// member, one message per line, named by `response.content._file`; the entry
/// keeps no `_webSocketMessages`. A socket without messages gets no member.
fn attach_messages(
    socket: &mut Value,
    members: &mut std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<(), Error> {
    let Some(messages) = socket
        .as_object_mut()
        .and_then(|entry| entry.remove("_webSocketMessages"))
    else {
        return Ok(());
    };
    let messages = messages.as_array().ok_or("HAR_INVALID_ARCHIVE")?;
    if messages.is_empty() {
        return Ok(());
    }
    let id = socket["_testrunRequestId"]
        .as_str()
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .unwrap_or_else(uuid::Uuid::new_v4);
    let name = format!("{id}.jsonl");
    let mut lines = Vec::new();
    for message in messages {
        serde_json::to_writer(&mut lines, message)
            .map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?;
        lines.push(b'\n');
    }
    socket["response"]["content"]["_file"] = json!(name);
    members.insert(name, lines);
    Ok(())
}
/// The reverse of [`attach_messages`] for an imported socket entry: its `.jsonl`
/// member becomes `_webSocketMessages` again. Returns the member's size.
fn detach_messages(
    socket: &mut Value,
    load: &mut impl FnMut(&str) -> Result<Vec<u8>, Error>,
) -> Result<usize, Error> {
    if socket.get("_webSocketMessages").is_some() {
        return Ok(0);
    }
    let Some(file) = socket["response"]["content"].get("_file") else {
        return Ok(0);
    };
    let bytes = load(file.as_str().ok_or("HAR_INVALID_ATTACHMENT")?)?;
    let messages = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_slice::<Value>(line)
                .ok()
                .filter(Value::is_object)
                .ok_or(Error::from("HAR_INVALID_ATTACHMENT"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    socket["_webSocketMessages"] = json!(messages);
    if let Some(content) = socket["response"]["content"].as_object_mut() {
        content.remove("_file");
    }
    Ok(bytes.len())
}
fn content(
    value: &Value,
    load: &mut impl FnMut(&str) -> Result<Vec<u8>, Error>,
) -> Result<Option<Vec<u8>>, Error> {
    if !value.is_object() {
        return Err("HAR_INVALID_BODY".into());
    }
    let bytes = if let Some(file) = value.get("_file") {
        load(file.as_str().ok_or("HAR_INVALID_ATTACHMENT")?)?
    } else if let Some(text) = value.get("text") {
        let text = text.as_str().ok_or("HAR_INVALID_BODY")?;
        match value.get("encoding").and_then(Value::as_str) {
            Some("base64") => {
                if text.len() > MAX_BODY.div_ceil(3) * 4 {
                    return Err("HAR_BODY_BOUNDS".into());
                }
                STANDARD
                    .decode(text)
                    .map_err(|_| Error::from("HAR_INVALID_BODY"))?
            }
            None | Some("") => {
                if text.len() > MAX_BODY {
                    return Err("HAR_BODY_BOUNDS".into());
                }
                text.as_bytes().to_vec()
            }
            _ => return Err("HAR_BODY_ENCODING_UNSUPPORTED".into()),
        }
    } else if value["size"].as_i64().is_some_and(|size| size > 0)
        || value["_testrunBodyUnavailable"] == true
    {
        return Ok(None);
    } else {
        Vec::new()
    };
    if bytes.len() > MAX_BODY {
        return Err("HAR_BODY_BOUNDS".into());
    }
    Ok(Some(bytes))
}
/// Pinned attachment names use the content digest and a MIME-derived extension.
fn attachment_name(bytes: &[u8], headers: &[Header]) -> String {
    use sha2::{Digest, Sha256};
    let mime = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| {
            header
                .value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    let extension = match mime.as_str() {
        "application/json" => "json",
        "text/html" => "html",
        "text/plain" => "txt",
        "text/css" => "css",
        "text/javascript" | "application/javascript" => "js",
        "image/png" => "png",
        "image/jpeg" => "jpeg",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        "application/wasm" => "wasm",
        "application/xml" | "text/xml" => "xml",
        _ => "dat",
    };
    format!("{:x}.{extension}", Sha256::digest(bytes))
}
impl Archive {
    /// Entries with bodies inline, or as member names when `members` collects attachments.
    fn encoded(
        &self,
        mut members: Option<&mut std::collections::BTreeMap<String, Vec<u8>>>,
    ) -> Result<Value, Error> {
        let mut content = |body: &Option<Vec<u8>>, headers: &[Header]| match (body, &mut members) {
            (None, _) => json!({"size":-1,"_testrunBodyUnavailable":true}),
            (Some(bytes), Some(members)) if !bytes.is_empty() => {
                let name = attachment_name(bytes, headers);
                members.entry(name.clone()).or_insert_with(|| bytes.clone());
                json!({"size":bytes.len(),"_file":name})
            }
            (Some(bytes), _) => {
                json!({"size":bytes.len(),"encoding":"base64","text":STANDARD.encode(bytes)})
            }
        };
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                let mut value = entry.template.clone();
                if entry.request.body.is_some() {
                    for (key, data) in content(&entry.request.body, &entry.request.headers)
                        .as_object()
                        .expect("Body content")
                    {
                        value["request"]["postData"][key] = data.clone();
                    }
                }
                for (key, data) in content(&entry.response.body, &entry.response.headers)
                    .as_object()
                    .expect("Body content")
                {
                    value["response"]["content"][key] = data.clone();
                }
                value["_testrunNormalizedForReplay"] = json!(true);
                value
            })
            .collect::<Vec<_>>();
        let mut entries = entries;
        for (index, socket) in &self.sockets {
            let mut socket = socket.clone();
            if let Some(members) = members.as_deref_mut() {
                attach_messages(&mut socket, members)?;
            }
            entries.insert((*index).min(entries.len()), socket);
        }
        let mut value = json!({"log":self.log});
        value["log"]["entries"] = json!(entries);
        value["log"]["_testrunNormalizedForReplay"] = json!(true);
        super::schema::encoded_len(&value, MAX_INPUT)
            .map_err(|_| Error::from("HAR_ARCHIVE_BOUNDS"))?;
        Ok(value)
    }
    /// The attached `.har` value and its distinct body members.
    pub(crate) fn encoded_members(
        &self,
    ) -> Result<(Value, std::collections::BTreeMap<String, Vec<u8>>), Error> {
        let mut members = std::collections::BTreeMap::new();
        let value = self.encoded(Some(&mut members))?;
        Ok((value, members))
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec(&self.encoded(None)?).map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))
    }
    /// A ZIP with `har.har` and one member per distinct nonempty body, readable by `har_register`.
    pub(crate) fn encode_attached(&self) -> Result<Vec<u8>, Error> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut members = std::collections::BTreeMap::new();
        let har = serde_json::to_vec(&self.encoded(Some(&mut members))?)
            .map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?;
        if members.len() >= 512
            || har.len() + members.values().map(Vec::len).sum::<usize>() > MAX_INPUT
        {
            return Err("HAR_ARCHIVE_BOUNDS".into());
        }
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o600);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let failed = |_| Error::from("HAR_INVALID_ARCHIVE");
        for (name, bytes) in std::iter::once(("har.har", &har))
            .chain(members.iter().map(|(name, bytes)| (name.as_str(), bytes)))
        {
            zip.start_file(name, options).map_err(failed)?;
            zip.write_all(bytes)
                .map_err(|error| Error::caused("HAR_IO", error))?;
        }
        let bytes = zip.finish().map_err(failed)?.into_inner();
        if bytes.len() > MAX_INPUT {
            return Err("HAR_ARCHIVE_BOUNDS".into());
        }
        Ok(bytes)
    }
    pub(crate) fn parse(
        value: &Value,
        mut load: impl FnMut(&str) -> Result<Vec<u8>, Error>,
    ) -> Result<Self, Error> {
        super::schema::encoded_len(value, MAX_INPUT)
            .map_err(|_| Error::from("HAR_ARCHIVE_BOUNDS"))?;
        if value["log"]["version"] != "1.2" {
            return Err("HAR_VERSION_UNSUPPORTED".into());
        }
        let input = value["log"]["entries"]
            .as_array()
            .ok_or("HAR_INVALID_ARCHIVE")?;
        if input.len() > MAX_ENTRIES {
            return Err("HAR_ENTRY_BOUNDS".into());
        }
        let mut entries = Vec::new();
        let mut sockets = Vec::new();
        let mut bytes = 0;
        for (index, entry) in input.iter().enumerate() {
            if entry["_resourceType"] == "websocket" {
                if !entry.is_object() {
                    return Err("HAR_INVALID_ARCHIVE".into());
                }
                let mut entry = entry.clone();
                bytes += detach_messages(&mut entry, &mut load)?;
                if bytes > MAX_BYTES {
                    return Err("HAR_ARCHIVE_BOUNDS".into());
                }
                sockets.push((index, entry));
                continue;
            }
            let source = &entry["request"];
            let method = source["method"].as_str().ok_or("HAR_INVALID_METHOD")?;
            if method.len() > 32 || http::Method::from_bytes(method.as_bytes()).is_err() {
                return Err("HAR_INVALID_METHOD".into());
            }
            let request = Request {
                url: url(&source["url"])?,
                method: method.into(),
                headers: headers(&source["headers"])?,
                body: source
                    .get("postData")
                    .map(|data| content(data, &mut load))
                    .transpose()?
                    .flatten(),
            };
            let source = &entry["response"];
            let status = source["status"]
                .as_u64()
                .filter(|status| *status == 0 || (100..=599).contains(status))
                .ok_or("HAR_INVALID_STATUS")? as u16;
            let mut response = Response {
                status,
                headers: headers(&source["headers"])?,
                body: content(&source["content"], &mut load)?,
            };
            if response.body.is_none()
                && (request.method == "HEAD" || matches!(status, 204 | 205 | 304))
            {
                response.body = Some(Vec::new());
            }
            bytes += request.url.len()
                + request.method.len()
                + request
                    .headers
                    .iter()
                    .chain(&response.headers)
                    .map(|h| h.name.len() + h.value.len())
                    .sum::<usize>()
                + request.body.as_ref().map_or(0, Vec::len)
                + response.body.as_ref().map_or(0, Vec::len);
            if bytes > MAX_BYTES {
                return Err("HAR_ARCHIVE_BOUNDS".into());
            }
            let template = Self::entry_metadata(entry);
            let unknown_post_data =
                entry["request"].get("postData").is_some() && request.body.is_none();
            entries.push(Entry {
                request,
                response,
                template,
                unknown_post_data,
            });
        }
        let mut log = value["log"].clone();
        log.as_object_mut()
            .ok_or("HAR_INVALID_ARCHIVE")?
            .remove("entries");
        Ok(Self {
            entries,
            sockets,
            log,
        })
    }
    fn entry_metadata(entry: &Value) -> Value {
        let mut template = entry.clone();
        for field in ["postData", "content"] {
            let owner = if field == "postData" {
                "request"
            } else {
                "response"
            };
            // `get_mut` never inserts a null field that a later import would refuse.
            if let Some(content) = template
                .get_mut(owner)
                .and_then(|owner| owner.get_mut(field))
                .and_then(Value::as_object_mut)
            {
                for key in ["text", "_file", "encoding"] {
                    content.remove(key);
                }
            }
        }
        template
    }

    /// Exact URL/method, POST bytes (with multipart boundary normalization),
    /// then greatest matching-header count. Stable ties preserve archive order.
    pub(crate) fn find(&self, request: &Request) -> Result<Option<Found<'_>>, Error> {
        let mut current = request.url.clone();
        let mut method = request.method.as_str();
        let mut visited = BTreeSet::new();
        loop {
            let mut best = None;
            let mut best_score = 0;
            let mut unavailable_body = false;
            for (index, entry) in self.entries.iter().enumerate() {
                let candidate = &entry.request;
                if candidate.url != current || candidate.method != method {
                    continue;
                }
                if method == "POST" && entry.unknown_post_data {
                    unavailable_body = true;
                    continue;
                }
                if method == "POST"
                    && let (Some(actual), Some(expected)) = (&request.body, &candidate.body)
                    && !post_matches(actual, expected, &request.headers, &candidate.headers)
                {
                    continue;
                }
                let score = candidate
                    .headers
                    .iter()
                    .filter(|candidate| {
                        request.headers.iter().any(|h| {
                            h.name.eq_ignore_ascii_case(&candidate.name)
                                && h.value == candidate.value
                        })
                    })
                    .count();
                if best.is_none() || score > best_score {
                    best = Some((index, entry));
                    best_score = score;
                }
            }
            let Some((index, entry)) = best else {
                return if unavailable_body {
                    Err("HAR_BODY_UNAVAILABLE".into())
                } else {
                    Ok(None)
                };
            };
            if !visited.insert(index) {
                return Err("HAR_REDIRECT_CYCLE".into());
            }
            if visited.len() > 64 {
                return Err("HAR_REDIRECT_BOUNDS".into());
            }
            let status = entry.response.status;
            if matches!(status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = entry
                    .response
                    .headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case("location"))
            {
                let next = url::Url::parse(&current)
                    .and_then(|base| base.join(&location.value))
                    .map_err(|_| Error::from("HAR_INVALID_REDIRECT"))?;
                current = url(&json!(next.as_str()))?;
                if matches!(status, 301 | 302) && method == "POST"
                    || status == 303 && !matches!(method, "GET" | "HEAD")
                {
                    method = "GET";
                }
                continue;
            }
            if entry.response.body.is_none() {
                return Err("HAR_BODY_UNAVAILABLE".into());
            }
            if !(200..=599).contains(&status) {
                return Err("HAR_RESPONSE_UNAVAILABLE".into());
            }
            return Ok(Some(Found {
                response: &entry.response,
                url: &entry.request.url,
                redirects: visited.len() - 1,
            }));
        }
    }
}
fn boundary(headers: &[Header]) -> Option<&str> {
    let value = &headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))?
        .value;
    if !value.contains("multipart/form-data") {
        return None;
    }
    let value = value.split_once("boundary=")?.1;
    value.split_whitespace().next()
}
fn post_matches(actual: &[u8], expected: &[u8], headers: &[Header], candidate: &[Header]) -> bool {
    if actual == expected {
        return true;
    }
    let (Some(actual_boundary), Some(expected_boundary)) = (boundary(headers), boundary(candidate))
    else {
        return false;
    };
    let (Ok(actual), Ok(expected)) = (std::str::from_utf8(actual), std::str::from_utf8(expected))
    else {
        return false;
    };
    actual.replace(actual_boundary, "") == expected.replace(expected_boundary, "")
}

#[cfg(test)]
mod tests;
