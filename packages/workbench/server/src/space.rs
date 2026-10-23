//! One space's server, composed: the catalog and the services the routes
//! use, with what the catalog adds from Storybook and the iOS Simulators.
//!
//! Storybooks this server runs are started on the first request through
//! their proxy and stopped with the server. A Storybook found running at
//! `url: auto` keeps its address for the session.
mod docs;
mod export;

pub(crate) use export::SpaceExport;

use crate::catalog::{Catalog, Extras};
use crate::compiler::Previews;
use crate::diagnostics::Diagnostics;
use crate::docs::service::DocsService;
use crate::host::Host;
use crate::proxies::Proxies;
use crate::simulators::{self, Device, Testrun};
use crate::storybook::{self, OwnedStorybook};
use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The Storybooks this server runs, by implementation key and folder.
pub(crate) type Storybooks = Arc<Mutex<HashMap<String, OwnedStorybook>>>;

/// Lists booted Simulators; the real one runs `simctl`.
pub(crate) type ListDevices =
    Arc<dyn Fn() -> BoxFuture<'static, Result<Vec<Device>>> + Send + Sync>;

/// Settings the host starts the server with.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    /// Opt-in server and browser performance diagnostics.
    pub(crate) trace: bool,
    /// This space's id in the host's list.
    pub(crate) id: String,
    /// The folder of its workbench.yaml and its key there, which describe
    /// it when the host lists no other spaces.
    pub(crate) dir: std::path::PathBuf,
    pub(crate) key: Option<String>,
    /// Whether the host receives handoffs.
    pub(crate) handoff: bool,
    pub(crate) export_retention_ms: Option<u64>,
    pub(crate) export_max_archive_bytes: Option<u64>,
    /// How long a canvas view stays current without a report, in milliseconds.
    pub(crate) view_stale_ms: Option<f64>,
}

pub(crate) struct Space {
    pub(crate) catalog: Catalog,
    pub(crate) host: Arc<Host>,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) proxies: Arc<Proxies>,
    pub(crate) settings: Settings,
    pub(crate) testrun: Testrun,
    /// The canvas view agents read, and the file announcing this server.
    pub(crate) agent: crate::agent_view::AgentView,
    pub(crate) product: PathBuf,
    pub(crate) port: u16,
    pub(crate) previews: Arc<Previews>,
    pub(crate) docs: DocsService<docs::NativeDocs>,
    listings: docs::Listings,
    /// The design export’s jobs.
    pub(crate) exports: Arc<crate::export::Jobs>,
    storybooks: Storybooks,
    /// The annotation layer’s styles, for captures of pages the workbench doesn’t serve.
    annotations: String,
    detected: Mutex<HashMap<String, String>>,
    scope: crate::scope::Discovery,
    devices: ListDevices,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The origin of an http(s) address, as `new URL(address).origin` gives it.
