//! Capturing an export's references through the capture helper, which runs
//! outside this process: up to four warm renderers, every size of one
//! address on the same renderer, one document revision per export for local
//! pages, and a warning instead of a failure for each reference that fails.
use super::paths::random_hex;
use super::plan::Plan;
use super::request::ExportRequest;
use super::types::{Reference, Screenshot};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Renderers an export captures with at once, at most.
const WORKERS: usize = 4;

/// One renderer of the capture helper. Each operation answers the image or
/// PDF bytes it produced.
#[async_trait]
pub(crate) trait CaptureWorker: Send + Sync {
    /// A page this server serves, at `base_url`, loaded and captured as an artboard.
    async fn capture(&self, base_url: &str, payload: &Value) -> Result<Vec<u8>>;
    /// A page loaded at its own address: an implementation's, a docs page, or any page for images.
    async fn capture_export_page(&self, payload: &Value) -> Result<Vec<u8>>;
    /// A page printed as PDF pages.
    async fn print_export_page(&self, payload: &Value) -> Result<Vec<u8>>;
}

/// Renderers opened for one export, closed when it is captured.
#[async_trait]
pub(crate) trait CapturePool: Send + Sync {
    fn workers(&self) -> Vec<Arc<dyn CaptureWorker>>;
    async fn close(&self) -> Result<()>;
}

/// The capture helper: one renderer, and pools of more when it has them.
#[async_trait]
pub(crate) trait Capturer: CaptureWorker {
    /// Opens `size` renderers for one export, or `None` when the helper has no pools.
    async fn pool(&self, size: usize) -> Result<Option<Box<dyn CapturePool>>>;
}

/// An export's captured references, in plan order, and what failed.
#[derive(Debug, Default)]
pub(crate) struct Captured {
    pub(crate) screenshots: Vec<Screenshot>,
    pub(crate) warnings: Vec<String>,
}

/// What the capture helper is asked to load for one reference.
fn payload(reference: &Reference, request: Option<&ExportRequest>, revision: &str) -> Value {
    let images = request.is_some_and(|request| request.is("images"));
    let mut payload = json!({
        "url": reference.url, "width": reference.width, "height": reference.height,
        "scroll": { "x": 0, "y": 0 }, "annotations": "", "anchors": [],
        "format": if images { request.map_or("png", |request| request.image_format.as_str()) } else { "jpeg" },
        "docsPage": reference.docs_page,
    });
    if let Some(request) = request {
        payload["paper"] = json!(request.paper);
    }
    payload["revision"] = json!(if reference.external || reference.docs_page {
        ""
    } else {
        revision
    });
    payload["reuse"] = json!(if reference.external { "storybook" } else { "" });
    // A docs page loads as a page, whole, cropped to an example when the reference is one.
    if reference.docs_page || images {
        payload["fullPage"] = json!(true);
        payload["strictFullPage"] = json!(images);
        if let Some(selector) = &reference.selector {
            payload["selector"] = json!(selector);
        }
    }
    payload
}

async fn take(
    worker: &dyn CaptureWorker,
    reference: &Reference,
    request: Option<&ExportRequest>,
    base_url: &str,
    revision: &str,
) -> Result<Vec<u8>> {
    let payload = payload(reference, request, revision);
    let pdf = request.is_some_and(|request| request.is("pdf"));
    let page = reference.external
        || reference.docs_page
        || request.is_some_and(|request| request.is("images"));
    let body = if pdf {
        worker.print_export_page(&payload).await?
    } else if page {
        worker.capture_export_page(&payload).await?
    } else {
        worker.capture(base_url, &payload).await?
    };
    if body.is_empty() {
        return Err(anyhow!("the renderer returned an empty image"));
    }
    Ok(body)
}

struct Shared {
    next: usize,
    completed: usize,
    screenshots: Vec<Option<Screenshot>>,
    warnings: Vec<String>,
}

