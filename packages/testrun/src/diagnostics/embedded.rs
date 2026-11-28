//! Borrowed CDP page observation; never enables endpoints, drives input or profiles.
use crate::{
    browser::cdp::{Connection, EventItem, Events, Lane},
    error::{Error, Result},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::watch;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Options {
    pub endpoint: String,
    pub target_id: String,
    pub app_id: String,
    #[serde(default = "super::react_native::mode")]
    pub mode: String,
    pub page_id: Option<String>,
    pub expected_url: Option<String>,
    #[serde(default)]
    pub allow_debugger_attachment: bool,
    #[serde(default)]
    pub network: bool,
    #[serde(default = "super::react_native::sample_ms")]
    pub sample_ms: u64,
}
pub(crate) struct Capture {
    options: Options,
    base: url::Url,
    stage: u8,
    selected: Value,
    connection: Option<Connection>,
    events: Option<Events>,
    lease: Option<crate::lease::ControlLease>,
    control: Option<(watch::Receiver<Option<String>>, Instant)>,
    pub actions: Vec<String>,
    epoch: String,
    gaps: Vec<String>,
}
fn invalid() -> Error {
    Error::invalid(
        "Select embedded_cdp discover/observe, exact pageId/expectedUrl and allowDebuggerAttachment:true for observe; sampleMs 100..10000",
    )
}
pub(super) fn pages(base: &url::Url, value: &Value) -> Result<Vec<Value>> {
    let entries = value
        .as_array()
        .filter(|a| a.len() <= 64)
        .ok_or_else(invalid)?;
    let mut ids = std::collections::BTreeSet::new();
    let mut pages = vec![];
    for entry in entries {
        if entry["type"] != "page" && entry["type"] != "webview" {
            continue;
        }
        let id = entry["id"].as_str().ok_or_else(invalid)?;
        super::model::label(id, 256)?;
        if !ids.insert(id) {
            return Err(invalid());
        }
        let socket = entry["webSocketDebuggerUrl"].as_str().ok_or_else(invalid)?;
        let u = url::Url::parse(socket).map_err(|_| invalid())?;
        if u.scheme() != "ws"
            || u.host_str() != base.host_str()
            || u.port() != base.port()
            || u.path() != format!("/devtools/page/{id}")
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || socket.len() > 2048
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(invalid());
        }
        let url = entry["url"].as_str().ok_or_else(invalid)?;
        super::model::label(url, 2048)?;
        pages.push(json!({"id":id,"url":url,"type":entry["type"],"webSocketDebuggerUrl":socket}));
    }
    Ok(pages)
}
impl Capture {
    pub fn new(options: Options) -> Result<Self> {
        let base = super::inspector::endpoint(&options.endpoint)?;
        for s in [&options.target_id, &options.app_id] {
            super::model::label(s, 256)?;
        }
        if !matches!(options.mode.as_str(), "discover" | "observe")
            || !(100..=10000).contains(&options.sample_ms)
        {
            return Err(invalid());
        }
        if options.mode == "observe" {
            super::model::label(options.page_id.as_deref().ok_or_else(invalid)?, 256)?;
            super::model::label(options.expected_url.as_deref().ok_or_else(invalid)?, 2048)?;
            if !options.allow_debugger_attachment {
                return Err(invalid());
            }
        }
        Ok(Self {
            options,
            base,
            stage: 0,
            selected: Value::Null,
            connection: None,
            events: None,
            lease: None,
            control: None,
            actions: vec![],
            epoch: uuid::Uuid::new_v4().to_string(),
            gaps: vec![
                "partial_window; endpoint/native process association is caller declared".into(),
            ],
        })
    }
    pub fn control(&mut self, stopped: watch::Receiver<Option<String>>, budget: Duration) {
        self.control = Some((stopped, Instant::now() + budget));
    }
    fn budget(&self) -> Result<Duration> {
        let mut budget = Duration::from_secs(3);
        if let Some((stop, end)) = &self.control {
            if stop.borrow().is_some() {
                return Err(Error::new("CANCELLED", "Embedded capture stopped"));
            }
            budget = budget.min(end.saturating_duration_since(Instant::now()));
        }
        if budget.is_zero() {
            return Err(Error::new("TIMEOUT", "Embedded capture budget expired"));
        }
        Ok(budget)
    }
    fn window(&self) -> Result<Duration> {
        self.budget()?;
        let requested = Duration::from_millis(self.options.sample_ms);
        Ok(self.control.as_ref().map_or(requested, |(_, end)| {
            requested.min(end.saturating_duration_since(Instant::now()))
        }))
    }
    async fn call(&self, method: &str) -> Result<()> {
        self.connection
            .as_ref()
            .unwrap()
            .call(
                None,
                method,
                json!({}),
                Lane::Ordinary,
                tokio::time::Instant::now() + self.budget()?,
            )
            .await
            .map(|_| ())
            .map_err(|_| {
                Error::new(
                    "EMBEDDED_PROTOCOL_UNAVAILABLE",
                    "Selected page refused observation; no alternate target or transport was tried",
                )
            })
    }
    fn packet(&self, kind: &str, data: Value, complete: bool) -> Result<(Vec<u8>, bool, Value)> {
        let value = json!({"schemaVersion":1,"adapter":"embedded-cdp/1","kind":kind,"targetId":self.options.target_id,"appId":self.options.app_id,"pageId":self.options.page_id,"runtimeGeneration":self.epoch,"actionIds":self.actions,"data":data});
        let mut bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
        if bytes.len() > 1024 * 1024 {
            return Err(Error::new(
                "EMBEDDED_BOUNDS",
                "Embedded packet exceeds 1 MiB",
            ));
        }
        bytes.push(b'\n');
        Ok((
            bytes,
            complete,
            json!({"adapter":"embedded-cdp/1","runtimeGeneration":self.epoch,"historyComplete":false,"nativeAssociationVerified":false,"sourceLoss":self.gaps,"stage":self.stage}),
        ))
    }
    pub async fn read(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.budget()?;
        if self.stage == 0 {
            return self.discover().await;
        }
        if self.stage == 1 {
            return self.attach().await;
        }
        if self.stage == 2 {
            let until = Instant::now() + self.window()?;
            let mut records = vec![];
            let mut size = 0;
            let mut changed = false;
            while records.len() < 256 && size < 256 * 1024 && Instant::now() < until {
                self.budget()?;
                let wait = until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50));
                let item = tokio::time::timeout(wait, self.events.as_mut().unwrap().next()).await;
                let event = match item {
                    Ok(Some(EventItem::Event(e))) => e,
                    Ok(Some(_)) => {
                        self.gaps.push("protocol_event_loss".into());
                        changed = true;
                        break;
                    }
                    Ok(None) => {
                        self.gaps
                            .push("socket_closed; process generation unknown".into());
                        changed = true;
                        break;
                    }
                    Err(_) => continue,
                };
                if matches!(
                    event.method.as_str(),
                    "Inspector.detached"
                        | "Inspector.targetCrashed"
                        | "Runtime.executionContextsCleared"
                        | "Runtime.executionContextDestroyed"
                        | "Page.frameNavigated"
                ) {
                    changed = true;
                }
                if event.method.starts_with("Runtime.")
                    || event.method.starts_with("Log.")
                    || event.method.starts_with("Page.")
                    || event.method.starts_with("Network.")
                    || event.method.starts_with("Inspector.")
                {
                    let item = json!({"method":event.method,"params":event.params});
                    let n = serde_json::to_vec(&item).map_err(|_| invalid())?.len();
                    if n > 32768 {
                        self.gaps.push("oversized_event_omitted".into());
                        changed = true;
                        break;
                    }
                    size += n;
                    records.push(item);
                }
                if changed {
                    break;
                }
            }
            if records.len() == 256 || size >= 256 * 1024 {
                self.gaps.push("event_budget_reached".into());
            }
            if changed {
                self.gaps
                    .push("page_or_context_replaced; rediscover; no reconnect".into());
            }
            self.stage = 3;
            return self.packet(
                "events",
                json!({"events":records,"generationChanged":changed,"loss":self.gaps}),
                true,
            );
        }
        self.packet("end", json!({}), true)
    }
    async fn discover(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        let value = super::inspector::get(&self.base, "json/list", self.budget()?).await?;
        let pages = pages(&self.base, &value)?;
        self.stage = 1;
        if self.options.mode == "discover" {
            self.stage = 3;
            return self.packet("discovery", json!({"pages":pages}), true);
        }
        let selected: Vec<_> = pages
            .into_iter()
            .filter(|p| {
                p["id"] == json!(self.options.page_id)
                    && p["url"] == json!(self.options.expected_url)
            })
            .collect();
        if selected.len() != 1 {
            return Err(Error::new(
                "EMBEDDED_TARGET_UNAVAILABLE",
                "Exact pageId and URL were not uniquely discovered; rediscover explicitly",
            ));
        }
        self.selected = selected[0].clone();
        self.packet("selection", self.selected.clone(), false)
    }
    async fn attach(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        let current = super::inspector::get(&self.base, "json/list", self.budget()?).await?;
        if !pages(&self.base, &current)?.contains(&self.selected) {
            return Err(Error::new(
                "EMBEDDED_TARGET_CHANGED",
                "Selected page changed before attachment; rediscover explicitly",
            ));
        }
        let socket = self.selected["webSocketDebuggerUrl"].as_str().unwrap();
        self.lease = Some(crate::lease::ControlLease::acquire(&format!(
            "embedded:{socket}"
        ))?);
        let deadline = tokio::time::Instant::now() + self.budget()?;
        let tcp = tokio::time::timeout_at(
            deadline,
            tokio::net::TcpStream::connect(super::inspector::address(&self.base)?),
        )
        .await
        .map_err(|_| Error::new("TIMEOUT", "Embedded socket timed out"))?
        .map_err(|_| {
            Error::new(
                "EMBEDDED_ENDPOINT_UNAVAILABLE",
                "Selected socket is unavailable",
            )
        })?;
        let (connection, events) = Connection::open_embedded(tcp, socket, deadline)
            .await
            .map_err(|_| Error::new("EMBEDDED_HANDSHAKE", "Selected page upgrade failed"))?;
        self.connection = Some(connection);
        self.events = Some(events);
        for method in [
            Some("Runtime.enable"),
            Some("Page.enable"),
            Some("Log.enable"),
            self.options.network.then_some("Network.enable"),
        ]
        .into_iter()
        .flatten()
        {
            if let Err(error) = self.call(method).await {
                if method == "Runtime.enable" {
                    return Err(error);
                }
                self.gaps.push(format!("unsupported:{method}"));
            }
        }
        self.stage = 2;
        self.packet("attachment",json!({"connected":true,"ownership":"borrowed page; attachment may affect other debuggers","inputPerformed":false}),false)
    }
    pub async fn settle(&mut self) -> Value {
        let closed = if let Some(c) = self.connection.take() {
            c.close(tokio::time::Instant::now() + Duration::from_secs(1))
                .await
                .is_ok()
        } else {
            true
        };
        self.events = None;
        self.lease = None;
        json!({"ownedSocketClosed":closed,"borrowedPageClosed":false,"endpointChanged":false,"nativeProcessState":"unverified"})
    }
}
