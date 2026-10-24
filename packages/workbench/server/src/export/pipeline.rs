//! One export from request to archive: plan the references, add the docs
//! pages, map them to the chosen lens, capture them, and assemble the format
//! asked for, as a background job. The server supplies what lives outside
//! this module through [`ExportHost`].
use super::build::{PortableBuilder, Selection, build};
use super::capture::{Captured, Capturer, capture_references};
use super::jobs::{Job, Jobs};
use super::output::{browser_output, images_output, pdf_output};
use super::paths::{component_encode, resolve_url};
use super::plan::{Plan, capture_plan, docs_capture_plan, page_items};
use super::request::{ExportRequest, request_key};
use super::selection::select_captures;
use super::types::{ExportOptions, ExportResult, ExportView, Reference};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) use super::plan::{DocsLensPlan, DocsPagePlan};

/// A story of a Storybook implementation, under the title a page maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Story {
    pub(crate) id: String,
    pub(crate) name: Option<String>,
    /// The scenario the story is shown as.
    pub(crate) scenario: String,
}

/// What an export needs from the server it runs in.
#[async_trait]
pub(crate) trait ExportHost: Send + Sync {
    fn capturer(&self) -> Arc<dyn Capturer>;
    /// Resolves once the capture helper's warm-up has settled, either way.
    async fn capture_ready(&self);
    /// The space's docs pages, each lens with the examples it renders here.
    async fn docs_pages(&self, view: &Value) -> Vec<DocsPagePlan>;
    /// The stories Storybook implementation `implementation` holds under `title`.
    async fn stories(&self, implementation: &str, title: &str) -> Result<Vec<Story>>;
    /// The native compiler's portable builds, when previews are on.
    async fn previews(&self) -> Option<Arc<dyn PortableBuilder>>;
    fn diagnostic(&self, level: &str, event: &str, details: Value);
}

/// One export to start.
#[derive(Debug, Clone)]
pub(crate) struct ExportInput {
    pub(crate) root: PathBuf,
    /// The resolved space, already narrowed by `select_view`.
    pub(crate) view: Value,
    /// The workbench's own address, ending in `/`.
    pub(crate) base_url: String,
    pub(crate) request: ExportRequest,
    /// The space's `workbench.yaml`.
    pub(crate) manifest: PathBuf,
    /// Workbench's own folder.
    pub(crate) product: Option<PathBuf>,
    pub(crate) max_archive_bytes: Option<usize>,
}

/// Starts the export, or joins the running one that asked the same.
///
/// # Errors
/// 409 while a different export is running.
pub(crate) fn start(
    jobs: &Arc<Jobs>,
    host: Arc<dyn ExportHost>,
    input: ExportInput,
) -> Result<Arc<Job>> {
    let plan = capture_plan(&input.view, &input.base_url);
    let (total, warnings) = (plan.captures.len(), plan.warnings.len());
    let key = request_key(&input.request);
    jobs.begin(key, total, warnings, move |progress| {
        host.diagnostic("info", "export.started", json!({ "screenshots": total }));
        async move {
            let outcome = run(
                host.as_ref(),
                &input,
                plan,
                &|count| progress.total(count),
                &|done| progress.completed(done),
            )
            .await;
            match &outcome {
                Ok(result) => host.diagnostic(
                    "info",
                    "export.completed",
                    json!({
                        "files": result.report.files.len(),
                        "archives": result.archives.len(),
                        "warnings": result.report.all_warnings().len(),
                    }),
                ),
                Err(error) => host.diagnostic(
                    "error",
                    "export.failed",
                    json!({ "message": format!("{error:#}") }),
                ),
            }
            outcome
        }
    })
}

fn lens_of(request: &ExportRequest) -> Option<String> {
    let lens = if request.variants == "current" {
        request
            .current
            .as_ref()
            .and_then(|current| current.lens.clone())
    } else {
        request.lens.clone()
    };
    lens.filter(|lens| !lens.is_empty())
}

/// Every page item of the space and its catalog.
fn items(view: &Value) -> Vec<&Value> {
    let mut items = page_items(&view["collections"]);
    items.extend(page_items(&view["catalogCollections"]));
    items
}

