//! Export jobs: one runs at a time, an identical request joins it and a
//! different one is refused; the latest settled job stays downloadable for a
//! retrying browser, and every job is forgotten a while after it settles.
use super::types::{Archive, ExportResult};
use crate::refused::refused;
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How long a settled job stays available unless told otherwise.
const RETENTION: Duration = Duration::from_mins(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Complete,
    Failed,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug)]
struct JobState {
    status: Status,
    completed: usize,
    total: usize,
    current: Option<String>,
    warnings: usize,
    result: Option<Arc<ExportResult>>,
    error: Option<String>,
    finished: Option<Instant>,
}

/// One export in progress or settled.
#[derive(Debug)]
pub(crate) struct Job {
    id: String,
    key: String,
    state: Mutex<JobState>,
    settled: watch::Sender<bool>,
}

impl Job {
    #[cfg(test)]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, JobState> {
        self.state.lock().expect("export job state")
    }

    pub(crate) fn is_running(&self) -> bool {
        self.lock().status == Status::Running
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.lock().status == Status::Complete
    }

    /// The finished export, once complete.
    pub(crate) fn result(&self) -> Option<Arc<ExportResult>> {
        self.lock().result.clone()
    }

    /// Why the job failed, once it has.
    pub(crate) fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Resolves once the job has settled.
    pub(crate) async fn wait(&self) {
        let mut settled = self.settled.subscribe();
        let _settled = settled.wait_for(|done| *done).await;
    }

    /// The status a canvas polls.
    pub(crate) fn status(&self) -> Value {
        let state = self.lock();
        let result = state.result.as_deref();
        json!({
            "ok": state.status != Status::Failed,
            "id": self.id,
            "status": state.status.name(),
            "completed": state.completed,
            "total": state.total,
            "current": state.current,
            "warnings": state.warnings,
            "error": state.error,
            "warningMessages": result.map_or_else(Vec::new, |result| result.report.all_warnings()),
            "download": result.map(|result| json!({ "filename": result.download().filename, "bytes": result.download().body.len() })),
            "parts": result.map_or_else(Vec::new, |result| result.archives.iter().enumerate().map(|(index, archive)| {
                json!({ "number": index + 1, "filename": archive.filename, "bytes": archive.body.len() })
            }).collect()),
        })
    }
}

/// What a running export reports as it goes.
#[derive(Clone)]
pub(crate) struct Progress(Arc<Job>);

impl Progress {
    pub(crate) fn total(&self, total: usize) {
        self.0.lock().total = total;
    }

    pub(crate) fn completed(&self, completed: usize) {
        self.0.lock().completed = completed;
    }
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, Arc<Job>>,
    active: Option<Arc<Job>>,
}

/// The export jobs of one space.
pub(crate) struct Jobs {
    registry: Mutex<Registry>,
    retention: Duration,
}

impl Jobs {
    pub(crate) fn new(retention: Option<Duration>) -> Arc<Self> {
        Arc::new(Self {
            registry: Mutex::default(),
            retention: retention.unwrap_or(RETENTION),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry.lock().expect("export jobs")
    }

    pub(crate) fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.lock().jobs.get(id).cloned()
    }

    /// Starts `work` as a job, or answers the running job with the same key.
    ///
    /// # Errors
    /// 409 while a different export is running.
    pub(crate) fn begin<F, Fut>(
        self: &Arc<Self>,
        key: String,
        total: usize,
        warnings: usize,
        work: F,
    ) -> Result<Arc<Job>>
    where
        F: FnOnce(Progress) -> Fut,
        Fut: Future<Output = Result<ExportResult>> + Send + 'static,
    {
        let job = {
            let mut registry = self.lock();
            if let Some(active) = registry
                .active
                .as_ref()
                .filter(|active| active.is_running())
            {
                if active.key == key {
                    return Ok(active.clone());
                }
                return Err(refused(
                    409,
                    "Another export is running. Wait for it to finish before starting a different export.",
                ));
            }
            // Keep the previous result for a retrying browser, not every repeated archive.
            let mut settled: Vec<(Instant, String)> = registry
                .jobs
                .values()
                .filter_map(|job| job.lock().finished.map(|at| (at, job.id.clone())))
                .collect();
            settled.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
            for (_, id) in settled.into_iter().skip(1) {
                registry.jobs.remove(&id);
            }
            let job = Arc::new(Job {
                id: super::paths::random_hex(),
                key,
                state: Mutex::new(JobState {
                    status: Status::Running,
                    completed: 0,
                    total,
                    current: None,
                    warnings,
                    result: None,
                    error: None,
                    finished: None,
                }),
                settled: watch::channel(false).0,
            });
            registry.jobs.insert(job.id.clone(), job.clone());
            registry.active = Some(job.clone());
            job
        };
        let running = work(Progress(job.clone()));
        let (jobs, settled) = (self.clone(), job.clone());
        tokio::spawn(async move {
            let outcome = running.await;
            {
                let mut state = settled.lock();
                match outcome {
                    Ok(result) => {
                        state.warnings = result.report.all_warnings().len();
                        state.completed = state.total;
                        state.result = Some(Arc::new(result));
                        state.status = Status::Complete;
                    }
                    Err(error) => {
                        state.error = Some(format!("{error:#}"));
                        state.status = Status::Failed;
                    }
                }
                state.current = None;
                state.finished = Some(Instant::now());
            }
            {
                let mut registry = jobs.lock();
                if registry
                    .active
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &settled))
                {
                    registry.active = None;
                }
            }
            settled.settled.send_replace(true);
            tokio::time::sleep(jobs.retention).await;
            jobs.lock().jobs.remove(&settled.id);
        });
        Ok(job)
    }
}

