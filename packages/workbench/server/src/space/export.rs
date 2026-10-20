//! The design export in this space: captures taken by the host's capture
//! helper, docs pages planned from their Markdown and listed examples,
//! stories read from the Storybook, and portable previews built by the
//! native compiler.
use super::Space;
use crate::docs::service::DocsSource as _;
use crate::export::{
    CapturePool, CaptureWorker, Capturer, DocsLensPlan, DocsPagePlan, ExportHost, PortableBuilder,
    PortableExport, Selection, Story,
};
use crate::host::Host;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// The host's capture helper, or one renderer of an export's pool.
struct HostCapture {
    host: Arc<Host>,
    worker: Option<String>,
}

#[async_trait]
impl CaptureWorker for HostCapture {
    async fn capture(&self, base_url: &str, payload: &Value) -> Result<Vec<u8>> {
        Ok(self
            .host
            .capture("capture", base_url, payload, self.worker.as_deref())
            .await?
            .body)
    }

    async fn capture_export_page(&self, payload: &Value) -> Result<Vec<u8>> {
        Ok(self
            .host
            .capture("captureExportPage", "", payload, self.worker.as_deref())
            .await?
            .body)
    }

    async fn print_export_page(&self, payload: &Value) -> Result<Vec<u8>> {
        Ok(self
            .host
            .capture("printExportPage", "", payload, self.worker.as_deref())
            .await?
            .body)
    }
}

struct HostPool {
    host: Arc<Host>,
    names: Vec<String>,
}

#[async_trait]
impl CapturePool for HostPool {
    fn workers(&self) -> Vec<Arc<dyn CaptureWorker>> {
        self.names
            .iter()
            .map(|name| {
                Arc::new(HostCapture {
                    host: self.host.clone(),
                    worker: Some(name.clone()),
                }) as Arc<dyn CaptureWorker>
            })
            .collect()
    }

    async fn close(&self) -> Result<()> {
        self.host.close_capture_pool(&self.names).await
    }
}

#[async_trait]
impl Capturer for HostCapture {
    async fn pool(&self, size: usize) -> Result<Option<Box<dyn CapturePool>>> {
        let names = self.host.capture_pool(size).await?;
        Ok(Some(Box::new(HostPool {
            host: self.host.clone(),
            names,
        })))
    }
}

/// The native compiler's portable build of this space.
struct Portable(Arc<Space>);

#[async_trait]
impl PortableBuilder for Portable {
    async fn export(&self, selection: Option<Selection>) -> Result<PortableExport> {
        let space = &self.0;
        let root = space.catalog.root().to_path_buf();
        let view = space.catalog.resolved().ok().flatten();
        let name = view
            .as_ref()
            .and_then(|view| {
                view["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        let config = crate::config::answer(&root, None)
            .map(|config| (!config.is_null()).then_some(config))
            .map_err(|error| format!("{error:#}"));
        let index = space.catalog.last_index();
        crate::portable::build(
            &space.previews,
            crate::portable::Request {
                root: &root,
                product: &space.product,
                index: &index,
                config,
                name,
                selection: selection.as_ref(),
            },
        )
        .await
    }
}

/// What the export asks of the space.
pub(crate) struct SpaceExport(pub(crate) Arc<Space>);

#[async_trait]
impl ExportHost for SpaceExport {
    fn capturer(&self) -> Arc<dyn Capturer> {
        Arc::new(HostCapture {
            host: self.0.host.clone(),
            worker: None,
        })
    }

    /// The host holds every capture until its warm-up has settled.
    async fn capture_ready(&self) {}

    async fn docs_pages(&self, view: &Value) -> Vec<DocsPagePlan> {
        let space = &self.0;
        let usable = space.docs_blocked().is_none() && space.previews.enabled().await;
        let mut out = Vec::new();
        for page in space.docs_pages(view) {
            let key = if page.page.is_empty() {
                page.src.clone()
            } else {
                page.page.clone()
            };
            if view["pages"].get(&key).is_none() {
                continue;
            }
            let placed = space.docs.outline(&page).ok().flatten().map(|examples| {
                examples
                    .into_iter()
                    .map(|example| (example.id, example.label))
                    .collect()
            });
            let mut lenses = Vec::new();
            for lens in &page.lenses {
                let examples = if usable {
                    match space.docs.source().list_examples(lens).await {
                        Ok(listed) => Ok(listed
                            .examples
                            .into_iter()
                            .map(|example| example.id)
                            .collect()),
                        Err(error) => Err(format!("{error:#}")),
                    }
                } else {
                    Ok(Vec::new())
                };
                lenses.push(DocsLensPlan {
                    key: lens.key.clone(),
                    label: lens.label.clone(),
                    examples,
                });
            }
            out.push(DocsPagePlan {
                page: page.page.clone(),
                src: page.src.clone(),
                label: page.label.clone(),
                placed,
                lenses,
            });
        }
        out
    }

    async fn stories(&self, implementation: &str, title: &str) -> Result<Vec<Story>> {
        let view = self.0.view().await?.unwrap_or(Value::Null);
        let stories =
            crate::storybook::stories(&view["implementations"], implementation, title).await?;
        Ok(stories
            .into_iter()
            .map(|story| Story {
                id: story.id,
                name: story.name.and_then(|name| name.as_str().map(str::to_owned)),
                state: story.state,
            })
            .collect())
    }

    async fn previews(&self) -> Option<Arc<dyn PortableBuilder>> {
        let space = self.0.clone();
        space
            .previews
            .enabled()
            .await
            .then(|| Arc::new(Portable(space)) as Arc<dyn PortableBuilder>)
    }

    fn diagnostic(&self, level: &str, event: &str, details: Value) {
        let (diagnostics, level, event) = (
            self.0.diagnostics.clone(),
            level.to_owned(),
            event.to_owned(),
        );
        tokio::spawn(async move { diagnostics.log(&level, &event, details).await });
    }
}
