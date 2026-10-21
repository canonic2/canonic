//! Docs pages in this space: their examples listed and built by the native
//! compiler, their entries and sources added to the page tree, their
//! problems in the shared list, and their examples named in handoffs.
//!
//! The page tree never waits for a listing: each docs lens's examples are
//! listed again in the background whenever the tree is asked for, and a
//! listing that turns out different tells the host the catalog changed.
use super::Space;
use crate::compiler::Previews;
use crate::docs::service::DocsSource;
use crate::docs::{
    DocsBundle, DocsLens, DocsPageEntry, ListedExample, Listing, docs_pages, lens_request,
};
use crate::host::Host;
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Why examples can't run here, or `None`; asked for every page.
pub(crate) type Blocked = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// The native compiler as the docs service's source of files and examples.
#[derive(Clone)]
pub(crate) struct NativeDocs {
    root: PathBuf,
    previews: Arc<Previews>,
    blocked: Blocked,
}

impl NativeDocs {
    pub(crate) fn new(root: &Path, previews: Arc<Previews>, blocked: Blocked) -> Self {
        Self {
            root: root.to_path_buf(),
            previews,
            blocked,
        }
    }
}

fn examples(value: &Value) -> Vec<ListedExample> {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

impl DocsSource for NativeDocs {
    fn read_file(&self, file: &str) -> Result<Option<String>> {
        if file.split(['/', '\\']).any(|part| part == "..") || Path::new(file).is_absolute() {
            return Ok(None);
        }
        let target = self.root.join(file);
        match std::fs::read_to_string(&target) {
            Ok(text) => Ok(Some(text)),
            Err(_) if !target.is_file() => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn blocked(&self) -> Option<String> {
        (self.blocked)()
    }

    fn list_examples(&self, lens: &DocsLens) -> impl Future<Output = Result<Listing>> + Send {
        let (previews, request) = (self.previews.clone(), lens_request(lens));
        async move {
            let listed = previews.docs_index(&request).await?;
            Ok(Listing {
                examples: examples(&listed["examples"]),
                problems: strings(&listed["problems"]),
            })
        }
    }

    fn bundle(&self, lens: &DocsLens) -> impl Future<Output = Result<DocsBundle>> + Send {
        let (previews, request) = (self.previews.clone(), lens_request(lens));
        async move {
            let built = previews.docs_bundle(&request).await?;
            Ok(DocsBundle {
                module: built["module"].as_str().unwrap_or_default().to_owned(),
                stylesheet: built["stylesheet"].as_str().map(str::to_owned),
                revision: built["revision"].as_str().unwrap_or_default().to_owned(),
                examples: examples(&built["examples"]),
                problems: strings(&built["problems"]),
            })
        }
    }
}

/// One lens's examples as last listed, and whether a listing is running.
#[derive(Default)]
pub(crate) struct Known {
    result: Option<Result<Listing, String>>,
    pending: bool,
}

/// Each lens's examples as last listed, by its compiler request.
pub(crate) type Listings = Arc<Mutex<HashMap<String, Known>>>;

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The docs pages of `view`, with the pages definitions declared.
pub(crate) fn pages_of(view: &Value, discovered: &[Value]) -> Vec<DocsPageEntry> {
    let discovered: Vec<DocsPageEntry> = discovered
        .iter()
        .filter_map(|entry| serde_json::from_value(entry.clone()).ok())
        .collect();
    docs_pages(view, &discovered)
}

/// Why examples can't run in a space: untrusted, or previews turned off.
pub(crate) fn docs_blocked(trusted: bool, view: Option<&Value>) -> Option<String> {
    if !trusted {
        return Some(
            "Examples run project code, so they appear in a trusted workspace only.".into(),
        );
    }
    if view.is_some_and(|view| view["previews"] == false) {
        return Some("Examples run project code, which previews: false turns off.".into());
    }
    None
}

/// Lists a lens again, telling the host when the listing changed.
async fn refresh(
    listings: &Listings,
    source: &NativeDocs,
    host: &Host,
    key: &str,
    lens: &DocsLens,
) {
    if !source.previews.enabled().await {
        if let Some(entry) = lock(listings).get_mut(key) {
            entry.pending = false;
        }
        return;
    }
    let listed = source
        .list_examples(lens)
        .await
        .map_err(|error| format!("{error:#}"));
    let changed = {
        let mut listings = lock(listings);
        let entry = listings.entry(key.to_owned()).or_default();
        entry.pending = false;
        let changed = entry.result.as_ref() != Some(&listed);
        entry.result = Some(listed);
        changed
    };
    if changed {
        host.catalog_changed().await;
    }
}

impl Space {
    /// Why docs examples can't run: they run project code, like previews.
    pub(crate) fn docs_blocked(&self) -> Option<String> {
        docs_blocked(
            self.catalog.trusted(),
            self.catalog.resolved().ok().flatten().as_ref(),
        )
    }

    /// The docs pages as the last assembly found them.
    pub(crate) fn docs_pages(&self, view: &Value) -> Vec<DocsPageEntry> {
        pages_of(view, &self.catalog.discovered_docs())
    }

    /// The configuration, with previews configured when a docs lens needs
    /// them: a docs page can be the first thing anyone asks this server for.
    ///
    /// # Errors
    /// Returns the reader's message when the configuration can't be read.
    pub(crate) async fn docs_ready(&self) -> Result<Option<Value>> {
        let _phase = crate::trace::phase("docs-ready");
        if !self.previews.enabled().await {
            let _assembled = self.view().await?;
        }
        self.catalog.resolved()
    }

    /// A lens's listing as last known, starting another in the background.
    fn known_listing(&self, lens: &DocsLens) -> Option<Result<Listing, String>> {
        let key = lens_request(lens).to_string();
        let mut listings = lock(&self.listings);
        let entry = listings.entry(key.clone()).or_default();
        let known = entry.result.clone();
        if !entry.pending {
            entry.pending = true;
            let (listings, source, host, lens) = (
                self.listings.clone(),
                self.docs.source().clone(),
                self.host.clone(),
                lens.clone(),
            );
            tokio::spawn(async move { refresh(&listings, &source, &host, &key, &lens).await });
        }
        known
    }

    /// Names, in a handoff from a docs lens, the Markdown and where each
    /// example in view is. Examples that can't be listed keep their IDs.
    pub(crate) async fn docs_handoff(&self, canvas: &mut Value) {
        if !canvas["docs"].is_object() {
            return;
        }
        let Ok(Some(view)) = self.catalog.resolved() else {
            return;
        };
        let src = canvas["src"].as_str().unwrap_or_default().to_owned();
        let pages = self.docs_pages(&view);
        let Some(page) = pages.iter().find(|candidate| candidate.page == src) else {
            return;
        };
        let lens = crate::docs::choose_lens(page, canvas["docs"]["lens"].as_str());
        canvas["docs"]["markdown"] = json!(page.src);
        canvas["docs"]["lensLabel"] = lens.map_or(Value::Null, |lens| json!(lens.label));
        let Some(lens) = lens else {
            return;
        };
        if self.docs_blocked().is_some() || !self.previews.enabled().await {
            return;
        }
        let Ok(listed) = self.docs.source().list_examples(lens).await else {
            return;
        };
        if let Some(examples) = canvas["docs"]["examples"].as_array_mut() {
            for example in examples {
                let id = example["id"].as_str().unwrap_or_default();
                if let Some(found) = listed.examples.iter().find(|candidate| candidate.id == id) {
                    let named = if found.export == "default" {
                        String::new()
                    } else {
                        format!(" ({})", found.export)
                    };
                    example["file"] = json!(format!("{}{named}", found.file));
                }
            }
        }
    }

    /// Lists every docs lens and builds its bundle, one at a time, so the
    /// first visit to a page is already built.
    pub(crate) async fn warm_docs(&self) {
        let began = Instant::now();
        if self.settings.trace {
            self.diagnostics.info("docs.warm.started", json!({})).await;
        }
        let mut built = 0;
        let result: Result<()> = async {
            let Some(view) = self.docs_ready().await? else {
                return Ok(());
            };
            if self.docs_blocked().is_some() || !self.previews.enabled().await {
                return Ok(());
            }
            for page in self.docs_pages(&view) {
                for lens in &page.lenses {
                    let key = lens_request(lens).to_string();
                    let ((), bundled) = tokio::join!(
                        refresh(&self.listings, self.docs.source(), &self.host, &key, lens),
                        self.docs.source().bundle(lens)
                    );
                    if bundled.is_ok() {
                        built += 1;
                    }
                }
            }
            Ok(())
        }
        .await;
        let elapsed = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok(()) => {
                self.diagnostics
                    .info(
                        "docs.warmed",
                        json!({ "bundles": built, "elapsedMs": elapsed }),
                    )
                    .await;
            }
            Err(error) => {
                self.diagnostics
                    .warn(
                        "docs.warm.failed",
                        json!({ "elapsedMs": elapsed, "message": format!("{error:#}") }),
                    )
                    .await;
            }
        }
    }
}

/// Adds each docs page's sources to its page entry, and the docs problems.
pub(crate) fn import(
    space: &Space,
    view: &Value,
    discovered: &[Value],
    pages: &mut Map<String, Value>,
    problems: &mut Vec<String>,
) {
    let entries = pages_of(view, discovered);
    if entries.is_empty() {
        return;
    }
    let root = space.catalog.root();
    let exists = |relative: &str| root.join(relative).exists();
    let text = |path: PathBuf| path.to_string_lossy().into_owned();
    for page in &entries {
        let mut entry = pages
            .get(&page.page)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let markdown = root.join(&page.src);
        let mut code = entry
            .get("code")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if page.page == page.src {
            entry.insert("design".into(), json!(text(markdown)));
        } else {
            code.push(json!({ "implementation": null, "label": "Docs", "path": text(markdown), "relative": page.src, "exists": exists(&page.src) }));
        }
        for lens in &page.lenses {
            code.push(json!({ "implementation": lens.key, "path": text(root.join(&lens.examples)), "relative": lens.examples, "exists": exists(&lens.examples) }));
        }
        entry.insert("code".into(), Value::Array(code));
        pages.insert(page.page.clone(), Value::Object(entry));
    }
    match space
        .docs
        .problems_with(&entries, |lens| space.known_listing(lens))
    {
        Ok(found) => problems.extend(found),
        Err(error) => problems.push(format!("Docs pages: {error:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_are_blocked_untrusted_or_with_previews_off() {
        assert!(
            docs_blocked(false, None)
                .unwrap()
                .contains("trusted workspace")
        );
        assert!(
            docs_blocked(true, Some(&json!({ "previews": false })))
                .unwrap()
                .contains("previews: false")
        );
        assert_eq!(docs_blocked(true, Some(&json!({ "previews": {} }))), None);
    }
}
