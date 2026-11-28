//! Isolated HTTP clients, bounded cookies and retained host-side request work.
use super::{contract::Reference, session::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{task::JoinHandle, time::Instant};
use ureq::{AsSendBody, RequestExt};
use url::Url;
use uuid::Uuid;

const MAX_BODY: usize = 1048576;
pub(crate) mod browser;
pub use browser::CookiePartition;
mod lanes;
use lanes::{Activity, Lease, TransportLane};
mod lifetime;
pub use lifetime::DisposeOptions;
pub(crate) use lifetime::schemas as lifetime_schemas;
mod payload;
pub use payload::{FormField, Payload};
mod parameters;
pub use parameters::{Parameters, QueryParameter};
mod proxy;
pub use proxy::Input as Proxy;
pub(crate) use proxy::bypasses as proxy_bypasses;
pub(crate) use proxy::trim as trim_proxy;
pub(crate) mod cookie_admin;
pub(crate) mod cookie_filter;
pub use cookie_filter::CookieFilter;
mod cookies;
pub use cookie_admin::CookieUrls;
mod credential_defaults;
use cookies::Cookies;
pub(crate) mod client_certificates;
mod credentials;
pub(crate) mod request_inputs;
pub use client_certificates::ClientCertificate;
mod defaults;
pub(crate) use credentials::Credential;
pub use credentials::Credentials;
mod connection_details;
mod details;
mod ecmascript_json;
mod files;
mod header_defaults;
mod mime;
mod multipart;
mod request_wire;
mod response;
mod storage_state;
pub(crate) use storage_state::copy_to_path as storage_state_copy;
pub(crate) use storage_state::load as storage_state_load;
pub(crate) use storage_state::schemas as storage_state_schemas;
pub(crate) use storage_state::write_owner_only as storage_state_write;
pub use storage_state::{Input as StorageStateInput, Options as StorageStateOptions};
pub(crate) async fn prepare_options(options: Options, deadline: Instant) -> Result<Options, Error> {
    let options = storage_state::prepare(options, deadline).await?;
    client_certificates::prepare_options(options, deadline).await
}
mod wire;
#[cfg(test)]
pub(crate) use files::hold_readers;
pub(crate) use files::prepare;
pub use multipart::Part as MultipartPart;
pub use response::BodyFormat;
pub(crate) use response::read as read_response;
pub(crate) const OPERATIONS: [&str; 24] = [
    "http_context_create",
    "http_context_list",
    "http_context_remove",
    "http_context_cookies_clear",
    "http_context_cookies_add",
    "http_context_cookies_read",
    "http_fetch",
    "route_fetch",
    "http_response_finalize",
    "http_response_discard",
    "http_response_read",
    "http_response_remove",
    "http_evidence_list",
    "http_evidence_read",
    "http_evidence_remove",
    "http_context_storage_state",
    "http_evidence_finalize",
    "http_context_dispose",
    "http_context_read",
    "http_response_body_read",
    "http_response_dispose",
    "http_request_inputs_set",
    "http_fetch_request",
    "http_request_input_remove",
];
pub(crate) fn capabilities(available: bool) -> Value {
    let mut value = json!({"available":available,"operations":OPERATIONS,"backend":"rust-host-http","browserAssociated":available,"cookies":["isolated-host-jar","browser-context-exchange"],"associatedSettings":"explicit-http-options","partitionedCookies":false,"cors":"host-http-no-browser-cors","maxContexts":16,"maxPendingResponses":32,"maxBodyBytes":MAX_BODY,"requestBodies":["text","base64","json","form","multipart"],"multipartFiles":["base64-parts","selected-regular-file-paths"],"multipartMimeTypes":"explicit-or-pinned-filename-inference","maxConcurrentFileReaders":4,"fileSnapshot":"before-http-dispatch","fileStreams":"bounded-regular-files-only","queryParameters":["encoded-replacement","ordered-appended-pairs"],"requestOptionOverrides":["timeoutMs","maxRedirects","failOnStatusCode","ignoreHttpsErrors"],"explicitRetries":{"option":"maxRetries","default":0,"maximum":20,"on":"connection-reset","replayReported":true},"maxQueryPairs":128,"maxUrlBytes":4096,"maxFormFields":128,"maxResponseHeaderBytes":16384,"maxCookiesPerContext":256,"maxCookieHeaderBytesPerContext":65536,"cookieEviction":"oldest-update-first","responseWitnesses":128,"automaticRetries":false,"redirectHeaders":"cookie-reselected-and-host-replaced-each-hop-authorization-stripped-cross-origin-other-explicit-headers-preserved","bodyFormat":"http-entity-body","contentEncoding":"decoded-when-supported","decodedContentEncodings":["gzip","x-gzip","deflate","br"],"maxEncodedBodyBytes":MAX_BODY,"tlsRoots":"rustls-webpki","clientCertificates":false});
    value["responseDetails"] = json!({"read":"http_response_read","remove":"http_response_remove","retainedResponses":128,"headerNames":"original-when-wire-observed","duplicateValues":true,"wireOrder":true,"wireStatusText":true,"wireMetadataFailure":"explicit-parsed-fallback","maxWireHeadBytes":16384,"bodyIndependent":true,"maxResultBytes":65536,"tlsSecurityDetails":true,"serverAddress":true,"resourceTiming":true,"maxSecurityDetailsBytes":16384,"timingEnd":"body-read-completion-or-unavailable","proxyTiming":"destination-observable-phases","syntheticProxyRefusal":"no-peer-security-or-connection-phases","fullSdkParity":false});
    value["authentication"] = json!({"scheme":"Basic","option":"httpCredentials","selection":"first-matching-lowercased-origin","defaultSend":"unauthorized","challengeRetries":"once-per-hop-without-nonempty-authorization","always":"initial-request-overwrites-authorization","replayReported":true,"maxEntries":32,"maxFieldBytes":4096,"maxTotalBytes":65536});
    value["clientCertificates"] = json!({"option":"clientCertificates","formats":["base64-encoded-PEM-chain-and-key","base64-encoded-PKCS12"],"selection":"normalized-exact-origin-per-hop","browserInheritance":available,"explicitEmptyOverride":true,"httpsProxyIdentityIsolation":true,"maxEntries":16,"maxDecodedBytes":1048576,"maxIdentitiesPerOrigin":16,"sameKeyType":"last-entry-with-pfx-applied-after-pem","authorityHints":"ignored-as-pinned","signatureSelection":"server-supported-signature-algorithm","callerPaths":true,"callerPathBase":"frontend-working-directory","fileSnapshot":"immutable-inline-before-allocation","fileSymlinks":"follow-selected-regular-files","inlinePrecedence":"present-including-empty","fileMaxPathBytes":4096,"sdkErrorStaging":false,"pkcs12":true,"encryptedKeys":true,"maxPassphraseBytes":4096,"fullSdkParity":false});
    value["storageState"] = json!({"initialization":"storageState","sources":["inline","caller-relative-regular-file"],"export":"http_context_storage_state","finalize":"http_evidence_finalize","browserAssociated":false,"browserAssociatedInitialization":false,"fullSdkParity":false,"maxBytes":super::storage::MAX_BYTES,"indexedDB":"opaque-isolated-records-or-live-associated-context-opt-in-export","opfs":"opaque-isolated-records-or-live-associated-context-opt-in-export","privacy":"private-evidence-with-explicit-read","pendingPublication":"retained-immutable-bytes-without-source-read-or-http-replay","callerExportPath":{"option":"path","base":"caller-working-directory","parents":"created-owner-only","existing":"replaced","mode":"owner-only"}});
    value["proxy"] = json!({"inputs":["url","server-bypass-username-password"],"protocols":["http","https","socks","socks4","socks4a","socks5","socks5h"],"transport":"http-connect-or-socks-handshake","authentication":"private-connect-basic-or-socks-uri-percent-decoded-utf8","socksObjectCredentials":"ignored-as-pinned","socksDestinationNames":"proxy-resolved","maxSocksCredentialBytes":255,"maxSocksDestinationBytes":255,"bypass":"pinned-case-sensitive-hostname-suffix-per-destination","httpsProxyVerification":"always-enabled-independent-of-endpoint-policy","idleTunnelReuse":false,"browserInheritance":available,"inheritedProtocols":["http","https","socks4","socks5"],"browserCreationSocks":available,"browserCreationSocksProtocols":["socks4","socks5"],"webkitBrowserSocks4":"pinned-http-fallback-without-socks-handshake","maxFieldBytes":4096,"fullSdkParity":false});
    value["headerDefaults"] = json!({"baseUrlRequired":false,"scope":"all-request-origins","requestOverride":true,"inheritedSnapshot":"per-fetch-admission"});
    value["requestStringBodies"] = json!({"jsonSelection":"first-caller-content-type-exact-application/json","contextDefaultsSelectJson":false,"validJson":"preserved-text","invalidJson":"quoted-json-string","binary":"unchanged","rawDefaultContentType":"application/octet-stream","explicitEmptyContentType":"preserved","maxBytes":MAX_BODY,"fullSdkParity":false});
    value["zeroRequestTimeout"] = json!("outer-operation-deadline-only");
    value["publicLifetime"] = json!({"contextDispose":"http_context_dispose","contextRead":"http_context_read","isolatedInflight":"cancel-owned","associatedInflight":"continue-dispatched","parentClose":"cancel-owned","reasonRead":"explicit-includeReason","maxReasonBytes":65536,"disposedContexts":"retained-until-idle-removal","responseBodyRead":"http_response_body_read","responseDispose":"http_response_dispose","metadataAndEvidenceIndependent":true,"disconnect":"actual-worker-and-durable-caller-settlement","fullSdkParity":false});
    value["requestOwnership"] = json!({"sameContextConcurrency":true,"ordinaryOperationCapacity":true,"maxPendingResponsesShared":32,"wireState":"per-active-request","idleTransportLanesPerContext":1,"activeCount":"http_context_list.activeRequests","responsiveRunningPublication":true,"cancellation":"exact-request-sockets-and-worker-lease","lateCancellation":"cannot-close-reused-lane","fullSdkParity":false});
    value["requestInputs"] = json!({"enable":"http_request_inputs_set","fetch":"http_fetch_request","release":"http_request_input_remove","defaultEnabled":true,"requiresNetworkDiagnostics":true,"captureLifecycle":"diagnostics-start-admission-with-persistent-explicit-opt-out","sourceId":"diagnostic-requestId","scope":"connection","retention":"until-explicit-release-or-connection-cleanup","sourcePageClose":"retained","sourceContextClose":"retained","releaseInflight":"owned-copy-continues","maxInputs":512,"maxRetainedBytes":33554432,"maxBodyBytes":MAX_BODY,"bodySource":"binary-initializer-or-caller-route-override","headers":"cumulative-caller-intent-with-pinned-object-projection","privacy":"private-memory-no-payload-in-diagnostics","fallbackOverrides":true,"continueOverrides":true,"routeOverrideFields":["url","method","headers","bodyBase64"],"routeInputNotReady":"callback-retained-for-wait-and-retry","routeUpdateBounds":"future-source-reads-refuse-without-other-input-eviction","fullSdkParity":false});
    value["cookieAdministration"] = json!({"add":"http_context_cookies_add","read":"http_context_cookies_read","maxInputCookies":256,"maxInputBytes":65536,"maxReadUrls":128,"maxReadBytes":65536,"urlReadMatching":"pinned-domain-suffix-path-prefix-secure-localhost","isolatedCapacityFailure":"atomic-refusal","filteredClear":true,"clearCriteria":["exact-string","ecmascript-regexp"],"filterGrammar":"ecmascript-unicode-16.0","maxRetainedNameGroups":256,"maxRegexPatternBytes":2048,"maxRegexCapturingGroups":254,"maxFilterCookies":4096,"maxFilterInputBytes":1048576,"maxFilterWorkers":4,"maxFilterNativeHeapBytes":33554432,"filterCancellation":"context-lease-until-worker-exits","fullSdkParity":false});
    value["responseDetails"]["headersObject"] = json!({"option":"includeHeadersObject","default":false,"names":"lowercase","duplicateJoin":", ","setCookieJoin":"newline","ownKeyOrder":"headerObjectKeys","protoSetter":"omitted-as-pinned"});
    value["responseBodyReads"] = json!({"formats":["base64","text","json","json_value"],"textDecoding":"node-utf8-replacement","jsonNumbers":"ieee754","jsonValue":"flat-utf16-ordered-graph","maxResultBytes":65536,"fullSdkParity":false});
    value
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Header {
    pub name: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default)]
    pub browser_context: Option<Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_partition: Option<CookiePartition>,
    #[serde(default)]
    pub allow_unknown_cookie_partitions: bool,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub headers: Option<Vec<Header>>,
    #[serde(skip)]
    pub(crate) inherited_headers: Option<header_defaults::Shared>,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    #[serde(default = "redirects")]
    pub max_redirects: u32,
    #[serde(default)]
    pub proxy: Option<Proxy>,
    #[serde(default)]
    pub ignore_https_errors: Option<bool>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub http_credentials: Option<Credentials>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_certificates: Option<Vec<ClientCertificate>>,
    #[serde(skip)]
    pub(crate) prepared_client_certificates: Option<client_certificates::Cached>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_state: Option<StorageStateInput>,
    #[serde(skip)]
    pub(crate) inherited_credentials: Option<credential_defaults::Shared>,
    #[serde(default)]
    pub fail_on_status_code: bool,
}
fn timeout() -> u64 {
    30000
}
fn redirects() -> u32 {
    20
}
pub(crate) fn response_read_limit() -> usize {
    65536
}
impl Default for Options {
    fn default() -> Self {
        Self {
            browser_context: None,
            cookie_partition: None,
            allow_unknown_cookie_partitions: false,
            base_url: None,
            headers: None,
            inherited_headers: None,
            timeout_ms: timeout(),
            max_redirects: redirects(),
            proxy: None,
            ignore_https_errors: None,
            user_agent: None,
            http_credentials: None,
            client_certificates: None,
            prepared_client_certificates: None,
            storage_state: None,
            inherited_credentials: None,
            fail_on_status_code: false,
        }
    }
}
impl Options {
    pub(crate) fn validate_browser_partition(
        &mut self,
        model: browser::PartitionModel,
        available: bool,
    ) -> Result<(), Error> {
        if let Some(partition) = &mut self.cookie_partition {
            match model {
                browser::PartitionModel::Unsupported => {
                    return Err("HTTP_COOKIE_PARTITION_UNSUPPORTED".into());
                }
                browser::PartitionModel::Site if partition.has_cross_site_ancestor.is_some() => {
                    return Err("HTTP_COOKIE_PARTITION_ANCESTRY_UNSUPPORTED".into());
                }
                browser::PartitionModel::Site => {}
                browser::PartitionModel::SiteAndAncestor => {
                    partition.has_cross_site_ancestor.get_or_insert(true);
                }
            }
        }
        if available || self.allow_unknown_cookie_partitions {
            return Ok(());
        }
        let mut error = Error::from("HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE");
        error.partial = Some(
            json!({"browserContext":self.browser_context,"created":false,"cookiePartitionIdentityAvailable":false,"requiredOption":"allowUnknownCookiePartitions"}),
        );
        Err(error)
    }
    pub(crate) fn inherit(&mut self, context: &super::contract::ContextOptions) {
        if self.client_certificates.is_none() {
            self.client_certificates
                .clone_from(&context.client_certificates);
        }
        if self.proxy.is_none() {
            self.proxy = context.proxy.clone().map(Proxy::from);
        }
        if self.http_credentials.is_none() {
            self.http_credentials.clone_from(&context.http_credentials);
        }
        if self.ignore_https_errors.is_none() {
            self.ignore_https_errors = context.ignore_https_errors;
        }
        if self.user_agent.as_ref().is_none_or(String::is_empty) {
            self.user_agent.clone_from(&context.user_agent);
        }
        if self.base_url.is_none() {
            self.base_url.clone_from(&context.base_url);
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if let Some(certificates) = &self.client_certificates {
            client_certificates::validate(certificates)?;
        }
        if let Some(state) = &self.storage_state {
            if self.browser_context.is_some() {
                return Err("HTTP_BROWSER_STORAGE_STATE_UNAVAILABLE".into());
            }
            state.validate()?;
        }
        if let Some(credentials) = self.effective_credentials()? {
            credentials.validate()?;
        }
        if let Some(agent) = &self.user_agent
            && !agent.is_empty()
        {
            validate_user_agent(agent)?;
        }
        if self.allow_unknown_cookie_partitions && self.browser_context.is_none() {
            return Err("HTTP_BROWSER_CONTEXT_REQUIRED".into());
        }
        if let Some(partition) = &self.cookie_partition {
            partition.validate()?;
            if self.browser_context.is_none() {
                return Err("HTTP_COOKIE_PARTITION_CONTEXT_REQUIRED".into());
            }
        }
        let headers = self.effective_headers()?;
        validate_headers(&headers)?;
        validate_request_header_values(&headers)?;
        if self.timeout_ms > 120000 || self.max_redirects > 20 {
            return Err("HTTP_OPTIONS_BOUNDS".into());
        }
        if let Some(base) = &self.base_url {
            absolute_url(base)?;
        }
        if let Some(proxy) = &self.proxy {
            proxy.prepare()?;
        }
        Ok(())
    }
    fn effective_headers(&self) -> Result<Vec<Header>, Error> {
        if let Some(headers) = &self.headers {
            return Ok(headers.clone());
        }
        match &self.inherited_headers {
            Some(headers) => headers
                .lock()
                .expect("Context HTTP headers")
                .clone()
                .ok_or_else(|| "HTTP_CONTEXT_HEADERS_UNCERTAIN".into()),
            None => Ok(vec![]),
        }
    }
}
pub(crate) fn validate_user_agent(agent: &str) -> Result<(), Error> {
    if agent.is_empty() || agent.len() > 2048 {
        return Err("HTTP_USER_AGENT_BOUNDS".into());
    }
    request_wire::header(agent)?;
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fetch {
    pub url: String,
    #[serde(default)]
    pub params: Option<Parameters>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_redirects: Option<u32>,
    #[serde(default)]
    pub fail_on_status_code: Option<bool>,
    #[serde(default)]
    pub ignore_https_errors: Option<bool>,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default = "method")]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub body: Option<Payload>,
    #[serde(default)]
    pub read_body: bool,
    #[serde(default = "body_limit")]
    pub max_bytes: usize,
}
fn method() -> String {
    "GET".into()
}
fn body_limit() -> usize {
    65536
}
impl Fetch {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.url.len() > 4096 || !(1..=MAX_BODY).contains(&self.max_bytes) {
            return Err("HTTP_REQUEST_BOUNDS".into());
        }
        if self.method.is_empty()
            || self.method.len() > 32
            || self.method.parse::<ureq::http::Method>().is_err()
        {
            return Err("HTTP_METHOD_INVALID".into());
        }
        validate_headers(&self.headers)?;
        validate_request_header_values(&self.headers)?;
        if self.timeout_ms.is_some_and(|value| value > 120000)
            || self.max_redirects.is_some_and(|value| value > 20)
            || self.max_retries > 20
        {
            return Err("HTTP_OPTIONS_BOUNDS".into());
        }
        if let Some(params) = &self.params {
            params.validate()?;
        }
        if let Some(body) = &self.body {
            body.validate()?;
        }
        Ok(())
    }
}
/// `route_fetch` options besides its callback and HTTP client.
#[derive(Default)]
pub(crate) struct RouteFetchOptions {
    pub(crate) max_redirects: Option<u32>,
    pub(crate) max_retries: Option<u32>,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) overrides: Option<super::routing::Overrides>,
}
impl RouteFetchOptions {
    /// The options of a `route_fetch` request.
    pub(crate) fn of(request: &super::contract::Request) -> Option<Self> {
        let super::contract::Request::RouteFetch {
            max_redirects,
            max_retries,
            timeout_ms,
            overrides,
            ..
        } = request
        else {
            return None;
        };
        Some(Self {
            max_redirects: *max_redirects,
            max_retries: *max_retries,
            timeout_ms: *timeout_ms,
            overrides: overrides.clone(),
        })
    }
    /// Submission bounds, checked before the callback is touched.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.max_redirects.is_some_and(|redirects| redirects > 20)
            || self.max_retries.is_some_and(|retries| retries > 20)
            || self.timeout_ms.is_some_and(|timeout| timeout > 120_000)
        {
            return Err("HTTP_OPTIONS_BOUNDS".into());
        }
        if let Some(overrides) = &self.overrides {
            overrides.validate()?;
        }
        Ok(())
    }
}
/// Pinned `route.fetch()`: a paused request, with optional overrides, as a
/// request for the caller's HTTP client. Overrides replace the URL (any HTTP(S)
/// URL), method, whole header list or body for this fetch only. The client's
/// cookie jar supplies cookies, so `Cookie` and transport-owned headers are not
/// copied from either source; redirects, retries and timeout are explicit.
pub(crate) fn route_fetch_request(
    request: &super::har::Request,
    options: &RouteFetchOptions,
) -> Result<Fetch, Error> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    const TRANSPORT: [&str; 9] = [
        "cookie",
        "host",
        "content-length",
        "connection",
        "keep-alive",
        "transfer-encoding",
        "upgrade",
        "proxy-connection",
        "te",
    ];
    options.validate()?;
    let overrides = options.overrides.clone().unwrap_or_default();
    let kept = |name: &str| {
        !name.starts_with(':') && !TRANSPORT.iter().any(|kept| name.eq_ignore_ascii_case(kept))
    };
    let headers = match &overrides.headers {
        Some(headers) => headers
            .iter()
            .filter(|header| kept(&header.name))
            .map(|header| json!({"name":header.name,"value":header.value}))
            .collect::<Vec<_>>(),
        None => request
            .headers
            .iter()
            .filter(|header| kept(&header.name))
            .map(|header| json!({"name":header.name,"value":header.value}))
            .collect(),
    };
    let body = match &overrides.body_base64 {
        Some(body) => Some(json!({"kind":"base64","data":body})),
        None => request
            .body
            .as_ref()
            .map(|bytes| json!({"kind":"base64","data":STANDARD.encode(bytes)})),
    };
    let url = overrides.url.as_ref().unwrap_or(&request.url);
    let method = overrides.method.as_ref().unwrap_or(&request.method);
    let fetch: Fetch = serde_json::from_value(json!({"url":url,"method":method,"headers":headers,"body":body,"maxRedirects":options.max_redirects,"maxRetries":options.max_retries.unwrap_or(0),"timeoutMs":options.timeout_ms,"readBody":true,"maxBytes":MAX_BODY}))
        .map_err(|_| Error::from("ROUTE_FETCH_REQUEST_INVALID"))?;
    fetch.validate()?;
    Ok(fetch)
}
/// An HTTP evidence item's body and status for `route_fulfill_http_evidence`.
pub(crate) fn fulfill_source(
    item: &super::artifacts::Artifact,
    status: Option<u16>,
) -> Result<(Vec<u8>, u16), Error> {
    let status = match status {
        Some(status) => status,
        None => item.summary["response"]["status"]
            .as_u64()
            .and_then(|status| u16::try_from(status).ok())
            .ok_or("ROUTE_STATUS_UNAVAILABLE")?,
    };
    Ok((evidence_body(item)?, status))
}
/// The decoded entity body of an HTTP response evidence item, for route fulfillment.
pub(crate) fn evidence_body(item: &super::artifacts::Artifact) -> Result<Vec<u8>, Error> {
    if item.summary["format"] != "http-entity-body"
        || item.summary["attachment"]["bytes"]
            .as_u64()
            .is_none_or(|size| size > MAX_BODY as u64)
    {
        return Err("BODY_EVIDENCE_FORMAT".into());
    }
    item.read()
}
pub(crate) fn validate_headers(headers: &[Header]) -> Result<(), Error> {
    if headers.len() > 64
        || headers
            .iter()
            .map(|h| h.name.len() + h.value.len())
            .sum::<usize>()
            > 16384
    {
        return Err("HTTP_HEADERS_BOUNDS".into());
    }
    for header in headers {
        if header.name.len() > 1024 {
            return Err("HTTP_HEADERS_BOUNDS".into());
        }
        header
            .name
            .parse::<ureq::http::HeaderName>()
            .map_err(|_| Error::from("HTTP_HEADER_INVALID"))?;
        header
            .value
            .parse::<ureq::http::HeaderValue>()
            .map_err(|_| Error::from("HTTP_HEADER_INVALID"))?;
    }
    Ok(())
}
fn validate_request_header_values(headers: &[Header]) -> Result<(), Error> {
    for header in headers {
        request_wire::header(&header.value)?;
    }
    Ok(())
}
fn absolute_url(value: &str) -> Result<Url, Error> {
    request_wire::absolute(value)
}
struct Prepared {
    body: Vec<u8>,
    content_type: Option<String>,
    credentials: Option<Credentials>,
}
struct Client {
    reference: Reference,
    options: Options,
    storage_origins: Vec<Value>,
    base: Option<Url>,
    lanes: Mutex<Vec<Arc<TransportLane>>>,
    prepared_certificates: Vec<client_certificates::Prepared>,
    proxy: Option<proxy::Prepared>,
    cookies: Mutex<Cookies>,
    browser: Option<Arc<dyn browser::BrowserCookies>>,
    activity: Mutex<Activity>,
}
pub(crate) struct Reply {
    pub(crate) metadata: Value,
    pub(crate) body: Option<Vec<u8>>,
    details: Option<Value>,
}
struct Job {
    client: Reference,
    disposed: Arc<AtomicBool>,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    wire: wire::Observer,
    task: JoinHandle<()>,
    result: Arc<Mutex<Option<Result<Reply, Error>>>>,
    finalizing: bool,
}
pub(crate) enum Ready {
    Reply(Reply),
    Complete(Value),
}
#[derive(Default)]
pub(crate) struct Contexts {
    items: BTreeMap<Reference, Arc<Client>>,
    connection: Option<Uuid>,
    jobs: BTreeMap<Uuid, Job>,
    evidence: super::artifacts::Store,
    witnesses: BTreeMap<Uuid, Value>,
    order: VecDeque<Uuid>,
    captures: BTreeMap<Uuid, Uuid>,
    body_lifetimes: BTreeMap<Uuid, lifetime::BodyLifetime>,
    failures: BTreeMap<Uuid, (&'static str, Option<Value>)>,
    details: details::Details,
    browser_headers: BTreeMap<Reference, header_defaults::Shared>,
    browser_credentials: BTreeMap<Reference, credential_defaults::Shared>,
}
impl Contexts {
    #[cfg(test)]
    pub(crate) fn create(&mut self, connection: Uuid, options: Options) -> Result<Value, Error> {
        self.create_with_browser(connection, options, None)
    }
    pub(crate) fn create_with_browser(
        &mut self,
        connection: Uuid,
        mut options: Options,
        browser: Option<Arc<dyn browser::BrowserCookies>>,
    ) -> Result<Value, Error> {
        options.validate()?;
        options.user_agent = options.user_agent.filter(|agent| !agent.is_empty());
        if options
            .browser_context
            .is_some_and(|context| context.connection_id != connection)
        {
            return Err("FOREIGN_REFERENCE".into());
        }
        if options.browser_context.is_some() != browser.is_some() {
            return Err("HTTP_BROWSER_CONTEXT_REQUIRED".into());
        }
        let associated = options.browser_context;
        let partition_identity_available = browser
            .as_ref()
            .map(|browser| browser.partition_identity_available());
        if let Some(browser) = &browser {
            options.validate_browser_partition(
                browser.partition_model(),
                browser.partition_identity_available(),
            )?;
        }
        let unknown_partitions_allowed = options.allow_unknown_cookie_partitions;
        let partition_configured = options.cookie_partition.is_some();
        let partition_ancestor = options
            .cookie_partition
            .as_ref()
            .and_then(|partition| partition.has_cross_site_ancestor);
        if self.items.len() >= 16 {
            return Err("HTTP_CONTEXT_CAPACITY".into());
        }
        if self.connection.is_some_and(|owner| owner != connection) {
            return Err("FOREIGN_REFERENCE".into());
        }
        let base = options.base_url.as_deref().map(absolute_url).transpose()?;
        let (cookies, storage_origins) = storage_state::initialize(options.storage_state.take())?;
        let proxy = options.proxy.as_ref().map(Proxy::prepare).transpose()?;
        let prepared_certificates = client_certificates::take_prepared(&mut options)?;
        let lane = Arc::new(TransportLane::new(
            &options,
            proxy.as_ref(),
            &prepared_certificates,
        ));
        let reference = Reference {
            connection_id: connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        self.items.insert(
            reference,
            Arc::new(Client {
                reference,
                options,
                base,
                lanes: Mutex::new(vec![lane]),
                prepared_certificates,
                proxy,
                cookies: Mutex::new(cookies),
                storage_origins,
                browser,
                activity: Mutex::new(Activity::default()),
            }),
        );
        self.connection = Some(connection);
        Ok(
            json!({"httpContext":reference,"created":true,"cookies":if associated.is_some(){"browser-context-exchange"}else{"isolated-host-jar"},"cors":"host-http-no-browser-cors","browserAssociated":associated.is_some(),"browserContext":associated,"cookiePartitionConfigured":partition_configured,"cookiePartitionCrossSiteAncestor":partition_ancestor,"cookiePartitionIdentityAvailable":partition_identity_available,"unknownCookiePartitionsAllowed":unknown_partitions_allowed}),
        )
    }
    fn get(&self, reference: Reference) -> Result<Arc<Client>, Error> {
        let client = self.lookup(reference)?;
        client.require_open(&client.activity.lock().expect("HTTP activity"))?;
        Ok(client)
    }
    fn lookup(&self, reference: Reference) -> Result<Arc<Client>, Error> {
        if self.connection != Some(reference.connection_id) {
            return Err("FOREIGN_REFERENCE".into());
        }
        self.items
            .get(&reference)
            .cloned()
            .ok_or("STALE_REFERENCE".into())
    }
    pub(crate) fn list(&self) -> Value {
        json!({"contexts":self.items.values().map(|client|{let mut cookies=client.cookies.lock().expect("HTTP cookies");cookies.prune();let associated=client.options.browser_context;json!({"httpContext":client.reference,"baseUrl":client.base.as_ref().map(|url|super::diagnostics::url(url.as_str())),"active":client.is_active(),"disposed":client.activity.lock().expect("HTTP activity").disposed.is_some(),"activeRequests":client.request_count(),"cookies":associated.is_none().then_some(cookies.keys.len()),"cookieBytes":associated.is_none().then_some(cookies.bytes),"evictedCookies":cookies.evicted,"droppedCookies":cookies.dropped,"proxyConfigured":client.options.proxy.is_some(),"tlsVerification":!client.options.ignore_https_errors.unwrap_or(false),"userAgentConfigured":client.options.user_agent.is_some(),"browserAssociated":associated.is_some(),"browserContext":associated,"cookiePartitionConfigured":client.options.cookie_partition.is_some(),"cookiePartitionCrossSiteAncestor":client.options.cookie_partition.as_ref().and_then(|partition|partition.has_cross_site_ancestor),"cookiePartitionIdentityAvailable":client.browser.as_ref().map(|browser|browser.partition_identity_available()),"unknownCookiePartitionsAllowed":client.options.allow_unknown_cookie_partitions})}).collect::<Vec<_>>()})
    }
    pub(crate) fn remove(&mut self, reference: Reference) -> Result<Value, Error> {
        let client = self.lookup(reference)?;
        if client.is_active() {
            return Err("HTTP_CONTEXT_BUSY".into());
        }
        self.items.remove(&reference);
        self.dispose_bodies(reference);
        Ok(json!({"httpContext":reference,"removed":true}))
    }
    pub(crate) fn associated_context(
        &self,
        reference: Reference,
    ) -> Result<Option<Reference>, Error> {
        Ok(self.get(reference)?.options.browser_context)
    }
    pub(crate) fn require_idle(&self, reference: Reference) -> Result<(), Error> {
        if self.get(reference)?.is_active() {
            Err("HTTP_CONTEXT_BUSY".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn clear_cookies(&self, reference: Reference) -> Result<Value, Error> {
        let client = self.get(reference)?;
        if client.browser.is_some() {
            return Err("HTTP_BROWSER_COOKIE_CONTEXT_REQUIRED".into());
        }
        if client.is_active() {
            return Err("HTTP_CONTEXT_BUSY".into());
        }
        *client.cookies.lock().expect("HTTP cookies") = Cookies::default();
        Ok(json!({"httpContext":reference,"cookiesCleared":true}))
    }
    pub(crate) fn pending(&self) -> Vec<Uuid> {
        self.jobs.keys().copied().collect()
    }
    pub(crate) fn has_pending(&self, id: Uuid) -> bool {
        self.jobs.contains_key(&id)
    }
    pub(crate) fn close(&mut self) -> Result<(), Error> {
        if self.evidence.has_pending()
            || !self.jobs.is_empty()
            || self.items.values().any(|client| client.is_active())
        {
            return Err("CLEANUP_PENDING".into());
        }
        self.items.clear();
        self.browser_headers.clear();
        self.browser_credentials.clear();
        Ok(())
    }
    pub(crate) fn start(
        &mut self,
        reference: Reference,
        mut fetch: Fetch,
        deadline: Instant,
    ) -> Result<Uuid, Error> {
        fetch.validate()?;
        fetch.method.make_ascii_uppercase();
        let client = self.get(reference)?;
        if self.jobs.len() >= 32 {
            return Err("HTTP_RESPONSE_CAPACITY".into());
        }
        let mut url = if let Some(base) = &client.base {
            base.join(&fetch.url)
                .map_err(|_| Error::from("HTTP_URL_INVALID"))?
        } else {
            absolute_url(&fetch.url)?
        };
        if let Some(params) = &fetch.params {
            params.apply(&mut url)?;
        }
        let url = absolute_url(url.as_str())?;
        let json_string = payload::json_string_header(&fetch.headers);
        let headers = client.options.effective_headers()?;
        // Freeze one default snapshot before allocation or HTTP dispatch. A
        // later browser-header change affects only subsequent requests.
        fetch.headers = headers.into_iter().chain(fetch.headers).collect();
        validate_request_header_values(&fetch.headers)?;
        let credentials = client.options.effective_credentials()?;
        let (bytes, content_type) = fetch
            .body
            .as_ref()
            .map(|body| body.encode_for_request(json_string))
            .transpose()?
            .unwrap_or_default();
        if bytes.len() > MAX_BODY {
            return Err("HTTP_REQUEST_BODY_BOUNDS".into());
        }
        if Instant::now() >= deadline {
            return Err("HTTP_DEADLINE".into());
        }
        let lease = client.request_lease()?;
        let cancelled = lease.cancelled.clone();
        let wire = lease.transport().wire.clone();
        let id = Uuid::new_v4();
        if fetch.read_body {
            self.evidence.reserve(id, 1, MAX_BODY)?;
        }
        let timeout_ms = fetch.timeout_ms.unwrap_or(client.options.timeout_ms);
        let deadline = if timeout_ms == 0 {
            deadline
        } else {
            deadline.min(Instant::now() + Duration::from_millis(timeout_ms))
        };
        let result = Arc::new(Mutex::new(None));
        let destination = result.clone();
        let task = tokio::task::spawn_blocking(move || {
            let reply=client.run(id,fetch,url,Prepared{body:bytes,content_type,credentials},deadline,lease).map_err(|mut error|{if error.partial.is_none(){error.partial=Some(json!({"httpContext":reference,"requestId":id,"requestMayHaveBeenSent":true,"potentiallyMutating":true,"replayed":false}));}error});
            *destination.lock().expect("Retained HTTP response") = Some(reply);
        });
        self.jobs.insert(
            id,
            Job {
                client: reference,
                disposed: Arc::new(AtomicBool::new(false)),
                deadline,
                cancelled,
                wire,
                task,
                result,
                finalizing: false,
            },
        );
        Ok(id)
    }
    pub(crate) fn deadline(&self, id: Uuid) -> Option<Instant> {
        self.jobs.get(&id).map(|job| job.deadline)
    }
    pub(crate) fn cancel_request(&self, id: Uuid) {
        if let Some(job) = self.jobs.get(&id) {
            job.cancelled.store(true, Ordering::SeqCst);
            job.wire.cancel(&job.cancelled);
        }
    }
    pub(crate) fn ready(&mut self, id: Uuid) -> Result<Option<Ready>, Error> {
        if let Some((code, partial)) = self.failures.get(&id) {
            let mut error = Error::from(*code);
            error.partial = partial.clone();
            return Err(error);
        }
        if let Some(value) = self.witnesses.get(&id) {
            return Ok(Some(Ready::Complete(value.clone())));
        }
        if let Some(artifact) = self.captures.get(&id) {
            let artifact = self.evidence.get(*artifact)?;
            let mut value = artifact.summary["response"].clone();
            value["body"] = artifact.summary;
            return Ok(Some(Ready::Complete(value)));
        }
        let job = self.jobs.get_mut(&id).ok_or("UNKNOWN_HTTP_RESPONSE")?;
        if job.finalizing || !job.task.is_finished() {
            return Ok(None);
        }
        let result = job
            .result
            .lock()
            .expect("Retained HTTP response")
            .take()
            .unwrap_or_else(|| Err(unknown(job.client, id)));
        match result {
            Ok(mut reply) => {
                if let Some(details) = reply.details.take() {
                    self.details.insert(id, details);
                }
                job.finalizing = true;
                Ok(Some(Ready::Reply(reply)))
            }
            Err(error) => {
                self.jobs.remove(&id);
                self.evidence.release(id);
                self.retain_witness(id);
                self.failures
                    .insert(id, (error.code, error.partial.clone()));
                Err(error)
            }
        }
    }
    pub(crate) fn restore(&mut self, id: Uuid, reply: Reply) {
        if let Some(job) = self.jobs.get_mut(&id) {
            *job.result.lock().expect("Retained HTTP response") = Some(Ok(reply));
            job.finalizing = false;
        }
    }
    pub(crate) fn complete(
        &mut self,
        id: Uuid,
        mut metadata: Value,
        artifact: Option<super::artifacts::Artifact>,
    ) -> Result<Value, Error> {
        if let Some(value) = self.witnesses.get(&id) {
            return Ok(value.clone());
        }
        if let Some(artifact) = artifact {
            let reference = self.jobs.get(&id).ok_or("UNKNOWN_HTTP_RESPONSE")?.client;
            let evidence_id = artifact.id;
            metadata["body"] = self.evidence.insert_for(id, artifact)?;
            metadata["bodyIncluded"] = json!(true);
            self.captures.insert(id, evidence_id);
            self.retain_body_lifetime(id, reference);
        }
        self.evidence.release(id);
        self.jobs.remove(&id);
        self.retain_witness(id);
        self.witnesses.insert(id, metadata.clone());
        Ok(metadata)
    }
    fn retain_witness(&mut self, id: Uuid) {
        if self.order.len() == 128 {
            let oldest = self.order.pop_front().expect("HTTP witness retention");
            self.witnesses.remove(&oldest);
            self.failures.remove(&oldest);
        }
        self.order.push_back(id);
    }
    pub(crate) fn evidence_list(&self) -> Value {
        json!({"evidence":self.evidence.list(),"pending":self.jobs.iter().map(|(id,job)|json!({"requestId":id,"httpContext":job.client,"ready":job.task.is_finished(),"finalizing":job.finalizing,"bodyIncluded":false})).collect::<Vec<_>>(),"pendingArtifacts":self.evidence.pending_summary(),"capacity":self.evidence.capacity_summary()})
    }
    pub(crate) fn evidence_get(&self, id: Uuid) -> Result<super::artifacts::Artifact, Error> {
        self.evidence.get(id)
    }
    pub(crate) fn response_read(&self, id: Uuid, max_bytes: usize) -> Result<Value, Error> {
        self.details.read(id, max_bytes)
    }
    pub(crate) fn response_read_options(
        &self,
        id: Uuid,
        max_bytes: usize,
        include_headers_object: bool,
    ) -> Result<Value, Error> {
        let mut result = self.response_read(id, max_bytes)?;
        if include_headers_object {
            details::headers_object(&mut result)?;
            crate::browser::schema::encoded_len(&result, max_bytes)
                .map_err(|_| Error::from("HTTP_RESPONSE_RESULT_BOUNDS"))?;
        }
        Ok(result)
    }
    pub(crate) fn response_remove(&mut self, id: Uuid) -> Value {
        self.details.remove(id)
    }
    pub(crate) fn evidence_forget(&mut self, id: Uuid) {
        self.evidence.forget(id);
        let requests = self
            .captures
            .iter()
            .filter(|(_, evidence)| **evidence == id)
            .map(|(request, _)| *request)
            .collect::<Vec<_>>();
        for request in requests {
            self.captures.remove(&request);
            self.body_lifetimes.remove(&request);
            self.witnesses.remove(&request);
            self.order.retain(|id| *id != request);
        }
    }
    pub(crate) fn discard(&mut self, id: Uuid) -> Result<Value, Error> {
        let job = self.jobs.get(&id).ok_or("UNKNOWN_HTTP_RESPONSE")?;
        if job.finalizing || !job.task.is_finished() {
            return Err("HTTP_RESPONSE_PENDING".into());
        }
        let reference = job.client;
        self.details.remove(id);
        self.jobs.remove(&id);
        self.evidence.release(id);
        self.complete(
            id,
            json!({"httpContext":reference,"requestId":id,"discarded":true,"replayed":false}),
            None,
        )
    }
}
pub(crate) fn materialize(
    directory: &Path,
    reply: &Reply,
) -> Result<Option<super::artifacts::Artifact>, Error> {
    if reply.body.is_none() {
        return Ok(None);
    }
    let directory = private_evidence_directory(directory, "http")?;
    let mut metadata = reply.metadata.clone();
    metadata["bodyIncluded"] = json!(true);
    reply.body.as_ref().map(|bytes|super::artifacts::Format{prefix:"http",extension:"bin",mime:"application/octet-stream",name:"http-entity-body",max_bytes:MAX_BODY}.write(&directory,bytes,json!({"response":metadata,"privacy":"private-explicit-http-body","redacted":false,"complete":true}))).transpose()
}
fn private_evidence_directory(directory: &Path, name: &str) -> Result<PathBuf, Error> {
    let directory = directory.join(name);
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err("EVIDENCE_IO".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = crate::artifacts::directory(&directory)
                && !std::fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir())
            {
                return Err(Error::caused("EVIDENCE_IO", error));
            }
        }
        Err(error) => return Err(Error::caused("EVIDENCE_IO", error)),
    }
    crate::artifacts::check_private_directory(&directory)
        .map_err(|error| Error::caused("EVIDENCE_IO", error))?;
    Ok(directory)
}
impl Client {
    fn run(
        &self,
        id: Uuid,
        fetch: Fetch,
        url: Url,
        prepared: Prepared,
        deadline: Instant,
        lease: Lease,
    ) -> Result<Reply, Error> {
        let mut authentication_retries = 0u32;
        for attempt in 0..=fetch.max_retries {
            let mut result = self.run_once(
                id,
                &fetch,
                url.clone(),
                (prepared.body.clone(), prepared.content_type.clone()),
                deadline,
                &lease,
                prepared.credentials.as_ref(),
                &mut authentication_retries,
            );
            let retry = result.as_ref().err().is_some_and(|error| {
                error.code == "HTTP_REQUEST_OUTCOME_UNKNOWN"
                    && error
                        .partial
                        .as_ref()
                        .is_some_and(|partial| partial["connectionReset"] == true)
            }) && attempt < fetch.max_retries
                && !lease.cancelled.load(Ordering::SeqCst)
                && Instant::now() < deadline;
            if retry {
                continue;
            }
            match &mut result {
                Ok(reply) => {
                    reply.metadata["retryCount"] = json!(attempt);
                    reply.metadata["authenticationRetries"] = json!(authentication_retries);
                    reply.metadata["replayed"] = json!(attempt > 0 || authentication_retries > 0);
                    if let Some(details) = &mut reply.details {
                        details["retryCount"] = json!(attempt);
                        details["authenticationRetries"] = json!(authentication_retries);
                        details["replayed"] = json!(attempt > 0 || authentication_retries > 0);
                    }
                }
                Err(error) => {
                    let partial=error.partial.get_or_insert_with(||json!({"httpContext":self.reference,"requestId":id,"requestMayHaveBeenSent":true,"potentiallyMutating":true}));
                    partial["retryCount"] = json!(attempt);
                    partial["authenticationRetries"] = json!(authentication_retries);
                    partial["replayed"] = json!(attempt > 0 || authentication_retries > 0);
                    if let Some(response) = partial
                        .get_mut("response")
                        .filter(|value| value.is_object())
                    {
                        response["retryCount"] = json!(attempt);
                        response["authenticationRetries"] = json!(authentication_retries);
                        response["replayed"] = json!(attempt > 0 || authentication_retries > 0);
                    }
                    if attempt > 0 || authentication_retries > 0 {
                        partial["requestMayHaveBeenSent"] = json!(true);
                        partial["potentiallyMutating"] = json!(true);
                    }
                }
            }
            return result;
        }
        unreachable!("Bounded retries always settle their final attempt")
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "one replay counter spans authentication and explicit connection-reset attempts"
    )]
    fn run_once(
        &self,
        id: Uuid,
        fetch: &Fetch,
        mut url: Url,
        entity: (Vec<u8>, Option<String>),
        deadline: Instant,
        lease: &Lease,
        credentials: Option<&Credentials>,
        authentication_retries: &mut u32,
    ) -> Result<Reply, Error> {
        let (mut body, content_type) = entity;
        let mut has_body = fetch.body.is_some();
        let cancelled = &lease.cancelled;
        let transport = lease.transport();
        let mut method = fetch
            .method
            .parse::<ureq::http::Method>()
            .map_err(|_| Error::from("HTTP_METHOD_INVALID"))?;
        let mut headers = ureq::http::HeaderMap::new();
        for header in &fetch.headers {
            headers.insert(
                header
                    .name
                    .parse::<ureq::http::HeaderName>()
                    .map_err(|_| Error::from("HTTP_HEADER_INVALID"))?,
                request_wire::header(&header.value)?,
            );
        }
        if let Some(content_type) = content_type {
            headers
                .entry(ureq::http::header::CONTENT_TYPE)
                .or_insert(request_wire::header(&content_type)?);
        }
        // Pinned APIRequest overwrites provisional or explicit lengths for
        // every serialized body, including an empty buffer. Request copies
        // must not retain a browser initializer's old body length.
        if has_body {
            headers.insert(
                ureq::http::header::CONTENT_LENGTH,
                request_wire::header(&body.len().to_string())?,
            );
        }
        let max_redirects = fetch.max_redirects.unwrap_or(self.options.max_redirects);
        let timeout_ms = fetch.timeout_ms.unwrap_or(self.options.timeout_ms);
        // The pinned client applies always-send once, after explicit initial
        // headers. Redirects preserve it within an origin and remove it when
        // the origin changes; challenged credentials are selected per hop.
        if let Some(credential) = credentials
            .and_then(|credentials| credentials.matching(&url))
            .filter(|credential| credential.always())
        {
            headers.insert(
                ureq::http::header::AUTHORIZATION,
                credential
                    .header()
                    .parse()
                    .map_err(|_| Error::from("HTTP_CREDENTIALS_INVALID"))?,
            );
        }
        for step in 0..=max_redirects {
            let (mut response, wire, cookie_update) = loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let remaining = if timeout_ms == 0 {
                    remaining
                } else {
                    remaining.min(Duration::from_millis(timeout_ms))
                };
                if remaining.is_zero() {
                    return Err(unknown(self.reference, id));
                }
                let mut absent_body = ();
                let request_body = if has_body {
                    body.as_body()
                } else {
                    absent_body.as_body()
                };
                let destination = request_wire::transport_url(&url);
                let mut request = ureq::http::Request::builder()
                    .method(method.clone())
                    .uri(destination.as_str())
                    .body(request_body)
                    .map_err(|_| Error::from("HTTP_REQUEST_INVALID"))?;
                *request.headers_mut() = headers.clone();
                if !request
                    .headers()
                    .contains_key(ureq::http::header::AUTHORIZATION)
                    && let Some(authorization) = request_wire::authorization(&url)?
                {
                    request
                        .headers_mut()
                        .insert(ureq::http::header::AUTHORIZATION, authorization);
                }
                request
                    .headers_mut()
                    .entry(ureq::http::header::ACCEPT)
                    .or_insert(ureq::http::HeaderValue::from_static("*/*"));
                request
                    .headers_mut()
                    .entry(ureq::http::header::ACCEPT_ENCODING)
                    .or_insert(ureq::http::HeaderValue::from_static("gzip,deflate,br"));
                {
                    let agent = self
                        .options
                        .user_agent
                        .as_deref()
                        .unwrap_or_else(|| defaults::isolated_user_agent());
                    request
                        .headers_mut()
                        .entry(ureq::http::header::USER_AGENT)
                        .or_insert(request_wire::header(agent)?);
                }
                if !headers.contains_key(ureq::http::header::COOKIE) {
                    let cookie = if let Some(browser) = &self.browser {
                        let cookies=browser.read(deadline).map_err(|cause|{
                        let mut error=Error::from("HTTP_BROWSER_COOKIES_UNAVAILABLE");
                        error.partial=Some(json!({"httpContext":self.reference,"requestId":id,"cookieFailure":cause.code,"requestMayHaveBeenSent":step>0||*authentication_retries>0,"potentiallyMutating":step>0||*authentication_retries>0,"replayed":false}));error
                    })?;
                        browser::header(&cookies, &url, self.options.cookie_partition.as_ref())?
                    } else {
                        self.cookies.lock().expect("HTTP cookies").header(&url)
                    };
                    if !cookie.is_empty() {
                        request.headers_mut().insert(
                            ureq::http::header::COOKIE,
                            request_wire::header(&cookie)
                                .map_err(|_| Error::from("HTTP_COOKIE_INVALID"))?,
                        );
                    }
                }
                let remaining = remaining.min(deadline.saturating_duration_since(Instant::now()));
                if cancelled.load(Ordering::SeqCst) || remaining.is_zero() {
                    let mut error = Error::from(if cancelled.load(Ordering::SeqCst) {
                        "CANCELLED"
                    } else {
                        "HTTP_DEADLINE"
                    });
                    error.partial = Some(
                        json!({"httpContext":self.reference,"requestId":id,"requestMayHaveBeenSent":step>0,"potentiallyMutating":step>0,"replayed":false}),
                    );
                    return Err(error);
                }
                let bypassed = self
                    .proxy
                    .as_ref()
                    .is_some_and(|proxy| proxy.bypasses(&url));
                // Select again for every redirect/authentication attempt. An
                // origin's private identity never becomes another origin's or
                // an HTTPS proxy's TLS default, including a pooled connection.
                let origin = url.origin().ascii_serialization();
                let agents = transport
                    .certificate_agents
                    .iter()
                    .find(|(owner, _)| owner == &origin)
                    .map_or(&transport.agents, |(_, agents)| agents);
                let (default_agent, override_agent) =
                    agents.direct.as_ref().filter(|_| bypassed).map_or(
                        (&agents.default, &agents.other_tls_policy),
                        |(default, other)| (default, other),
                    );
                let agent = if fetch
                    .ignore_https_errors
                    .is_some_and(|value| value != self.options.ignore_https_errors.unwrap_or(false))
                {
                    override_agent
                } else {
                    default_agent
                };
                let request_deadline = deadline.min(Instant::now() + remaining);
                transport
                    .wire
                    .start_deadline(Some(cancelled.clone()), request_deadline.into_std());
                let response = request
                    .with_agent(agent)
                    .configure()
                    .timeout_global(Some(remaining))
                    .run();
                let response = transport
                    .wire
                    .refusal_response(response, &method, request_deadline.into_std())
                    .map_err(|cause| transport_failure(self.reference, id, cause))?;
                let wire = transport.wire.take(&response);
                let cookie_update = if let Some(browser) = &self.browser {
                    let cookies = browser::response_cookies(
                        &url,
                        response.headers(),
                        self.options.cookie_partition.as_ref(),
                    );
                    if cookies
                        .as_array()
                        .is_some_and(|cookies| !cookies.is_empty())
                    {
                        match browser.write(cookies, deadline) {
                            Ok(()) => json!({"state":"acknowledged"}),
                            Err(error) => {
                                json!({"state":"unknown","code":error.code,"replayed":false})
                            }
                        }
                    } else {
                        json!({"state":"none"})
                    }
                } else {
                    self.cookies
                        .lock()
                        .expect("HTTP cookies")
                        .receive(&url, response.headers());
                    Value::Null
                };
                // An acknowledged Basic challenge permits one replay with this
                // hop's selected credentials. A nonempty Authorization header then
                // prevents another challenge replay, even for incorrect secrets.
                if response.status().as_u16() == 401
                    && headers
                        .get(ureq::http::header::AUTHORIZATION)
                        .is_none_or(|value| value.is_empty())
                    && response
                        .headers()
                        .get(ureq::http::header::WWW_AUTHENTICATE)
                        .and_then(|value| value.to_str().ok())
                        .is_some_and(|value| value.trim().starts_with("Basic"))
                    && cookie_update["state"] != "unknown"
                    && !cancelled.load(Ordering::SeqCst)
                    && Instant::now() < deadline
                    && let Some(credential) =
                        credentials.and_then(|credentials| credentials.matching(&url))
                {
                    headers.insert(
                        ureq::http::header::AUTHORIZATION,
                        credential
                            .header()
                            .parse()
                            .map_err(|_| Error::from("HTTP_CREDENTIALS_INVALID"))?,
                    );
                    *authentication_retries += 1;
                    continue;
                }
                break (response, wire, cookie_update);
            };
            let status = response.status().as_u16();
            let redirect_status = matches!(status, 301 | 302 | 303 | 307 | 308);
            let redirect = redirect_status
                && response
                    .headers()
                    .get(ureq::http::header::LOCATION)
                    .is_some_and(|location| !location.is_empty());
            if redirect
                && let Some(location) = response.headers().get(ureq::http::header::LOCATION)
                && step < max_redirects
                && cookie_update["state"] != "unknown"
                && !cancelled.load(Ordering::SeqCst)
            {
                let next = url
                    .join(
                        location
                            .to_str()
                            .map_err(|_| Error::from("HTTP_REDIRECT_INVALID"))?,
                    )
                    .map_err(|_| Error::from("HTTP_REDIRECT_INVALID"))?;
                let next = absolute_url(next.as_str())?;
                if status == 303
                    && !matches!(method, ureq::http::Method::GET | ureq::http::Method::HEAD)
                    || matches!(status, 301 | 302) && method == ureq::http::Method::POST
                {
                    method = ureq::http::Method::GET;
                    has_body = false;
                    body.clear();
                    for name in [
                        ureq::http::header::CONTENT_ENCODING,
                        ureq::http::header::CONTENT_LANGUAGE,
                        ureq::http::header::CONTENT_LENGTH,
                        ureq::http::header::CONTENT_LOCATION,
                        ureq::http::header::CONTENT_TYPE,
                    ] {
                        headers.remove(name);
                    }
                }
                headers.remove(ureq::http::header::COOKIE);
                if next.origin() != url.origin() {
                    headers.remove(ureq::http::header::AUTHORIZATION);
                }
                headers.insert(
                    ureq::http::header::HOST,
                    next[url::Position::BeforeHost..url::Position::AfterPort]
                        .parse()
                        .map_err(|_| Error::from("HTTP_REDIRECT_INVALID"))?,
                );
                url = next;
                continue;
            }
            let raw_headers=json!(response.headers().iter().map(|(name,value)|json!({"name":name.as_str(),"value":value.to_str().unwrap_or("[binary header]")})).collect::<Vec<_>>());
            let mut metadata = json!({"httpContext":self.reference,"requestId":id,"url":super::diagnostics::url(url.as_str()),"status":status,"headers":super::network::headers(&raw_headers),"redirects":step,"redirectLimitReached":redirect_status&&step==max_redirects&&max_redirects>0,"bodyRequested":fetch.read_body,"bodyIncluded":false,"bodyComplete":false,"source":"rust-host-http","cookies":"isolated-host-jar","cors":"host-http-no-browser-cors","replayed":false,"dispatchCompleted":true,"potentiallyMutating":true});
            metadata["tlsVerification"] = json!(
                !fetch
                    .ignore_https_errors
                    .unwrap_or(self.options.ignore_https_errors.unwrap_or(false))
            );
            metadata["ok"] = json!((200..300).contains(&status));
            metadata["tlsPolicyOverridden"] = json!(fetch.ignore_https_errors.is_some());
            if let Some(context) = self.options.browser_context {
                metadata["cookies"] = json!("browser-context-exchange");
                metadata["browserContext"] = json!(context);
                metadata["browserCookieUpdate"] = cookie_update.clone();
                metadata["redirectStoppedForCookieUpdate"] =
                    json!(redirect && cookie_update["state"] == "unknown");
            }
            metadata["redirectStoppedForCancellation"] =
                json!(redirect && cancelled.load(Ordering::SeqCst));
            if metadata["redirectLimitReached"] == true {
                let mut error = Error::from("HTTP_REDIRECT_LIMIT");
                error.partial = Some(metadata);
                return Err(error);
            }
            if fetch
                .fail_on_status_code
                .unwrap_or(self.options.fail_on_status_code)
                && !(200..400).contains(&status)
            {
                let mut error = Error::from("HTTP_STATUS");
                error.partial = Some(metadata);
                return Err(error);
            }
            let bytes = if fetch.read_body {
                let encoding = response
                    .headers()
                    .get_all(ureq::http::header::CONTENT_ENCODING)
                    .iter()
                    .map(|value| value.to_str().unwrap_or("[binary header]"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut bytes = Vec::new();
                response
                    .body_mut()
                    .as_reader()
                    .take(MAX_BODY as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|cause| {
                        let reset = cause.kind() == std::io::ErrorKind::ConnectionReset;
                        let mut error = Error::caused("HTTP_REQUEST_OUTCOME_UNKNOWN", cause);
                        let mut partial = unknown(self.reference, id)
                            .partial
                            .expect("HTTP outcome metadata");
                        partial["response"] = metadata.clone();
                        partial["failureKind"] = json!("response_body");
                        partial["connectionReset"] = json!(reset);
                        error.partial = Some(partial);
                        error
                    })?;
                if bytes.len() > MAX_BODY {
                    let mut error = Error::from("HTTP_BODY_BOUNDS");
                    metadata["bodyBoundsStage"] = json!("encoded");
                    error.partial = Some(metadata);
                    return Err(error);
                }
                metadata["encodedBodyBytes"] = json!(bytes.len());
                metadata["contentEncoding"] = json!(match encoding.to_ascii_lowercase().as_str() {
                    "" => "identity",
                    "gzip" => "gzip",
                    "x-gzip" => "x-gzip",
                    "deflate" => "deflate",
                    "br" => "br",
                    _ => "unrecognized",
                });
                let body = response::decode(bytes, &encoding, fetch.max_bytes).map_err(|code| {
                    let mut error = Error::from(code);
                    let mut partial = metadata.clone();
                    partial["failureKind"] = json!("response_decoding");
                    error.partial = Some(partial);
                    error
                })?;
                metadata["bodyDecoded"] = json!(body.decoded);
                metadata["bodyBytes"] = json!(body.bytes.len());
                Some(body.bytes)
            } else {
                None
            };
            metadata["bodyComplete"] = json!(bytes.is_some());
            let transport = transport
                .wire
                .transport_details(bytes.is_some() || response.body().content_length() == Some(0));
            let mut details = details::capture(id, &url, &response, wire);
            let Value::Object(transport) = transport else {
                unreachable!("Transport fields");
            };
            details
                .as_object_mut()
                .expect("Private response fields")
                .extend(transport);
            return Ok(Reply {
                metadata,
                body: bytes,
                details: Some(details),
            });
        }
        Err(unknown(self.reference, id))
    }
}
pub(crate) fn pending(context: Reference, id: Uuid) -> Error {
    let mut error = Error::from("HTTP_RESPONSE_PENDING");
    error.partial = Some(
        json!({"httpContext":context,"requestId":id,"requestRetained":true,"nextAction":"http_response_finalize","replayed":false,"potentiallyMutating":true}),
    );
    error
}
fn unknown(context: Reference, id: Uuid) -> Error {
    let mut error = Error::from("HTTP_REQUEST_OUTCOME_UNKNOWN");
    error.partial = Some(
        json!({"httpContext":context,"requestId":id,"requestMayHaveBeenSent":true,"potentiallyMutating":true,"replayed":false}),
    );
    error
}
fn transport_failure(context: Reference, id: Uuid, cause: ureq::Error) -> Error {
    let reset = matches!(&cause,ureq::Error::Io(error) if error.kind()==std::io::ErrorKind::ConnectionReset);
    let kind = match &cause {
        ureq::Error::Timeout(_) => "timeout",
        ureq::Error::HostNotFound => "dns",
        ureq::Error::ConnectionFailed => "connection",
        ureq::Error::Io(error)
            if error
                .get_ref()
                .is_some_and(|cause| cause.is::<rustls::Error>()) =>
        {
            "tls"
        }
        ureq::Error::Io(_) => "io",
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "tls",
        _ => "transport",
    };
    let mut error = Error::caused("HTTP_REQUEST_OUTCOME_UNKNOWN", cause);
    let mut partial = unknown(context, id).partial.expect("HTTP outcome metadata");
    partial["failureKind"] = json!(kind);
    partial["connectionReset"] = json!(reset);
    error.partial = Some(partial);
    error
}

pub(crate) fn header_schema() -> Value {
    json!({"type":"array","maxItems":64,"items":{"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":1024},"value":{"type":"string","maxLength":16384}},"required":["name","value"],"additionalProperties":false}})
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let headers = header_schema();
    let mut options = json!({"type":"object","properties":{"browserContext":{"anyOf":[reference,{"type":"null"}]},"baseUrl":{"type":["string","null"],"maxLength":4096},"headers":headers,"timeoutMs":{"type":"integer","minimum":0,"maximum":120000,"default":30000},"maxRedirects":{"type":"integer","minimum":0,"maximum":20,"default":20},"proxy":{"type":["string","null"],"maxLength":4096},"ignoreHttpsErrors":{"type":"boolean","default":false},"failOnStatusCode":{"type":"boolean","default":false}},"additionalProperties":false});
    options["properties"]["cookiePartition"] = json!({"anyOf":[{"type":"null"},{"type":"object","properties":{"topLevelSite":{"type":"string","minLength":1,"maxLength":2048,"description":"Exact canonical HTTP(S) origin naming the selected browser cookie partition"},"hasCrossSiteAncestor":{"type":["boolean","null"],"description":"Chrome partition ancestry; omitted selects its pinned true default. WebKit accepts only an omitted ancestry."}},"required":["topLevelSite"],"additionalProperties":false}]});
    options["properties"]["headers"] = json!({"anyOf":[headers,{"type":"null"}],"description":"Default HTTP headers across request origins, with or without baseUrl. Omitted/null follows current associated context headers; an explicit list, including [], overrides them."});
    options["properties"]["proxy"] = Proxy::schema();
    let mut request = json!({"type":"object","properties":{"url":{"type":"string","maxLength":4096},"method":{"type":"string","minLength":1,"maxLength":32,"default":"GET"},"headers":headers,"body":body_schema(),"readBody":{"type":"boolean","default":false},"maxBytes":{"type":"integer","minimum":1,"maximum":MAX_BODY,"default":65536}},"required":["url"],"additionalProperties":false});
    request["properties"]["params"] = json!({"anyOf":[{"type":["string","null"],"maxLength":4096},{"type":"array","maxItems":128,"items":{"type":"object","properties":{"name":{"type":"string","maxLength":1024},"value":{"type":"string","maxLength":4096}},"required":["name","value"],"additionalProperties":false}}]});
    request["properties"]["timeoutMs"] = json!({"type":["integer","null"],"minimum":0,"maximum":120000,"description":"Zero disables the HTTP client timeout; the enclosing operation deadline still applies"});
    request["properties"]["failOnStatusCode"] = json!({"type":["boolean","null"]});
    request["properties"]["ignoreHttpsErrors"] = json!({"type":["boolean","null"],"description":"Override TLS verification for this request and its redirects; the context default is unchanged"});
    request["properties"]["maxRetries"] =
        json!({"type":"integer","minimum":0,"maximum":20,"default":0});
    request["properties"]["maxRedirects"] =
        json!({"type":["integer","null"],"minimum":0,"maximum":20});
    options["properties"]["allowUnknownCookiePartitions"] = json!({"type":"boolean","default":false,"description":"Explicitly accept a browser cookie view that omits partition labels; required for associated Firefox HTTP clients. This cannot select a partition."});
    options["properties"]["ignoreHttpsErrors"] = json!({"type":["boolean","null"],"description":"Override browser-context TLS policy; omitted inherits the associated context or verifies TLS for isolated clients."});
    options["properties"]["userAgent"] = json!({"type":["string","null"],"maxLength":2048,"description":"HTTP default User-Agent; omitted, null or empty inherits the associated browser's configured or engine-default agent, or the pinned initial-host/language agent for an isolated client. Per-request headers take precedence, including a literal empty User-Agent header."});
    options["properties"]["httpCredentials"] = Credentials::schema();
    options["properties"]["clientCertificates"] = client_certificates::schema();
    options["properties"]["storageState"] = StorageStateInput::schema();
    let mut schemas = vec![
        json!({"type":"object","properties":{"operation":{"const":"http_context_create"},"options":options},"required":["operation"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"http_fetch"},"httpContext":reference,"request":request},"required":["operation","httpContext","request"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"route_fetch"},"callback":reference,"httpContext":reference,"maxRedirects":{"type":["integer","null"],"minimum":0,"maximum":20},"timeoutMs":{"type":["integer","null"],"minimum":0,"maximum":120000},"maxRetries":{"type":["integer","null"],"minimum":0,"maximum":20,"description":"Retries after a connection reset"},"overrides":{"anyOf":[super::routing::Overrides::schema(),{"type":"null"}],"description":"Replace the URL, method, whole header list or bodyBase64 for this fetch only"}},"required":["operation","callback","httpContext"],"additionalProperties":false,"description":"Send a pending route callback's request through an HTTP client and keep the callback pending; reply with route_fulfill_http_evidence to use the response"}),
        json!({"type":"object","properties":{"operation":{"const":"http_evidence_read"},"evidenceId":{"type":"string","format":"uuid"},"format":{"enum":["base64","text","json","json_value"]},"maxBytes":{"type":"integer","minimum":1,"maximum":65536,"default":65536,"description":"Bound the entire encoded private result; oversized values refuse without truncation or HTTP replay"}},"required":["operation","evidenceId","format"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"http_response_read"},"requestId":{"type":"string","format":"uuid"},"includeHeadersObject":{"type":"boolean","default":false,"description":"Include the pinned SDK lowercased/joined header object and own-key order, retaining the original header array"},"maxBytes":{"type":"integer","minimum":1,"maximum":65536,"default":65536}},"required":["operation","requestId"],"additionalProperties":false}),
    ];
    for operation in ["http_context_list", "http_evidence_list"] {
        schemas.push(json!({"type":"object","properties":{"operation":{"const":operation}},"required":["operation"],"additionalProperties":false}));
    }
    for operation in ["http_context_remove", "http_context_cookies_clear"] {
        let mut schema = json!({"type":"object","properties":{"operation":{"const":operation},"httpContext":reference},"required":["operation","httpContext"],"additionalProperties":false});
        if operation == "http_context_cookies_clear" {
            schema["properties"]["options"] = CookieFilter::schema();
        }
        schemas.push(schema);
    }
    for operation in [
        "http_response_finalize",
        "http_response_discard",
        "http_response_remove",
    ] {
        schemas.push(json!({"type":"object","properties":{"operation":{"const":operation},"requestId":{"type":"string","format":"uuid"}},"required":["operation","requestId"],"additionalProperties":false}));
    }
    schemas.push(json!({"type":"object","properties":{"operation":{"const":"http_evidence_remove"},"evidenceId":{"type":"string","format":"uuid"}},"required":["operation","evidenceId"],"additionalProperties":false}));
    schemas.extend(cookie_admin::schemas(reference));
    schemas
}

