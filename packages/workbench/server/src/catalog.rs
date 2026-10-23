//! The page tree as the canvas reads it: the space's configuration with what
//! only this machine can add. The page index's previews and docs pages, the
//! address of a Storybook it finds running, the proxy origin of one this
//! server runs, booted iOS Simulators, and the docs pages' problems.
//!
//! A Storybook or Simulator that is not running leaves the other pages
//! usable and adds one problem. The answer is fresh on every request: the
//! configuration files are small, and an answer that lags an edit is never
//! worth the time saved.
mod answer;
mod import;
mod request;

pub(crate) use answer::IndexAnswer;
pub(crate) use import::{encode_component, page_items};

use crate::compiler::{Catalog as Built, Previews, Settings, lens_requests};
use crate::host::Host;
use crate::serve::{Output, Resident};
use anyhow::Result;
use import::Assembly;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Where a space's configuration is: the folder of its `workbench.yaml` and
/// its key there, or `None` for the file's first space.
#[derive(Clone, Debug)]
pub(crate) struct Where {
    pub(crate) dir: PathBuf,
    pub(crate) key: Option<String>,
}

pub(crate) struct Catalog {
    root: PathBuf,
    product: PathBuf,
    place: Where,
    trusted: bool,
    resident: Arc<Resident>,
    previews: Arc<Previews>,
    host: Arc<Host>,
    output: Output,
    /// The last complete index answer and its generation, reused while the
    /// resident index reports nothing changed.
    last: Mutex<Option<(String, IndexAnswer)>>,
    /// Docs pages definitions declared, as the last assembly found them.
    discovered: Mutex<Vec<Value>>,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Definitions discovery found that the index could not list, from the
/// errors it reports for them: they build to show those errors.
fn declared(index: &IndexAnswer) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for error in &index.errors {
        let Some((file, _)) = error.split_once(": ") else {
            continue;
        };
        if (file.ends_with(".workbench.ts") || file.ends_with(".workbench.tsx"))
            && !files.iter().any(|known| known == file)
        {
            files.push(file.to_owned());
        }
    }
    files
}

