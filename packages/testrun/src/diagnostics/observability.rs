//! Explicit provider reads; credentials stay with a selected existing owner.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Scope {
    pub account: String,
    pub project: String,
    pub environment: String,
    pub region: String,
    pub resource: String,
    pub release: String,
}
impl Scope {
    pub fn validate(&self) -> Result<()> {
        for s in [
            &self.account,
            &self.project,
            &self.environment,
            &self.region,
            &self.resource,
            &self.release,
        ] {
            super::model::label(s, 256)?;
        }
        Ok(())
    }
    pub fn fields(&self) -> [(&str, &str); 6] {
        [
            ("account", &self.account),
            ("project", &self.project),
            ("environment", &self.environment),
            ("region", &self.region),
            ("resource", &self.resource),
            ("release", &self.release),
        ]
    }
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Window {
    pub from: String,
    pub to: String,
}
pub(crate) fn utc(text: &str) -> Result<f64> {
    let t = time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
        .map_err(|_| invalid())?;
    Ok(t.unix_timestamp_nanos() as f64 / 1e9)
}
impl Window {
    pub fn validate(&self, max_seconds: u32) -> Result<(f64, f64)> {
        if !self.from.ends_with('Z') || !self.to.ends_with('Z') {
            return Err(invalid());
        }
        let a = utc(&self.from)?;
        let b = utc(&self.to)?;
        if !a.is_finite() || !b.is_finite() || b <= a || b - a > max_seconds as f64 {
            return Err(invalid());
        }
        Ok((a, b))
    }
}
fn invalid() -> Error {
    Error::invalid(
        "Select exact provider scope and explicit UTC Z window with a private configured read preset and bounded limits",
    )
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Options {
    pub config_file: PathBuf,
    pub source_id: String,
    pub scope: Scope,
    pub window: Window,
    #[serde(default)]
    pub include_text: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Config {
    schema_version: u32,
    source_id: String,
    scope: Scope,
    endpoint: String,
    authorization_owner: String,
    authorization: Authorization,
    read: Read,
    timeout_ms: u32,
    max_window_seconds: u32,
    max_records: usize,
    max_bytes: usize,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Authorization {
    None,
    BearerFile { path: PathBuf },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Read {
    Prometheus {
        metric: String,
        unit: String,
        definition: String,
        #[serde(rename = "stepSeconds")]
        step_seconds: u32,
        #[serde(rename = "maxSeries")]
        max_series: usize,
        #[serde(rename = "scopeLabels")]
        scope_labels: BTreeMap<String, String>,
    },
    Loki {
        #[serde(rename = "scopeLabels")]
        scope_labels: BTreeMap<String, String>,
    },
    SentryEvent,
}
pub(crate) struct Capture {
    options: Options,
    job: Option<tokio::task::JoinHandle<Result<Vec<u8>>>>,
}
fn label_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
}
fn credential_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "password"
            | "token"
            | "access_token"
            | "refresh_token"
            | "api_key"
            | "api-key"
            | "apikey"
            | "secret"
    )
}
fn selector(scope: &Scope, labels: &BTreeMap<String, String>) -> Result<String> {
    if labels.len() != 6 {
        return Err(invalid());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut fields = vec![];
    for (field, value) in scope.fields() {
        let name = labels
            .get(field)
            .filter(|s| label_name(s) && !credential_key(s))
            .ok_or_else(invalid)?;
        if !names.insert(name) {
            return Err(invalid());
        }
        fields.push(format!("{name}={}", serde_json::to_string(value).unwrap()));
    }
    Ok(format!("{{{}}}", fields.join(",")))
}
fn scrub(v: &mut Value, secret: &str) {
    match v {
        Value::String(s) if !secret.is_empty() => {
            *s = s.replace(secret, "[redacted-provider-credential]");
        }
        Value::Array(a) => a.iter_mut().for_each(|v| scrub(v, secret)),
        Value::Object(o) => {
            let fields = std::mem::take(o);
            for (mut k, mut value) in fields {
                if credential_key(&k) {
                    continue;
                }
                if !secret.is_empty() {
                    k = k.replace(secret, "[redacted-provider-credential]");
                }
                scrub(&mut value, secret);
                o.insert(k, value);
            }
        }
        _ => {}
    }
}
fn perform(options: Options, budget: Duration) -> Result<Vec<u8>> {
    let bytes = crate::artifacts::read_private_file(&options.config_file, 32768).map_err(|_| {
        Error::new(
            "OBSERVABILITY_CONFIG",
            "Read the selected private regular config file; no other configuration was searched",
        )
    })?;
    let config: Config =
        serde_json::from_value(super::analysis::apple_json(&bytes)?).map_err(|_| invalid())?;
    validate_config(&config, &options)?;
    let (from, to) = options.window.validate(config.max_window_seconds)?;
    let (endpoint, (provider, format, unit, definition)) =
        prepare_read(&config, &options, from, to)?;
    let (raw, secret) = provider_read(&config, &endpoint, budget)?;
    let mut payload = super::analysis::apple_json(&raw)?;
    filter_payload(&config, &options, provider, from, to, &mut payload)?;
    scrub(&mut payload, &secret);
    let mut packet = json!({"schemaVersion":1,"provider":provider,"sourceId":options.source_id,"scope":options.scope,"window":options.window,"authorizationOwner":config.authorization_owner,"configSha256":super::digest(&bytes),"request":{"method":"GET","endpointSha256":super::digest(config.endpoint.as_bytes()),"parameters":endpoint.query_pairs().map(|(k,v)|(k.into_owned(),v.into_owned())).collect::<BTreeMap<_,_>>()},"format":format,"unit":unit,"definition":definition,"pages":[{"payload":payload,"responseSha256":super::digest(&raw)}],"coverage":"partial","limits":{"maxRecords":config.max_records,"maxBytes":config.max_bytes,"timeoutMs":config.timeout_ms},"scan":{"bytes":null,"cost":null,"meaning":"backend scan/cost unknown; response and request-time bounds only"}});
    scrub(&mut packet, &secret);
    serde_json::to_vec(&packet).map_err(|_| invalid())
}
impl Capture {
    pub fn new(options: Options) -> Result<Self> {
        if !options.config_file.is_absolute() {
            return Err(invalid());
        }
        super::model::label(&options.source_id, 256)?;
        options.scope.validate()?;
        options.window.validate(86400)?;
        Ok(Self { options, job: None })
    }
    pub async fn read(&mut self, budget: Duration) -> Result<Vec<u8>> {
        if !cfg!(unix) {
            return Err(Error::new(
                "UNSUPPORTED_HOST",
                "Configured provider reads currently require Unix private-file ownership checks; use explicit exports on other hosts",
            ));
        }
        if budget.is_zero() {
            return Err(Error::new("TIMEOUT", "Provider read budget expired"));
        }
        let options = self.options.clone();
        self.job = Some(tokio::task::spawn_blocking(move || {
            perform(options, budget)
        }));
        let result = self
            .job
            .as_mut()
            .unwrap()
            .await
            .map_err(|_| Error::new("OBSERVABILITY_WORKER", "Provider read worker stopped"))?;
        self.job = None;
        result
    }
    pub async fn settle(&mut self) -> Value {
        let pending = if let Some(job) = self.job.take() {
            tokio::time::timeout(Duration::from_secs(1), job)
                .await
                .is_err()
        } else {
            false
        };
        json!({"providerReadMayContinue":pending,"providerQueryCancellation":"not_proven; provider work/cost may outlive client timeout","credentialsRefreshed":false,"mutationPerformed":false})
    }
}

fn validate_config(config: &Config, options: &Options) -> Result<()> {
    if config.schema_version != 1
        || config.source_id != options.source_id
        || config.scope != options.scope
    {
        return Err(Error::new(
            "OBSERVABILITY_SCOPE",
            "Configured source/scope differs; no provider request was sent",
        ));
    }
    config.scope.validate()?;
    super::model::label(&config.authorization_owner, 256)?;
    if !(1..=86400).contains(&config.max_window_seconds)
        || !(1..=5000).contains(&config.timeout_ms)
        || !(1..=2000).contains(&config.max_records)
        || !(1024..=1048576).contains(&config.max_bytes)
    {
        return Err(invalid());
    }
    Ok(())
}

fn provider_endpoint(config: &Config) -> Result<url::Url> {
    let endpoint = url::Url::parse(&config.endpoint).map_err(|_| invalid())?;
    let local = endpoint
        .host_str()
        .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if !(endpoint.scheme() == "https"
        || endpoint.scheme() == "http" && local && endpoint.port().is_some())
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || config.endpoint.len() > 2048
    {
        return Err(invalid());
    }
    Ok(endpoint)
}

type ReadSemantics = (&'static str, &'static str, Option<String>, Option<String>);
fn prepare_read(
    config: &Config,
    options: &Options,
    from: f64,
    to: f64,
) -> Result<(url::Url, ReadSemantics)> {
    let mut endpoint = provider_endpoint(config)?;
    let prefix = endpoint.path().trim_end_matches('/').to_owned();
    let semantics = match &config.read {
        Read::Prometheus {
            metric,
            unit,
            definition,
            step_seconds,
            max_series,
            scope_labels,
        } => {
            if metric.is_empty()
                || metric.len() > 128
                || !metric.bytes().enumerate().all(|(i, b)| {
                    b == b'_' || b == b':' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit()
                })
                || !(1..=3600).contains(step_seconds)
                || !(1..=32).contains(max_series)
                || (((to - from) / *step_seconds as f64).floor() as usize + 1)
                    .saturating_mul(*max_series)
                    > config.max_records
            {
                return Err(invalid());
            }
            super::model::label(unit, 256)?;
            super::model::label(definition, 256)?;
            let query = format!("{metric}{}", selector(&config.scope, scope_labels)?);
            endpoint.set_path(&format!("{prefix}/api/v1/query_range"));
            endpoint
                .query_pairs_mut()
                .append_pair("query", &query)
                .append_pair("start", &from.to_string())
                .append_pair("end", &to.to_string())
                .append_pair("step", &step_seconds.to_string())
                .append_pair("limit", &max_series.to_string())
                .append_pair("timeout", &format!("{}ms", config.timeout_ms));
            (
                "prometheus",
                "prometheus-json",
                Some(unit.clone()),
                Some(definition.clone()),
            )
        }
        Read::Loki { scope_labels } => {
            let query = selector(&config.scope, scope_labels)?;
            endpoint.set_path(&format!("{prefix}/loki/api/v1/query_range"));
            endpoint
                .query_pairs_mut()
                .append_pair("query", &query)
                .append_pair("start", &options.window.from)
                .append_pair("end", &options.window.to)
                .append_pair("limit", &config.max_records.to_string())
                .append_pair("direction", "forward");
            ("loki", "loki-json", None, None)
        }
        Read::SentryEvent => {
            if ![&config.scope.account, &config.scope.project]
                .iter()
                .all(|s| {
                    s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                })
                || config.scope.resource.len() != 32
                || !config.scope.resource.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(invalid());
            }
            endpoint.set_path(&format!(
                "{prefix}/api/0/projects/{}/{}/events/{}/",
                config.scope.account, config.scope.project, config.scope.resource
            ));
            ("sentry", "sentry-json", None, None)
        }
    };
    Ok((endpoint, semantics))
}

fn provider_read(
    config: &Config,
    endpoint: &url::Url,
    budget: Duration,
) -> Result<(Vec<u8>, String)> {
    let secret = match &config.authorization {
        Authorization::None => String::new(),
        Authorization::BearerFile { path } => {
            let bytes=crate::artifacts::read_private_file(path,4096).map_err(|_|Error::new("OBSERVABILITY_CREDENTIAL","Selected credential file is unavailable/private ownership invalid; refresh through its existing owner"))?;
            let s = std::str::from_utf8(&bytes).map_err(|_| invalid())?.trim();
            if s.len() < 16 || s.chars().any(char::is_control) {
                return Err(invalid());
            }
            s.to_owned()
        }
    };
    let timeout = budget.min(Duration::from_millis(config.timeout_ms as u64));
    if timeout.is_zero() {
        return Err(Error::new("TIMEOUT", "Provider read budget expired"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut request = agent.get(endpoint.as_str());
    if !secret.is_empty() {
        request = request.header("Authorization", format!("Bearer {secret}"));
    }
    let mut response=request.call().map_err(|_|Error::new("OBSERVABILITY_UNAVAILABLE","Selected provider read failed/timed out; no retry, redirect, alternate endpoint or credential was tried"))?;
    let status = response.status().as_u16();
    if status == 401 || status == 403 {
        return Err(Error::new(
            "OBSERVABILITY_DENIED",
            "Provider denied this principal; refresh/review access through its existing owner, without stronger-credential fallback",
        ));
    }
    if status == 429 {
        let retry = response
            .headers()
            .get("retry-after")
            .and_then(|s| s.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        return Err(Error::new(
            "OBSERVABILITY_RATE_LIMITED",
            "Provider rate limited the read; no retry ran",
        )
        .details(json!({"retryAfterSeconds":retry})));
    }
    if status != 200 {
        return Err(Error::new("OBSERVABILITY_HTTP","Provider did not return HTTP 200; response body and redirect location were not retained").details(json!({"httpStatus":status})));
    }
    let raw = response
        .body_mut()
        .with_config()
        .limit(config.max_bytes as u64)
        .read_to_vec()
        .map_err(|_| {
            Error::new(
                "OBSERVABILITY_BOUNDS",
                "Provider response exceeded its byte budget or could not be read",
            )
        })?;
    Ok((raw, secret))
}

fn filter_series(
    config: &Config,
    options: &Options,
    provider: &str,
    scope_labels: &BTreeMap<String, String>,
    payload: &mut Value,
) -> Result<()> {
    if payload["status"] != "success" {
        return Err(Error::new(
            "OBSERVABILITY_PROVIDER_ERROR",
            "Provider returned a query error; error payload was not retained",
        ));
    }
    let results = payload["data"]["result"].as_array().ok_or_else(invalid)?;
    if provider == "prometheus"
        && (payload["data"]["resultType"] != "matrix"
            || matches!(&config.read,Read::Prometheus{max_series,..} if results.len()>*max_series))
        || provider == "loki" && payload["data"]["resultType"] != "streams"
    {
        return Err(Error::new(
            "OBSERVABILITY_BOUNDS",
            "Provider returned an unsupported result type or exceeded selected series limit",
        ));
    }
    let mut records = 0;
    for series in results {
        if let Read::Prometheus { metric, .. } = &config.read
            && series["metric"]["__name__"] != *metric
        {
            return Err(Error::new(
                "OBSERVABILITY_SCOPE",
                "Provider returned a metric different from the configured literal selection",
            ));
        }
        let labels = if provider == "loki" {
            &series["stream"]
        } else {
            &series["metric"]
        };
        for (field, value) in config.scope.fields() {
            if labels[&scope_labels[field]] != value {
                return Err(Error::new(
                    "OBSERVABILITY_SCOPE",
                    "Provider result contradicts selected label scope; no response retained",
                ));
            }
        }
        records += series["values"].as_array().map_or(1, Vec::len);
    }
    if records > config.max_records {
        return Err(Error::new(
            "OBSERVABILITY_BOUNDS",
            "Provider result exceeds its record budget",
        ));
    }
    // Retain only selected scope labels, metric name and explicitly requested log text.
    for series in payload["data"]["result"].as_array_mut().unwrap() {
        let field = if provider == "loki" {
            "stream"
        } else {
            "metric"
        };
        series[field]
            .as_object_mut()
            .ok_or_else(invalid)?
            .retain(|key, _| key == "__name__" || scope_labels.values().any(|name| name == key));
        if provider == "loki" && !options.include_text {
            for value in series["values"].as_array_mut().ok_or_else(invalid)? {
                let text = value[1].as_str().ok_or_else(invalid)?;
                let digest = super::digest(text.as_bytes());
                value[1] = json!("[text omitted]");
                if let Some(a) = value.as_array_mut() {
                    a.truncate(2);
                    a.push(json!({"textSha256":digest}));
                }
            }
        }
    }
    Ok(())
}

fn filter_sentry(
    config: &Config,
    options: &Options,
    from: f64,
    to: f64,
    payload: &mut Value,
) -> Result<()> {
    let release = payload["release"]
        .as_str()
        .or_else(|| payload["release"]["version"].as_str());
    let environment = payload["tags"]
        .as_array()
        .and_then(|a| a.iter().find(|t| t["key"] == "environment"))
        .and_then(|t| t["value"].as_str());
    let created = payload["dateCreated"]
        .as_str()
        .ok_or_else(invalid)
        .and_then(utc)?;
    if release != Some(config.scope.release.as_str())
        || environment != Some(config.scope.environment.as_str())
        || created < from
        || created > to
        || payload["eventID"] != config.scope.resource
    {
        return Err(Error::new(
            "OBSERVABILITY_SCOPE",
            "Sentry event identity/release/environment/window does not match; no response retained",
        ));
    }
    let fields = [
        "eventID",
        "dateCreated",
        "release",
        "tags",
        "platform",
        "level",
        "contexts",
    ];
    payload
        .as_object_mut()
        .ok_or_else(invalid)?
        .retain(|key, _| {
            fields.contains(&key.as_str())
                || options.include_text && ["title", "message", "entries"].contains(&key.as_str())
        });
    if !options.include_text {
        payload["release"] = json!(config.scope.release);
        if let Some(tags) = payload["tags"].as_array_mut() {
            tags.retain(|t| matches!(t["key"].as_str(), Some("environment" | "release")));
        }
        let mut trace = payload["contexts"]["trace"].clone();
        if let Some(m) = trace.as_object_mut() {
            m.retain(|k, _| {
                [
                    "trace_id",
                    "span_id",
                    "parent_span_id",
                    "op",
                    "status",
                    "type",
                ]
                .contains(&k.as_str())
            });
        }
        payload["contexts"] = json!({"trace":trace});
    }
    Ok(())
}

fn filter_payload(
    config: &Config,
    options: &Options,
    provider: &str,
    from: f64,
    to: f64,
    payload: &mut Value,
) -> Result<()> {
    // Validate actual result identities and limits before retaining any response.
    match &config.read {
        Read::Prometheus { scope_labels, .. } | Read::Loki { scope_labels } => {
            filter_series(config, options, provider, scope_labels, payload)?;
        }
        Read::SentryEvent => {
            filter_sentry(config, options, from, to, payload)?;
        }
    }
    // Discard unknown response fields before persistence, including provider extensions.
    if matches!(provider, "prometheus" | "loki") {
        let mut results = payload["data"]["result"].clone();
        for series in results.as_array_mut().ok_or_else(invalid)? {
            series
                .as_object_mut()
                .ok_or_else(invalid)?
                .retain(|k, _| matches!(k.as_str(), "metric" | "stream" | "values"));
            for point in series["values"].as_array().ok_or_else(invalid)? {
                let at = if provider == "loki" {
                    point[0]
                        .as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|n| n as f64 / 1e9)
                } else {
                    point[0].as_f64()
                };
                if at.is_none_or(|n| !n.is_finite() || n < from || n > to) {
                    return Err(Error::new(
                        "OBSERVABILITY_SCOPE",
                        "Provider returned an observation outside the selected UTC window",
                    ));
                }
            }
        }
        *payload = json!({"status":"success","data":{"resultType":payload["data"]["resultType"],"result":results},"providerWarningsPresent":payload.get("warnings").is_some(),"providerInfosPresent":payload.get("infos").is_some()});
    }
    Ok(())
}