/// Captures `plan`, reporting the number captured so far to `progress`.
///
/// # Errors
/// Only when the helper cannot open its renderers; each reference that fails
/// becomes a warning.
pub(crate) async fn capture_references(
    capturer: Arc<dyn Capturer>,
    plan: &Plan,
    base_url: &str,
    progress: &(dyn Fn(usize) + Send + Sync),
    request: Option<&ExportRequest>,
) -> Result<Captured> {
    // One load per local address during this export; another export sees fresh sources.
    let revision = format!("export-{}", random_hex());
    // Every size of one page or story stays on one renderer: the first shot
    // pays for loading it, the other sizes reuse that document.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut by_url: HashMap<String, usize> = HashMap::new();
    for (index, reference) in plan.captures.iter().enumerate() {
        let key = format!(
            "{}:{}",
            if reference.external {
                "external"
            } else {
                "local"
            },
            reference.url
        );
        let group = *by_url.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(index);
    }
    let pool = if groups.len() > 1 {
        capturer.pool(WORKERS.min(groups.len())).await?
    } else {
        None
    };
    let workers: Vec<Arc<dyn CaptureWorker>> = match &pool {
        Some(pool) => pool.workers(),
        None => vec![capturer.clone() as Arc<dyn CaptureWorker>],
    };
    let shared = Mutex::new(Shared {
        next: 0,
        completed: 0,
        screenshots: vec![None; plan.captures.len()],
        warnings: plan.warnings.clone(),
    });
    if !plan.captures.is_empty() {
        progress(0);
    }
    let work = |worker: Arc<dyn CaptureWorker>| {
        let (groups, shared, revision) = (&groups, &shared, &revision);
        async move {
            loop {
                let group = {
                    let mut state = shared.lock().expect("capture state");
                    let Some(group) = groups.get(state.next) else {
                        break;
                    };
                    state.next += 1;
                    group
                };
                for index in group {
                    let reference = &plan.captures[*index];
                    let taken = take(worker.as_ref(), reference, request, base_url, revision).await;
                    let completed = {
                        let mut state = shared.lock().expect("capture state");
                        match taken {
                            Ok(body) => {
                                state.screenshots[*index] = Some(Screenshot::of(reference, body));
                            }
                            Err(error) => state.warnings.push(format!(
                                "{} — {} · {}: {error:#}",
                                reference.page, reference.label, reference.size_label
                            )),
                        }
                        state.completed += 1;
                        state.completed
                    };
                    progress(completed);
                }
            }
        }
    };
    futures::future::join_all(workers.into_iter().map(work)).await;
    let mut state = shared.into_inner().expect("capture state");
    // Cleanup never discards successful captures: a renderer that resists
    // shutdown is reported, not a reason to fail the export.
    if let Some(pool) = pool
        && let Err(error) = pool.close().await
    {
        state
            .warnings
            .push(format!("Capture worker cleanup: {error:#}"));
    }
    Ok(Captured {
        screenshots: state.screenshots.into_iter().flatten().collect(),
        warnings: state.warnings,
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A capture helper that answers fixed bytes and records what it was asked.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) payloads: Mutex<Vec<(usize, Value)>>,
        pub(crate) active: AtomicUsize,
        pub(crate) peak: AtomicUsize,
        pub(crate) pools: Option<usize>,
        pub(crate) closed: AtomicUsize,
        pub(crate) close_fails: bool,
        pub(crate) delay: Option<Duration>,
        pub(crate) fail_url: Option<String>,
        /// Bytes `print_export_page` answers; `None` answers "jpeg" for everything.
        pub(crate) pdf: Option<fn(&Value) -> Vec<u8>>,
    }

    pub(crate) struct Worker {
        pub(crate) fake: Arc<Fake>,
        pub(crate) id: usize,
    }

    impl Fake {
        async fn answer(&self, id: usize, payload: &Value) -> Result<Vec<u8>> {
            self.payloads
                .lock()
                .expect("payloads")
                .push((id, payload.clone()));
            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.fail_url.as_deref() == payload["url"].as_str() {
                return Err(anyhow!("the page crashed"));
            }
            Ok(b"jpeg".to_vec())
        }
    }

    #[async_trait]
    impl CaptureWorker for Worker {
        async fn capture(&self, _base: &str, payload: &Value) -> Result<Vec<u8>> {
            self.fake.answer(self.id, payload).await
        }
        async fn capture_export_page(&self, payload: &Value) -> Result<Vec<u8>> {
            self.fake.answer(self.id, payload).await
        }
        async fn print_export_page(&self, payload: &Value) -> Result<Vec<u8>> {
            self.fake.answer(self.id, payload).await?;
            Ok(self
                .fake
                .pdf
                .map_or_else(|| b"jpeg".to_vec(), |pdf| pdf(payload)))
        }
    }

    struct Pool(Arc<Fake>, usize);

    #[async_trait]
    impl CapturePool for Pool {
        fn workers(&self) -> Vec<Arc<dyn CaptureWorker>> {
            (1..=self.1)
                .map(|id| {
                    Arc::new(Worker {
                        fake: self.0.clone(),
                        id,
                    }) as Arc<dyn CaptureWorker>
                })
                .collect()
        }
        async fn close(&self) -> Result<()> {
            self.0.closed.fetch_add(1, Ordering::SeqCst);
            if self.0.close_fails {
                Err(anyhow!("close failed"))
            } else {
                Ok(())
            }
        }
    }

    /// The fake as a capture helper; `pools` makes it open pools.
    pub(crate) struct Helper(pub(crate) Arc<Fake>);

    #[async_trait]
    impl CaptureWorker for Helper {
        async fn capture(&self, base: &str, payload: &Value) -> Result<Vec<u8>> {
            Worker {
                fake: self.0.clone(),
                id: 0,
            }
            .capture(base, payload)
            .await
        }
        async fn capture_export_page(&self, payload: &Value) -> Result<Vec<u8>> {
            Worker {
                fake: self.0.clone(),
                id: 0,
            }
            .capture_export_page(payload)
            .await
        }
        async fn print_export_page(&self, payload: &Value) -> Result<Vec<u8>> {
            Worker {
                fake: self.0.clone(),
                id: 0,
            }
            .print_export_page(payload)
            .await
        }
    }

    #[async_trait]
    impl Capturer for Helper {
        async fn pool(&self, size: usize) -> Result<Option<Box<dyn CapturePool>>> {
            Ok(self
                .0
                .pools
                .map(|_| Box::new(Pool(self.0.clone(), size)) as Box<dyn CapturePool>))
        }
    }

    fn story(id: &str, size: &str) -> Reference {
        Reference {
            page: id.into(),
            state: "default".into(),
            variant: format!("default-{size}"),
            label: id.into(),
            size: size.into(),
            size_label: size.into(),
            url: format!("http://localhost:6006/iframe.html?id={id}"),
            external: true,
            width: 393,
            height: 852,
            ..Reference::default()
        }
    }

    fn plan(captures: Vec<Reference>) -> Plan {
        Plan {
            captures,
            warnings: Vec::new(),
        }
    }

    #[tokio::test]
    async fn captures_export_references_through_four_reusable_workers() {
        let fake = Arc::new(Fake {
            pools: Some(4),
            delay: Some(Duration::from_millis(10)),
            ..Fake::default()
        });
        let stories = plan(
            (0..7)
                .map(|index| story(&format!("story-{index}"), "mobile"))
                .collect(),
        );
        let result = capture_references(
            Arc::new(Helper(fake.clone())),
            &stories,
            "http://localhost:3579/",
            &|_| {},
            None,
        )
        .await
        .expect("captured");
        assert_eq!(fake.peak.load(Ordering::SeqCst), 4);
        assert_eq!(result.screenshots.len(), 7);
        assert!(
            fake.payloads
                .lock()
                .expect("payloads")
                .iter()
                .all(|(_, payload)| payload["reuse"] == "storybook")
        );
        assert_eq!(fake.closed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn keeps_successful_captures_when_pool_cleanup_fails() {
        let fake = Arc::new(Fake {
            pools: Some(2),
            close_fails: true,
            ..Fake::default()
        });
        let stories = plan(vec![story("button", "mobile"), story("card", "mobile")]);
        let result = capture_references(
            Arc::new(Helper(fake)),
            &stories,
            "http://localhost:3579/",
            &|_| {},
            None,
        )
        .await
        .expect("captured");
        assert_eq!(result.screenshots.len(), 2);
        assert_eq!(result.warnings, ["Capture worker cleanup: close failed"]);
    }

    #[tokio::test]
    async fn keeps_every_size_for_a_story_on_one_warm_worker() {
        let fake = Arc::new(Fake {
            pools: Some(2),
            ..Fake::default()
        });
        let stories = plan(
            ["button", "card"]
                .iter()
                .flat_map(|id| ["mobile", "laptop", "fit"].map(|size| story(id, size)))
                .collect(),
        );
        let result = capture_references(
            Arc::new(Helper(fake.clone())),
            &stories,
            "http://localhost:3579/",
            &|_| {},
            None,
        )
        .await
        .expect("captured");
        assert_eq!(result.screenshots.len(), 6);
        let mut workers: HashMap<String, std::collections::BTreeSet<usize>> = HashMap::new();
        for (worker, payload) in fake.payloads.lock().expect("payloads").iter() {
            workers
                .entry(payload["url"].as_str().unwrap_or_default().to_owned())
                .or_default()
                .insert(*worker);
        }
        assert!(workers.values().all(|ids| ids.len() == 1));
    }

    #[tokio::test]
    async fn local_sizes_share_a_document_revision_while_another_export_gets_a_fresh_one() {
        let fake = Arc::new(Fake::default());
        let local = plan(
            [393, 1512]
                .iter()
                .map(|width| Reference {
                    page: "button".into(),
                    label: "Default".into(),
                    size_label: width.to_string(),
                    url: "http://localhost:3579/button.html".into(),
                    width: *width,
                    height: 852,
                    ..Reference::default()
                })
                .collect(),
        );
        let helper: Arc<dyn Capturer> = Arc::new(Helper(fake.clone()));
        capture_references(
            helper.clone(),
            &local,
            "http://localhost:3579/",
            &|_| {},
            None,
        )
        .await
        .expect("first");
        capture_references(helper, &local, "http://localhost:3579/", &|_| {}, None)
            .await
            .expect("second");
        let revisions: Vec<String> = fake
            .payloads
            .lock()
            .expect("payloads")
            .iter()
            .map(|(_, payload)| payload["revision"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert!(revisions[0].starts_with("export-"));
        assert_eq!(revisions[0], revisions[1]);
        assert_ne!(revisions[0], revisions[2]);
        assert_eq!(revisions[2], revisions[3]);
    }

    #[tokio::test]
    async fn a_failed_reference_is_a_warning_and_progress_counts_every_reference() {
        let fake = Arc::new(Fake {
            fail_url: Some("http://localhost:6006/iframe.html?id=card".into()),
            ..Fake::default()
        });
        let stories = plan(vec![story("button", "mobile"), story("card", "mobile")]);
        let seen = Mutex::new(Vec::new());
        let result = capture_references(
            Arc::new(Helper(fake)),
            &stories,
            "http://localhost:3579/",
            &|done| seen.lock().expect("seen").push(done),
            None,
        )
        .await
        .expect("captured");
        assert_eq!(result.screenshots.len(), 1);
        assert_eq!(result.warnings, ["card — card · mobile: the page crashed"]);
        assert_eq!(*seen.lock().expect("seen"), [0, 1, 2]);
    }

    #[test]
    fn docs_and_image_references_load_whole_pages_cropped_to_their_example() {
        let request =
            crate::export::read_request(&json!({ "format": "images", "imageFormat": "jpeg" }))
                .expect("request");
        let reference = Reference {
            docs_page: true,
            selector: Some("#example-a [data-wb-example-stage]".into()),
            ..story("a", "docs")
        };
        let sent = payload(&reference, Some(&request), "export-1");
        assert_eq!(sent["format"], "jpeg");
        assert_eq!(sent["paper"], "A4");
        assert_eq!(
            (sent["fullPage"].clone(), sent["strictFullPage"].clone()),
            (json!(true), json!(true))
        );
        assert_eq!(sent["selector"], "#example-a [data-wb-example-stage]");
        assert_eq!(sent["revision"], "");
        let local = payload(
            &Reference {
                url: "http://localhost:3579/a.html".into(),
                ..Reference::default()
            },
            None,
            "export-1",
        );
        assert_eq!(
            (local["revision"].clone(), local.get("fullPage")),
            (json!("export-1"), None)
        );
        assert_eq!(local.get("paper"), None);
    }
}