impl Catalog {
    #[expect(
        clippy::too_many_arguments,
        reason = "the catalog is composed from the server's services at startup"
    )]
    pub(crate) fn new(
        root: &Path,
        product: &Path,
        place: Where,
        trusted: bool,
        resident: Arc<Resident>,
        previews: Arc<Previews>,
        host: Arc<Host>,
        output: Output,
    ) -> Self {
        // Project code runs on the host's Node runtime, asked for when a
        // definition or the compiler first needs it.
        let asking = host.clone();
        previews.provide_runtime(Arc::new(move || {
            let asking = asking.clone();
            Box::pin(async move { asking.node().await })
        }));
        Self {
            root: root.to_path_buf(),
            product: product.to_path_buf(),
            place,
            trusted,
            resident,
            previews,
            host,
            output,
            last: Mutex::new(None),
            discovered: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn place(&self) -> &Where {
        &self.place
    }

    pub(crate) fn trusted(&self) -> bool {
        self.trusted
    }

    /// The configuration as the files say, without the index: `None`
    /// without a `workbench.yaml`.
    ///
    /// # Errors
    /// Returns the reader's message when a file can't be read or parsed.
    pub(crate) fn resolved(&self) -> Result<Option<Value>> {
        let view = crate::config::answer(&self.place.dir, self.place.key.as_deref())?;
        Ok((!view.is_null()).then_some(view))
    }

    /// The page index’s last complete answer, for builds of what it lists.
    pub(crate) fn last_index(&self) -> IndexAnswer {
        lock(&self.last)
            .as_ref()
            .map(|(_, answer)| answer.clone())
            .unwrap_or_default()
    }

    /// Docs pages definitions declared, as the last assembly found them.
    pub(crate) fn discovered_docs(&self) -> Vec<Value> {
        lock(&self.discovered).clone()
    }

    /// The page index's answer for these preview settings, asking the host
    /// for the Node runtime only when a definition needs evaluating.
    async fn index(&self, options: &Value) -> Result<(IndexAnswer, Value)> {
        let cached = lock(&self.last)
            .as_ref()
            .map(|(generation, _)| generation.clone())
            .unwrap_or_default();
        let (root, product, options_owned) =
            (self.root.clone(), self.product.clone(), options.clone());
        let mut ask = tokio::task::spawn_blocking(move || {
            index_request(&root, &product, &options_owned, None, &cached)
        })
        .await??;
        let mut response = self.resident.index(ask.clone()).await?;
        let mut first = None;
        if response.needs_evaluator() {
            first = Some(response.timings().clone());
            ask.node = self.host.node().await?;
            response = self.resident.index(ask).await?;
        }
        let mut timings = response.timings().clone();
        if let Some(first) = first {
            merge_timings(&mut timings, &first);
        }
        if response.reuses()
            && let Some((generation, answer)) = lock(&self.last).as_ref()
            && Some(generation.as_str()) == response.generation()
        {
            return Ok((answer.clone(), timings));
        }
        let answer = read_response(&response)?;
        if let Some(generation) = response.generation() {
            *lock(&self.last) = Some((generation.to_owned(), answer.clone()));
        }
        Ok((answer, timings))
    }

    /// Previews and docs pages from the index, with the compiler told what
    /// to build. Problems join the assembly's; nothing here fails the answer.
    async fn import_previews(&self, view: &Value, into: &mut Assembly) {
        lock(&self.discovered).clear();
        if view["previews"] == Value::Bool(false) {
            self.previews.configure(None).await;
            return;
        }
        let options = if view["previews"].is_object() {
            view["previews"].clone()
        } else {
            json!({})
        };
        if !self.trusted {
            if self.has_definitions(&options).await {
                into.problems
                    .push("Workbench previews require a trusted workspace.".into());
            }
            return;
        }
        let began = Instant::now();
        let (index, timings) = match self.index(&options).await {
            Ok(found) => found,
            Err(error) => {
                into.problems.push(format!("Workbench previews: {error:#}"));
                return;
            }
        };
        // The compiler starts nothing until a preview or docs lens is asked for.
        self.previews
            .configure(Some(Settings {
                options: options.clone(),
                node: None,
                catalog: compiler_catalog(view, &index),
            }))
            .await;
        into.problems.extend(self.previews.problems().await);
        let elapsed = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut details = timings.as_object().cloned().unwrap_or_default();
        details.insert("elapsedMs".into(), json!(elapsed));
        details.insert("previews".into(), json!(index.previews.len()));
        details.insert("docs".into(), json!(index.docs.len()));
        details.insert("errors".into(), json!(index.errors.len()));
        let _sent = self
            .output
            .diagnostic("previews.indexed", Value::Object(details))
            .await;
        into.problems.extend(index.errors.iter().cloned());
        import::import(view, &index, &self.root, into);
        lock(&self.discovered).clone_from(&into.discovered_docs);
    }

    /// Whether the space has any preview or docs definition, without running them.
    async fn has_definitions(&self, options: &Value) -> bool {
        let root = self.root.clone();
        let options = options.clone();
        tokio::task::spawn_blocking(move || {
            let Ok(includes) = crate::discovery::Includes::read(&options) else {
                return false;
            };
            crate::discovery::discover(
                &root,
                &std::collections::HashMap::new(),
                true,
                includes.native(),
            )
            .is_ok_and(|found| !found.files.is_empty())
        })
        .await
        .unwrap_or(false)
    }

    /// The configuration with the index's pages and the machine's
    /// implementations: `None` without a `workbench.yaml`.
    ///
    /// # Errors
    /// Returns the reader's message when the configuration can't be read.
    pub(crate) async fn assemble(&self, extras: &dyn Extras) -> Result<Option<Value>> {
        let Some(view) = self.resolved()? else {
            return Ok(None);
        };
        let mut into = Assembly {
            view_collections: view["collections"].as_array().cloned().unwrap_or_default(),
            pages: view["pages"].as_object().cloned().unwrap_or_default(),
            problems: view["problems"]
                .as_array()
                .map(|problems| {
                    problems
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            implementations: view["implementations"]
                .as_object()
                .cloned()
                .unwrap_or_default(),
            ..Assembly::default()
        };
        self.import_previews(&view, &mut into).await;
        extras
            .urls(&mut into.implementations, &mut into.problems)
            .await;
        extras
            .storybooks(&view, &mut into.implementations, &mut into.problems)
            .await;
        extras
            .simulators(
                &mut into.implementations,
                &mut into.collections,
                &mut into.pages,
                &mut into.problems,
            )
            .await;
        let mut assembled = view;
        assembled["collections"] = Value::Array(into.view_collections);
        let discovered = into.discovered_docs.clone();
        extras
            .docs(&assembled, &discovered, &mut into.pages, &mut into.problems)
            .await;
        assembled["implementations"] = Value::Object(into.implementations);
        assembled["catalogCollections"] = Value::Array(into.collections);
        assembled["pages"] = crate::config::ordered(into.pages);
        assembled["problems"] = json!(into.problems);
        Ok(Some(assembled))
    }
}

/// The page-index request for a space: its saved index, and the identity
/// that invalidates it when the binary, the evaluator or the settings change.
///
/// # Errors
/// Fails when the saved index's folder can't be made.
pub(crate) fn index_request(
    root: &Path,
    product: &Path,
    options: &Value,
    node: Option<&Path>,
    cached: &str,
) -> Result<crate::model::Request> {
    let saved = request::saved_path(&std::env::temp_dir().join("canonic-workbench-index"), root)?;
    let node_dir = product.join("server/node");
    let evaluator = node_dir.join("preview/evaluate-definitions.cjs");
    let files = [
        std::env::current_exe().unwrap_or_default(),
        evaluator.clone(),
        node_dir.join("preview/compiler.cjs"),
        product.join("sdk/api.js"),
    ];
    let identity = request::identity(&files, options);
    Ok(request::request(
        root, saved, identity, evaluator, node, options, cached,
    ))
}

/// The index's records as previews, docs pages and problems.
///
/// # Errors
/// Fails when a record's entry is not a definition's.
pub(crate) fn read_response(response: &crate::protocol::Response) -> Result<IndexAnswer> {
    let records = response
        .records()
        .iter()
        .map(|listed| answer::Record {
            file: &listed.file,
            docs: listed.docs,
            entry: listed.entry.as_deref().map_err(String::as_str),
        })
        .collect();
    answer::read(records)
}

/// What the compiler builds for a space: the index's previews, the
/// definitions it could not list, each docs lens, and every docs page.
pub(crate) fn compiler_catalog(view: &Value, index: &IndexAnswer) -> Built {
    let mut pages: Vec<String> = crate::docs::docs_pages(view, &[])
        .into_iter()
        .map(|page| page.src)
        .collect();
    pages.extend(
        index
            .docs
            .iter()
            .filter_map(|page| page["src"].as_str().map(str::to_owned)),
    );
    Built {
        previews: index
            .previews
            .iter()
            .filter_map(|preview| preview["file"].as_str().map(str::to_owned))
            .collect(),
        declared: declared(index),
        lenses: lens_requests(&index.docs),
        pages,
    }
}

/// What the catalog adds from other services: Storybook addresses, the
/// Simulator catalog and the docs pages' sources and problems.
#[async_trait::async_trait]
pub(crate) trait Extras: Send + Sync {
    async fn urls(&self, _implementations: &mut Map<String, Value>, _problems: &mut Vec<String>) {}
    async fn storybooks(
        &self,
        view: &Value,
        implementations: &mut Map<String, Value>,
        problems: &mut Vec<String>,
    );
    async fn simulators(
        &self,
        implementations: &mut Map<String, Value>,
        collections: &mut Vec<Value>,
        pages: &mut Map<String, Value>,
        problems: &mut Vec<String>,
    );
    async fn docs(
        &self,
        view: &Value,
        discovered: &[Value],
        pages: &mut Map<String, Value>,
        problems: &mut Vec<String>,
    );
}

/// Adds the first pass's timings to the second's, as the Node index client did.
fn merge_timings(timings: &mut Value, first: &Value) {
    let Some(first) = first.as_object() else {
        return;
    };
    let Some(out) = timings.as_object_mut() else {
        return;
    };
    for (key, value) in first {
        if let Some(number) = value.as_f64()
            && (key.ends_with("Ms") || key == "parsed" || key == "directoriesRead")
        {
            let sum = number + out.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            out.insert(key.clone(), json!(sum));
        }
    }
    if let Some(reused) = first.get("reused") {
        out.insert("reused".into(), reused.clone());
    }
    out.insert("passes".into(), json!(2));
    if let Some(error) = first.get("saveError") {
        out.insert("saveError".into(), error.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_that_could_not_be_listed_are_declared_once() {
        let index = IndexAnswer {
            errors: vec![
                "src/button.workbench.tsx: Unexpected token".into(),
                "src/button.workbench.tsx: Duplicate preview id: button".into(),
                "docs/guide.md: missing".into(),
            ],
            ..IndexAnswer::default()
        };
        assert_eq!(declared(&index), ["src/button.workbench.tsx"]);
    }

    #[test]
    fn two_passes_add_their_timings() {
        let mut timings = json!({ "walkMs": 2, "parsed": 1, "other": 5 });
        merge_timings(
            &mut timings,
            &json!({ "walkMs": 3, "parsed": 4, "other": 9, "reused": true }),
        );
        assert_eq!(
            timings,
            json!({ "walkMs": 5.0, "parsed": 5.0, "other": 5, "reused": true, "passes": 2 })
        );
    }
}