/// A Storybook lens captures each mapped story at the sizes its page has.
async fn storybook_references(
    host: &dyn ExportHost,
    view: &Value,
    plan: &mut Plan,
    lens: &str,
) -> Result<()> {
    let implementation = &view["implementations"][lens];
    let address = implementation["url"].as_str().unwrap_or_default();
    let mut replacements = Vec::new();
    for item in items(view) {
        let Some(title) = item["implementations"][lens]["title"]
            .as_str()
            .filter(|title| !title.is_empty())
        else {
            continue;
        };
        let src = item["src"].as_str().unwrap_or_default();
        let mut sizes: Vec<Reference> = Vec::new();
        for reference in plan
            .captures
            .iter()
            .filter(|reference| reference.page == src && !reference.docs_page)
        {
            match sizes.iter_mut().find(|seen| seen.size == reference.size) {
                Some(seen) => *seen = reference.clone(),
                None => sizes.push(reference.clone()),
            }
        }
        for story in host.stories(lens, title).await? {
            for reference in &sizes {
                replacements.push(Reference {
                    scenario: story.scenario.clone(),
                    label: story
                        .name
                        .clone()
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| story.scenario.clone()),
                    variant: format!("{}-{}", story.scenario, reference.size),
                    external: true,
                    url: format!(
                        "{address}/iframe.html?id={}&viewMode=story",
                        component_encode(&story.id)
                    ),
                    ..reference.clone()
                });
            }
        }
    }
    plan.captures.retain(|reference| reference.docs_page);
    plan.captures.extend(replacements);
    Ok(())
}

/// Points every artboard reference at the lens's implementation.
fn map_to_lens(view: &Value, plan: &mut Plan, lens: &str, base_url: &str) -> Result<()> {
    let implementation = &view["implementations"][lens];
    let kind = implementation["kind"].as_str().unwrap_or_default();
    let items = items(view);
    for reference in plan
        .captures
        .iter_mut()
        .filter(|reference| !reference.docs_page)
    {
        let item = items
            .iter()
            .find(|item| item["src"] == reference.page.as_str());
        if item.is_some_and(|item| item["implementationOnly"] == lens) {
            continue;
        }
        let mapping = item
            .map(|item| &item["implementations"][lens])
            .filter(|mapping| !mapping.is_null());
        let Some(mapping) = mapping.filter(|_| implementation.is_object()) else {
            bail!("The selected lens is not available for {}.", reference.page);
        };
        if kind == "storybook" {
            continue;
        }
        if kind != "url" && kind != "workbench" {
            bail!("Export of this lens is not supported: {lens}");
        }
        let target = mapping["scenarios"][&reference.scenario]
            .as_str()
            .filter(|target| !target.is_empty())
            .or(mapping["path"].as_str())
            .unwrap_or_default();
        let base = implementation["base"].as_str().unwrap_or_default();
        reference.url = resolve_url(&format!("{base}{target}"), base_url);
        reference.external = kind != "workbench";
    }
    Ok(())
}

async fn run(
    host: &dyn ExportHost,
    input: &ExportInput,
    mut plan: Plan,
    total: &(dyn Fn(usize) + Send + Sync),
    completed: &(dyn Fn(usize) + Send + Sync),
) -> Result<ExportResult> {
    let (view, request) = (&input.view, &input.request);
    if !request.is("browser") {
        host.capture_ready().await;
        let pages: Vec<DocsPagePlan> = host
            .docs_pages(view)
            .await
            .into_iter()
            .filter(|page| {
                view["pages"]
                    .get(if page.page.is_empty() {
                        &page.src
                    } else {
                        &page.page
                    })
                    .is_some()
            })
            .collect();
        let docs = docs_capture_plan(&pages, &input.base_url);
        plan.captures.extend(docs.captures);
        plan.warnings.extend(docs.warnings);
    }
    let lens = lens_of(request);
    let kind = lens
        .as_deref()
        .and_then(|lens| view["implementations"][lens]["kind"].as_str());
    if lens.is_some() {
        // Iterations belong to the design; an implementation lens shows the shipped code.
        plan.captures
            .retain(|reference| reference.iteration.is_none());
    }
    if let Some(lens) = lens.as_deref().filter(|_| kind == Some("storybook")) {
        storybook_references(host, view, &mut plan, lens).await?;
    }
    plan.captures = select_captures(std::mem::take(&mut plan.captures), request);
    if kind == Some("docs") {
        plan.captures.retain(|reference| reference.docs_page);
    }
    if let Some(lens) = &lens {
        map_to_lens(view, &mut plan, lens, &input.base_url)?;
    }
    if request.is("browser") {
        plan.captures.clear();
    }
    total(plan.captures.len());
    let progress = |done: usize| {
        completed(done);
        if done == 1 || (done > 0 && done.is_multiple_of(100)) {
            host.diagnostic(
                "info",
                "export.progress",
                json!({ "completed": done, "total": plan.captures.len() }),
            );
        }
    };
    let captured = capture_references(
        host.capturer(),
        &plan,
        &input.base_url,
        &progress,
        Some(request),
    )
    .await?;
    assemble(host, input, captured).await
}