/// The file a download asks for: the whole export, or numbered part `part`.
///
/// # Errors
/// 404 for a part that doesn't exist.
pub(crate) fn archive_part<'a>(
    result: &'a ExportResult,
    part: Option<&str>,
) -> Result<&'a Archive> {
    let Some(raw) = part.filter(|raw| !raw.is_empty()) else {
        return Ok(result.download());
    };
    raw.parse::<usize>()
        .ok()
        .filter(|number| *number >= 1)
        .and_then(|number| result.archives.get(number - 1))
        .ok_or_else(|| refused(404, "That export ZIP part does not exist."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::archive::tests::report;
    use crate::refused::status_of;
    use anyhow::anyhow;
    use tokio::sync::oneshot;

    fn result(parts: usize) -> ExportResult {
        let archive = |filename: String| Archive {
            filename,
            body: vec![1, 2, 3],
            content_type: "application/zip",
            files: Vec::new(),
        };
        let mut report = report();
        report.warnings = vec!["Card: missing".into()];
        ExportResult {
            archives: (1..=parts)
                .map(|part| archive(format!("acme-part-{part}.zip")))
                .collect(),
            bundle: (parts > 1).then(|| archive("acme-parts.zip".into())),
            report,
        }
    }

    async fn settle(job: &Job) -> Value {
        job.wait().await;
        job.status()
    }

    #[tokio::test]
    async fn a_job_reports_progress_then_its_download_and_parts() {
        let jobs = Jobs::new(None);
        let (release, released) = oneshot::channel::<()>();
        let job = jobs
            .begin("a".into(), 6, 0, |progress| async move {
                progress.completed(2);
                let _released = released.await;
                Ok(result(2))
            })
            .expect("job");
        tokio::task::yield_now().await;
        let running = job.status();
        assert_eq!(
            (
                running["status"].clone(),
                running["total"].clone(),
                running["download"].clone()
            ),
            (json!("running"), json!(6), Value::Null)
        );
        let _sent = release.send(());
        let status = settle(&job).await;
        assert_eq!(status["status"], "complete");
        assert_eq!(status["completed"], 6);
        assert_eq!(status["warnings"], 1);
        assert_eq!(status["warningMessages"], json!(["Card: missing"]));
        assert_eq!(
            status["download"],
            json!({ "filename": "acme-parts.zip", "bytes": 3 })
        );
        assert_eq!(
            status["parts"][1],
            json!({ "number": 2, "filename": "acme-part-2.zip", "bytes": 3 })
        );
        let done = job.result().expect("result");
        assert_eq!(
            archive_part(&done, None).expect("download").filename,
            "acme-parts.zip"
        );
        assert_eq!(
            archive_part(&done, Some("2")).expect("part").filename,
            "acme-part-2.zip"
        );
        assert_eq!(
            status_of(&archive_part(&done, Some("3")).expect_err("missing"), 400),
            404
        );
        assert_eq!(
            status_of(&archive_part(&done, Some("0")).expect_err("zero"), 400),
            404
        );
    }

    #[tokio::test]
    async fn overlapping_identical_exports_share_a_job_and_a_different_one_is_refused() {
        let jobs = Jobs::new(None);
        let (release, released) = oneshot::channel::<()>();
        let first = jobs
            .begin("same".into(), 1, 0, |_| async move {
                let _released = released.await;
                Ok(result(1))
            })
            .expect("first");
        let second = jobs
            .begin("same".into(), 1, 0, |_| async { Ok(result(1)) })
            .expect("second");
        assert_eq!(first.id(), second.id());
        let different = jobs
            .begin("other".into(), 1, 0, |_| async { Ok(result(1)) })
            .expect_err("busy");
        assert_eq!(status_of(&different, 500), 409);
        let _sent = release.send(());
        first.wait().await;
        assert!(
            jobs.begin("other".into(), 1, 0, |_| async { Ok(result(1)) })
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_failed_job_says_why() {
        let jobs = Jobs::new(None);
        let job = jobs
            .begin("a".into(), 1, 2, |_| async {
                Err(anyhow!("capture helper stopped"))
            })
            .expect("job");
        let status = settle(&job).await;
        assert_eq!(
            (
                status["ok"].clone(),
                status["status"].clone(),
                status["error"].clone()
            ),
            (
                json!(false),
                json!("failed"),
                json!("capture helper stopped")
            )
        );
        assert_eq!(status["warnings"], 2);
        assert_eq!(status["parts"], json!([]));
    }

    #[tokio::test]
    async fn retains_a_slow_job_until_after_it_settles_then_forgets_it() {
        let jobs = Jobs::new(Some(Duration::from_millis(100)));
        let (release, released) = oneshot::channel::<()>();
        let job = jobs
            .begin("a".into(), 1, 0, |_| async move {
                let _released = released.await;
                Ok(result(1))
            })
            .expect("job");
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(jobs.get(job.id()).is_some());
        let _sent = release.send(());
        job.wait().await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(jobs.get(job.id()).is_none());
    }

    #[tokio::test]
    async fn bounds_retained_completed_bundles_to_the_latest_settled_one() {
        let jobs = Jobs::new(None);
        let mut ids = Vec::new();
        for _ in 0..3 {
            let job = jobs
                .begin("a".into(), 1, 0, |_| async { Ok(result(1)) })
                .expect("job");
            job.wait().await;
            tokio::task::yield_now().await;
            ids.push(job.id().to_owned());
        }
        assert!(jobs.get(&ids[0]).is_none());
        assert!(jobs.get(&ids[1]).is_some());
        assert!(jobs.get(&ids[2]).is_some());
    }
}