pub(super) fn body_schema() -> Value {
    let mut body = json!({"anyOf":[{"type":"null"},{"type":"object","properties":{"kind":{"const":"text"},"text":{"type":"string","maxLength":MAX_BODY}},"required":["kind","text"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"base64"},"data":{"type":"string","maxLength":MAX_BODY.div_ceil(3)*4}},"required":["kind","data"],"additionalProperties":false}]});
    let variants = body["anyOf"].as_array_mut().expect("HTTP payload schemas");
    variants.extend([
        json!({"type":"object","properties":{"kind":{"const":"json"},"value":{}},"required":["kind","value"],"additionalProperties":false}),
        json!({"type":"object","properties":{"kind":{"const":"form"},"fields":{"type":"array","maxItems":128,"items":{"type":"object","properties":{"name":{"type":"string","maxLength":1024},"value":{"type":"string","maxLength":16384}},"required":["name","value"],"additionalProperties":false}}},"required":["kind","fields"],"additionalProperties":false}),
    ]);
    let parts = json!({"oneOf":[
        {"type":"object","properties":{"kind":{"const":"text"},"name":{"type":"string","maxLength":1024},"value":{"type":"string","maxLength":MAX_BODY}},"required":["kind","name","value"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"file"},"name":{"type":"string","maxLength":1024},"filename":{"type":"string","maxLength":1024},"mimeType":{"type":"string","maxLength":256,"default":"","description":"Explicit MIME type; omitted or empty infers from the filename using the pinned SDK table"},"data":{"type":"string","maxLength":MAX_BODY.div_ceil(3)*4}},"required":["kind","name","filename","data"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"file_path"},"name":{"type":"string","maxLength":1024},"path":{"type":"string","minLength":1,"maxLength":4096,"description":"Absolute server-host path to one selected regular file; bytes are snapshotted before HTTP dispatch"},"filename":{"type":["string","null"],"maxLength":1024},"mimeType":{"type":"string","maxLength":256,"default":""}},"required":["kind","name","path"],"additionalProperties":false}
    ]});
    variants.push(json!({"type":"object","properties":{"kind":{"const":"multipart"},"fields":{"type":"array","maxItems":128,"items":parts}},"required":["kind","fields"],"additionalProperties":false}));
    variants[1]["description"] = json!(
        "String data: the first caller Content-Type header selects JSON handling only for exact application/json. Valid JSON text is preserved; other strings are JSON-quoted within the body byte bound. Context default headers do not select this branch; base64 data bypasses it."
    );
    body
}

#[cfg(test)]
mod tests;