///
/// Parsed by the WHATWG rules the capture helper loads it with, so an
/// address can't name one host here and reach another there.
pub(crate) fn origin_of(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let origin = url.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

/// An implementation's address: its `base`, else its `url`.
fn address(implementation: &Value) -> Option<&str> {
    ["base", "url"].into_iter().find_map(|field| {
        implementation[field]
            .as_str()
            .filter(|text| !text.is_empty())
    })
}

impl Space {
    #[expect(
        clippy::too_many_arguments,
        reason = "the space is composed from the server's services at startup"
    )]
    pub(crate) fn new(
        catalog: Catalog,
        host: Arc<Host>,
        diagnostics: Diagnostics,
        proxies: Arc<Proxies>,
        storybooks: Storybooks,
        settings: Settings,
        product: &Path,
        port: u16,
        previews: Arc<Previews>,
    ) -> Self {
        let testrun = Testrun::new(catalog.root());
        let (trusted, place) = (catalog.trusted(), catalog.place().clone());
        let blocked: docs::Blocked = Arc::new(move || {
            let view = crate::config::answer(&place.dir, place.key.as_deref())
                .ok()
                .filter(|view| !view.is_null());
            docs::docs_blocked(trusted, view.as_ref())
        });
        let docs = DocsService::new(docs::NativeDocs::new(
            catalog.root(),
            previews.clone(),
            blocked,
        ));
        let exports = crate::export::Jobs::new(
            settings
                .export_retention_ms
                .map(std::time::Duration::from_millis),
        );
        let agent = crate::agent_view::AgentView::new(catalog.root(), settings.view_stale_ms);
        Self {
            agent,
            catalog,
            host,
            diagnostics,
            proxies,
            settings,
            testrun,
            product: product.to_path_buf(),
            port,
            previews,
            docs,
            listings: docs::Listings::default(),
            exports,
            storybooks,
            annotations: std::fs::read_to_string(product.join("ui/legacy/annotations.css"))
                .unwrap_or_default(),
            detected: Mutex::new(HashMap::new()),
            scope: crate::scope::Discovery::default(),
            devices: Arc::new(|| Box::pin(simulators::booted())),
        }
    }

    pub(crate) fn annotations_css(&self) -> &str {
        &self.annotations
    }

    pub(crate) async fn devices(&self) -> Result<Vec<Device>> {
        (self.devices)().await
    }

    /// The configuration with every page this machine adds.
    ///
    /// # Errors
    /// Returns the reader's message when the configuration can't be read.
    pub(crate) async fn view(&self) -> Result<Option<Value>> {
        self.catalog.assemble(self).await
    }

    /// Refresh external discovery only for the explicit configuration request.
    pub(crate) async fn refreshed_view(&self) -> Result<Option<Value>> {
        if let Some(view) = self.catalog.resolved()? {
            self.scope
                .refresh(self.catalog.root(), self.catalog.trusted(), &view)
                .await;
        }
        self.view().await
    }

    /// The view as the browser gets it: URL and Storybook implementations at
    /// their proxies, the upstream address kept. The server itself keeps
    /// talking to them directly.
    pub(crate) async fn proxied(&self, mut view: Value) -> Value {
        let Some(implementations) = view["implementations"].as_object_mut() else {
            return view;
        };
        for (key, implementation) in implementations.iter_mut() {
            if implementation["owned"] == true {
                continue;
            }
            let field = match implementation["kind"].as_str() {
                Some("url") => "base",
                Some("storybook") => "url",
                _ => continue,
            };
            let Some(upstream) = implementation[field].as_str().map(str::to_owned) else {
                continue;
            };
            let lower = upstream.to_ascii_lowercase();
            if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                continue;
            }
            match self.proxies.address(&upstream).await {
                Ok(at_proxy) => {
                    implementation["upstream"] = json!(upstream);
                    implementation[field] = json!(at_proxy);
                }
                Err(error) => {
                    self.diagnostics
                        .warn(
                            "proxy.failed",
                            json!({ "implementation": key, "message": format!("{error:#}") }),
                        )
                        .await;
                }
            }
        }
        view
    }

    /// The origins a page capture may load: every implementation's, and
    /// every proxy's.
    pub(crate) fn origins(&self) -> Vec<String> {
        let view = self
            .catalog
            .resolved()
            .ok()
            .flatten()
            .unwrap_or(Value::Null);
        let detected = lock(&self.detected).clone();
        let mut out: Vec<String> = view["implementations"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(key, implementation)| {
                let auto = implementation["kind"] == "storybook" && implementation["auto"] == true;
                match detected.get(key) {
                    Some(found) if auto => origin_of(found),
                    _ => address(implementation).and_then(origin_of),
                }
            })
            .collect();
        out.extend(self.proxies.origins());
        out
    }

    /// The proxy origin of a Storybook this server runs, starting nothing yet.
    async fn owned_storybook(&self, key: &str, implementation: &Value) -> Result<String> {
        let root = implementation["root"].as_str().ok_or_else(|| {
            anyhow!("Workbench runs Storybook in the implementation’s root, and it has none.")
        })?;
        let id = format!("{key}|{root}");
        let known = lock(&self.storybooks).contains_key(&id);
        if !known {
            let node = self.host.node().await?;
            let preload = self
                .product
                .join("server/node/src/modules/storybook/socket-listen.cjs");
            let mut options = storybook::Options::new(PathBuf::from(root), node, preload);
            let diagnostics = self.diagnostics.clone();
            options.log = Some(Arc::new(move |event: &str, details: Value| {
                let (diagnostics, event) = (diagnostics.clone(), event.to_owned());
                tokio::spawn(async move { diagnostics.info(&event, details).await });
            }));
            lock(&self.storybooks)
                .entry(id.clone())
                .or_insert_with(|| OwnedStorybook::new(options));
        }
        self.proxies.owned(&id).await
    }

    /// Stops every Storybook this server runs.
    pub(crate) async fn close(&self) {
        let running: Vec<OwnedStorybook> = lock(&self.storybooks)
            .drain()
            .map(|(_, storybook)| storybook)
            .collect();
        for storybook in running {
            storybook.close().await;
        }
    }
}

/// The proxies' sockets for owned implementations: a Storybook this server
/// runs, started for the proxy's port.
pub(crate) fn sockets(storybooks: Storybooks) -> crate::proxies::Sockets {
    Arc::new(move |key: String, port: u16| {
        let storybook = lock(&storybooks).get(&key).cloned();
        Box::pin(async move {
            match storybook {
                Some(storybook) => storybook.socket(port).await,
                None => Err(anyhow!("No implementation {key} runs here.")),
            }
        })
    })
}