/// The archive, PDF or images of the captured references, in the format asked for.
async fn assemble(
    host: &dyn ExportHost,
    input: &ExportInput,
    captured: Captured,
) -> Result<ExportResult> {
    let (view, request) = (&input.view, &input.request);
    let name = view["name"]
        .as_str()
        .filter(|name| !name.is_empty())
        .unwrap_or("Workbench");
    let previews = host.previews().await;
    let selection = || Selection {
        pages: view["pages"]
            .as_object()
            .map(|pages| pages.keys().cloned().collect())
            .unwrap_or_default(),
        scenarios: request.scenarios.clone(),
        sizes: request.sizes.clone(),
    };
    match request.format.as_str() {
        "pdf" => pdf_output(name, request, &captured.screenshots, &captured.warnings),
        "images" => images_output(
            name,
            request,
            &captured.screenshots,
            &captured.warnings,
            input.max_archive_bytes,
        ),
        "browser" => {
            let portable = match &previews {
                Some(previews) => Some(previews.export(Some(selection())).await?),
                None => None,
            };
            browser_output(name, request, portable.as_ref(), input.max_archive_bytes)
        }
        _ => {
            let options = ExportOptions {
                request: Some(request.clone()),
                manifest: Some(input.manifest.clone()),
                screenshots: captured.screenshots,
                capture_warnings: captured.warnings,
                max_archive_bytes: input.max_archive_bytes,
                product: input.product.clone(),
                ..ExportOptions::default()
            };
            build(
                &input.root,
                &ExportView::from_config(view),
                options,
                previews.as_deref(),
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::archive::tests::{names, read_zip};
    use crate::export::capture::tests::{Fake, Helper};
    use crate::export::output::tests::{blank_pdf, page_sizes};
    use crate::export::read_request;
    use crate::export::source::tests::Project;
    use crate::export::types::PortableExport;
    use std::sync::Mutex;

    struct Host {
        fake: Arc<Fake>,
        docs: Vec<DocsPagePlan>,
        previews: Option<Arc<dyn PortableBuilder>>,
        events: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ExportHost for Host {
        fn capturer(&self) -> Arc<dyn Capturer> {
            Arc::new(Helper(self.fake.clone()))
        }
        async fn capture_ready(&self) {}
        async fn docs_pages(&self, _view: &Value) -> Vec<DocsPagePlan> {
            self.docs.clone()
        }
        async fn stories(&self, _implementation: &str, title: &str) -> Result<Vec<Story>> {
            if title != "Components/Button" {
                bail!("No stories titled “{title}”");
            }
            Ok(vec![
                Story {
                    id: "components-button--primary".into(),
                    name: Some("Primary".into()),
                    scenario: "primary".into(),
                },
                Story {
                    id: "components-button--ghost".into(),
                    name: None,
                    scenario: "ghost".into(),
                },
            ])
        }
        async fn previews(&self) -> Option<Arc<dyn PortableBuilder>> {
            self.previews.clone()
        }
        fn diagnostic(&self, _level: &str, event: &str, _details: Value) {
            self.events.lock().expect("events").push(event.to_owned());
        }
    }

    #[expect(clippy::cast_possible_truncation, reason = "a page width in points")]
    fn printed(payload: &Value) -> Vec<u8> {
        blank_pdf(
            (payload["width"].as_f64().unwrap_or_default() * 0.75) as f32,
            2000.0,
            false,
        )
    }

    struct Failing;

    #[async_trait]
    impl PortableBuilder for Failing {
        async fn export(&self, _selection: Option<Selection>) -> Result<PortableExport> {
            bail!("Preview worker stopped")
        }
    }

    fn host(fake: Fake) -> Arc<Host> {
        Arc::new(Host {
            fake: Arc::new(fake),
            docs: Vec::new(),
            previews: None,
            events: Mutex::new(Vec::new()),
        })
    }

    fn space(project: &Project) -> Value {
        project.write("workbench.yaml", "name: Acme\n");
        let design = project.write("pages/sign-in.html", "<main>Sign in</main>");
        json!({
            "name": "Acme",
            "implementations": {
                "storybook": { "kind": "storybook", "url": "http://localhost:6006" },
                "dev": { "kind": "url", "base": "http://localhost:3000" },
            },
            "collections": [{ "name": "Pages", "items": [{
                "label": "Sign in", "src": "pages/sign-in.html", "sizes": ["laptop", "mobile"],
                "scenarios": [{ "id": "default", "label": "Default" }, { "id": "error", "label": "Error" }],
                "implementations": {
                    "storybook": { "title": "Components/Button" },
                    "dev": { "path": "/sign-in", "scenarios": { "error": "/sign-in?error=1" } },
                },
            }] }],
            "pages": { "pages/sign-in.html": { "label": "Sign in", "design": design.to_string_lossy(), "code": [] } },
        })
    }

    fn input(project: &Project, view: Value, request: &Value) -> ExportInput {
        ExportInput {
            root: project.0.clone(),
            view,
            base_url: "http://127.0.0.1:3579/".into(),
            request: read_request(request).expect("request"),
            manifest: project.0.join("workbench.yaml"),
            product: None,
            max_archive_bytes: None,
        }
    }

    async fn finished(job: &Job) -> Arc<ExportResult> {
        job.wait().await;
        job.result()
            .unwrap_or_else(|| panic!("export failed: {:?}", job.error()))
    }

    #[tokio::test]
    async fn a_space_export_captures_every_scenario_and_size_and_archives_sources_with_the_failed_preview_cause()
     {
        let project = Project::new("pipeline");
        let view = space(&project);
        let host = Arc::new(Host {
            previews: Some(Arc::new(Failing)),
            ..Arc::into_inner(host(Fake::default())).expect("host")
        });
        let jobs = Jobs::new(None);
        let job = start(&jobs, host.clone(), input(&project, view, &json!({}))).expect("job");
        assert_eq!(job.status()["total"], 4);
        let result = finished(&job).await;
        assert_eq!(job.status()["completed"], 4);
        assert_eq!(job.status()["warnings"], 1);
        let archived = read_zip(&result.download().body);
        assert!(
            archived
                .iter()
                .any(|(name, _)| name == "acme-design-system/pages/sign-in.html")
        );
        assert_eq!(
            archived
                .iter()
                .filter(|(name, body)| name.rsplit('.').next() == Some("jpg") && body == b"jpeg")
                .count(),
            4
        );
        assert_eq!(
            result.report.warnings,
            ["Portable browser previews: Preview worker stopped"]
        );
        assert!(
            !archived
                .iter()
                .any(|(name, _)| name.contains("browser/index.html"))
        );
        assert_eq!(
            *host.events.lock().expect("events"),
            ["export.started", "export.progress", "export.completed"]
        );
    }

    #[tokio::test]
    async fn a_current_page_pdf_prints_only_its_selected_scenario_and_actual_viewport() {
        let project = Project::new("pdf");
        let view = space(&project);
        let host = host(Fake {
            pdf: Some(printed),
            ..Fake::default()
        });
        let request = json!({ "format": "pdf", "scope": "page", "current": { "page": "pages/sign-in.html", "width": 777, "height": 600 } });
        let jobs = Jobs::new(None);
        let result =
            finished(&start(&jobs, host.clone(), input(&project, view, &request)).expect("job"))
                .await;
        let printed = host.fake.payloads.lock().expect("payloads").clone();
        assert_eq!(printed.len(), 1);
        assert_eq!(
            (
                printed[0].1["width"].clone(),
                printed[0].1["height"].clone()
            ),
            (json!(777), json!(600))
        );
        assert_eq!(result.download().content_type, "application/pdf");
        assert_eq!(page_sizes(&result.download().body), [(582.75, 2000.0)]);
    }

    #[tokio::test]
    async fn a_storybook_lens_captures_each_mapped_story_at_the_page_sizes() {
        let project = Project::new("storybook");
        let view = space(&project);
        let host = host(Fake::default());
        let jobs = Jobs::new(None);
        let job = start(
            &jobs,
            host.clone(),
            input(
                &project,
                view,
                &json!({ "format": "images", "lens": "storybook" }),
            ),
        )
        .expect("job");
        let result = finished(&job).await;
        let urls: Vec<String> = host
            .fake
            .payloads
            .lock()
            .expect("payloads")
            .iter()
            .map(|(_, payload)| {
                format!(
                    "{} {}",
                    payload["url"].as_str().unwrap_or_default(),
                    payload["width"]
                )
            })
            .collect();
        assert_eq!(
            urls,
            [
                "http://localhost:6006/iframe.html?id=components-button--primary&viewMode=story 1512",
                "http://localhost:6006/iframe.html?id=components-button--primary&viewMode=story 393",
                "http://localhost:6006/iframe.html?id=components-button--ghost&viewMode=story 1512",
                "http://localhost:6006/iframe.html?id=components-button--ghost&viewMode=story 393",
            ]
        );
        assert_eq!(
            result.report.files,
            [
                "001-pages-sign-in-html-primary-laptop.png",
                "002-pages-sign-in-html-primary-mobile.png",
                "003-pages-sign-in-html-ghost-laptop.png",
                "004-pages-sign-in-html-ghost-mobile.png",
            ]
        );
    }

    #[tokio::test]
    async fn a_url_lens_captures_the_implementation_and_an_unmapped_lens_fails_the_job() {
        let project = Project::new("lens");
        let view = space(&project);
        let host = host(Fake::default());
        let jobs = Jobs::new(None);
        let request = json!({ "format": "images", "lens": "dev", "sizes": ["mobile"] });
        finished(
            &start(&jobs, host.clone(), input(&project, view.clone(), &request)).expect("job"),
        )
        .await;
        let urls: Vec<String> = host
            .fake
            .payloads
            .lock()
            .expect("payloads")
            .iter()
            .map(|(_, payload)| payload["url"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(
            urls,
            [
                "http://localhost:3000/sign-in",
                "http://localhost:3000/sign-in?error=1"
            ]
        );
        let missing = start(
            &jobs,
            host,
            input(
                &project,
                view,
                &json!({ "format": "images", "lens": "nowhere" }),
            ),
        )
        .expect("job");
        missing.wait().await;
        assert_eq!(
            missing.error().as_deref(),
            Some("The selected lens is not available for pages/sign-in.html.")
        );
    }

    #[tokio::test]
    async fn docs_pages_add_their_whole_page_and_example_references() {
        let project = Project::new("docs");
        let mut view = space(&project);
        view["pages"]["docs/button.md"] = json!({ "label": "Button", "design": null, "code": [] });
        let docs = vec![
            DocsPagePlan {
                page: "docs/button.md".into(),
                src: "docs/button.md".into(),
                label: "Button".into(),
                placed: Some(vec![("primary".into(), "Primary".into())]),
                lenses: vec![DocsLensPlan {
                    key: "web".into(),
                    label: "Web".into(),
                    examples: Ok(vec!["primary".into()]),
                }],
            },
            DocsPagePlan {
                page: "docs/other.md".into(),
                src: "docs/other.md".into(),
                label: "Other".into(),
                placed: Some(vec![]),
                lenses: vec![],
            },
        ];
        let host = Arc::new(Host {
            docs,
            ..Arc::into_inner(host(Fake::default())).expect("host")
        });
        let jobs = Jobs::new(None);
        let job = start(&jobs, host.clone(), input(&project, view, &json!({}))).expect("job");
        let result = finished(&job).await;
        let docs: Vec<(String, Value)> = host
            .fake
            .payloads
            .lock()
            .expect("payloads")
            .iter()
            .filter(|(_, payload)| payload["docsPage"] == true)
            .map(|(_, payload)| {
                (
                    payload["url"].as_str().unwrap_or_default().to_owned(),
                    payload["selector"].clone(),
                )
            })
            .collect();
        assert_eq!(
            docs,
            [
                (
                    "http://127.0.0.1:3579/docs/button.md?lens=web".to_owned(),
                    Value::Null
                ),
                (
                    "http://127.0.0.1:3579/docs/button.md?lens=web".to_owned(),
                    json!("#example-primary [data-wb-example-stage]")
                ),
            ]
        );
        assert!(
            names(&result.download().body)
                .iter()
                .any(|name| name.ends_with("web-primary.jpg"))
        );
    }
}