#[async_trait::async_trait]
impl Extras for Space {
    async fn urls(&self, implementations: &mut Map<String, Value>, _problems: &mut Vec<String>) {
        self.scope.resolve(implementations).await;
    }
    async fn storybooks(
        &self,
        _view: &Value,
        implementations: &mut Map<String, Value>,
        problems: &mut Vec<String>,
    ) {
        let root = self.catalog.root().to_path_buf();
        let keys: Vec<String> = implementations.keys().cloned().collect();
        for key in &keys {
            let implementation = &implementations[key];
            if implementation["kind"] != "storybook" || implementation["auto"] != true {
                continue;
            }
            let known = lock(&self.detected).get(key).cloned();
            let found = match known {
                Some(address) => Ok(address),
                None => storybook::detect(&root).await,
            };
            match found {
                Ok(address) => {
                    lock(&self.detected).insert(key.clone(), address.clone());
                    implementations[key]["url"] = json!(address);
                    implementations[key]["auto"] = json!(true);
                }
                Err(error) => problems.push(format!("implementations › {key}: {error:#}")),
            }
        }
        // A Storybook this server runs is addressed at its proxy and starts
        // on the first request there. It runs project code, so only in a
        // trusted workspace.
        for key in &keys {
            if implementations[key]["kind"] != "storybook" || implementations[key]["owned"] != true
            {
                continue;
            }
            if !self.catalog.trusted() {
                problems.push(format!("implementations › {key}: Workbench runs Storybook in a trusted workspace only, since Storybook runs project code."));
                continue;
            }
            let implementation = implementations[key].clone();
            match self.owned_storybook(key, &implementation).await {
                Ok(origin) => implementations[key]["url"] = json!(origin),
                Err(error) => problems.push(format!("implementations › {key}: {error:#}")),
            }
        }
    }

    async fn simulators(
        &self,
        implementations: &mut Map<String, Value>,
        collections: &mut Vec<Value>,
        pages: &mut Map<String, Value>,
        problems: &mut Vec<String>,
    ) {
        let keys: Vec<String> = implementations
            .iter()
            .filter(|(_, implementation)| {
                implementation["kind"] == "ios-simulator"
                    && (implementation["catalog"] == true
                        || implementation["catalog"]
                            .as_str()
                            .is_some_and(|catalog| !catalog.is_empty()))
            })
            .map(|(key, _)| key.clone())
            .collect();
        if keys.is_empty() {
            return;
        }
        match self.devices().await {
            Ok(devices) => {
                for key in keys {
                    let imported = simulators::catalog(&devices, &key, &implementations[&key]);
                    if imported.collections.is_empty() {
                        problems.push(format!(
                            "implementations › {key}: no matching booted iOS Simulator was found."
                        ));
                    }
                    collections.extend(imported.collections);
                    pages.extend(imported.pages);
                }
            }
            Err(error) => problems.push(format!("iOS Simulator: {error:#}")),
        }
    }

    async fn docs(
        &self,
        view: &Value,
        discovered: &[Value],
        pages: &mut Map<String, Value>,
        problems: &mut Vec<String>,
    ) {
        docs::import(self, view, discovered, pages, problems);
    }
}

#[cfg(test)]
mod tests {
    use super::origin_of;

    #[test]
    fn origins_are_scheme_and_host_without_default_ports() {
        assert_eq!(
            origin_of("http://127.0.0.1:6006/iframe.html").as_deref(),
            Some("http://127.0.0.1:6006")
        );
        assert_eq!(
            origin_of("HTTPS://Example.com:443/a").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            origin_of("http://user@localhost:80").as_deref(),
            Some("http://localhost")
        );
        assert_eq!(origin_of("/relative"), None);
        assert_eq!(origin_of("ftp://example.com"), None);
        assert_eq!(
            origin_of("http://[::1]:6006/").as_deref(),
            Some("http://[::1]:6006")
        );
    }

    #[test]
    fn origins_follow_the_host_a_browser_would_load() {
        for (address, origin) in [
            (
                "http://evil.example\\@127.0.0.1:6006/",
                "http://evil.example",
            ),
            (
                "http://127.0.0.1:6006\\@evil.example/",
                "http://127.0.0.1:6006",
            ),
            ("http://a@b@evil.example/", "http://evil.example"),
            (
                "http://evil.example?@127.0.0.1:6006/",
                "http://evil.example",
            ),
            ("http://EVIL.example:80/", "http://evil.example"),
        ] {
            assert_eq!(origin_of(address).as_deref(), Some(origin), "{address}");
        }
        assert_eq!(origin_of("http://"), None);
        assert_eq!(origin_of("http://exa mple.com/"), None);
    }
}
