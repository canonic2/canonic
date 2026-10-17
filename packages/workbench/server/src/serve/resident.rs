//! The resident page index: index requests answered in this process, and a
//! watcher that re-indexes in the background after a change and reports when
//! the answer changed.
//!
//! While the watcher is healthy and nothing has changed since the last answer,
//! a request for that answer is reused without reading the disk. A change in
//! the answer's scope, or a watcher error, sends the next request through the
//! full index check. The scope is what the last answer can depend on: the
//! walk's folders, the files its definitions read and the configuration.
use super::messages::Output;
use crate::{
    discovery::{self, Includes},
    index,
    model::Request,
    protocol::Response,
    saved::Saved,
};
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::json;
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;

/// Quiet time after a change before re-indexing, so one save or one checkout
/// is one background index.
const SETTLE: Duration = Duration::from_millis(150);

pub(crate) struct Resident {
    shared: Arc<Shared>,
}

struct Shared {
    output: Output,
    /// Changes seen by the watcher; an answer is clean for the count it saw.
    changes: AtomicU64,
    state: Mutex<State>,
    /// What the last answer depends on; `None` counts every change.
    scope: Mutex<Option<Scope>>,
    /// The listed records as last announced: the first answer, then each
    /// `changed`. A request that already fetched a newer answer does not
    /// update it, so the change is still announced.
    announced: Mutex<Option<String>>,
    /// One index at a time: they share the saved index on disk.
    running: tokio::sync::Mutex<()>,
    signal: mpsc::UnboundedSender<()>,
}

#[derive(Default)]
struct State {
    watching: Option<Watching>,
    /// The latest request, which the background index repeats.
    last: Option<Request>,
    clean: Option<Clean>,
}

struct Watching {
    root: PathBuf,
    healthy: Arc<AtomicBool>,
    _watcher: Option<RecommendedWatcher>,
}

struct Clean {
    key: Key,
    generation: String,
    changes: u64,
}

/// Paths whose changes can alter an answer.
struct Scope {
    root: PathBuf,
    /// Folders the walk starts from; what the walk can reach below them counts.
    folders: Vec<PathBuf>,
    /// Files the definitions read, the configuration and the `.gitignore`
    /// files that applied.
    files: HashSet<PathBuf>,
    /// The rules of those `.gitignore` files, outermost first.
    rules: discovery::Rules,
}

impl Scope {
    /// The scope of the answer with `generation`, read back from its save.
    /// When that save is missing or older (it failed), or a definition's reads
    /// are unknown, there is no scope and every change counts.
    fn of(request: &Request, generation: &str) -> Option<Self> {
        let includes = Includes::read(&request.options).ok()?;
        let saved = Saved::load(&request.saved, &request.identity);
        if saved.generation != generation
            || saved
                .records
                .values()
                .any(|record| record.watched.is_empty())
        {
            return None;
        }
        let root = std::fs::canonicalize(&request.root).unwrap_or_else(|_| request.root.clone());
        let folders = discovery::bases(includes.native())
            .into_iter()
            .map(|base| {
                if base.is_empty() {
                    root.clone()
                } else {
                    root.join(base)
                }
            })
            .collect();
        let mut files: HashSet<PathBuf> = saved
            .records
            .values()
            .flat_map(|record| record.watched.iter().map(|(file, _)| root.join(file)))
            .chain(saved.settings.iter().map(|(file, _)| root.join(file)))
            .chain(saved.ignores.iter().map(|(file, _)| root.join(file)))
            .collect();
        let mut rules: discovery::Rules = saved
            .ignores
            .iter()
            .filter_map(|(file, _)| discovery::gitignore_in(root.join(file).parent()?))
            .collect();
        rules.sort_by_key(|rules| rules.path().components().count());
        files.insert(
            root.join(
                request
                    .options
                    .get("config")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("workbench.config.ts"),
            ),
        );
        Some(Self {
            root,
            folders,
            files,
            rules,
        })
    }

    /// A file a definition read, or a path the walk can reach: below a folder,
    /// not inside a folder the walk skips and not ignored. A `.gitignore` the
    /// walk can reach counts too, as it changes what the walk lists.
    fn contains(&self, path: &Path) -> bool {
        if self.files.contains(path) {
            return true;
        }
        let gitignore = path.file_name().is_some_and(|name| name == ".gitignore");
        self.folders.iter().any(|folder| {
            path.strip_prefix(folder).is_ok_and(|rest| {
                let parts: Vec<_> = rest.components().collect();
                parts.iter().enumerate().all(|(at, part)| match part {
                    Component::Normal(name) => {
                        (gitignore && at + 1 == parts.len())
                            || !discovery::skipped(&name.to_string_lossy())
                    }
                    _ => true,
                })
            })
        }) && !self.ignored(path)
    }

    /// Whether `.gitignore` rules leave out this path or a folder above it.
    fn ignored(&self, path: &Path) -> bool {
        let Ok(rest) = path.strip_prefix(&self.root) else {
            return false;
        };
        let parts: Vec<_> = rest.components().collect();
        let mut current = self.root.clone();
        for (at, part) in parts.iter().enumerate() {
            current.push(part);
            let is_dir = at + 1 < parts.len() || current.is_dir();
            let above: discovery::Rules = self
                .rules
                .iter()
                .filter(|rules| current.starts_with(rules.path()) && current != rules.path())
                .cloned()
                .collect();
            if discovery::ignored(&above, &current, is_dir) {
                return true;
            }
        }
        false
    }
}

/// What makes two requests ask for the same answer.
#[derive(PartialEq, Eq)]
struct Key {
    root: PathBuf,
    identity: String,
    options: String,
}

impl Key {
    fn of(request: &Request) -> Self {
        Self {
            root: request.root.clone(),
            identity: request.identity.clone(),
            options: request.options.to_string(),
        }
    }
}

impl Resident {
    pub(crate) fn new(output: Output) -> Self {
        let (signal, settled) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            output,
            changes: AtomicU64::new(0),
            state: Mutex::new(State::default()),
            scope: Mutex::new(None),
            announced: Mutex::new(None),
            running: tokio::sync::Mutex::new(()),
            signal,
        });
        tokio::spawn(settle(Arc::downgrade(&shared), settled));
        Self { shared }
    }

    pub(crate) async fn index(&self, mut request: Request) -> Result<Response> {
        let shared = &self.shared;
        {
            let mut state = lock(&shared.state);
            shared.watch(&mut state, &request.root);
            let healthy = state
                .watching
                .as_ref()
                .is_some_and(|watching| watching.healthy.load(Ordering::SeqCst));
            if healthy
                && let Some(clean) = &state.clean
                && clean.key == Key::of(&request)
                && clean.changes == shared.changes.load(Ordering::SeqCst)
                && clean.generation == request.cached_generation
            {
                return Ok(Response::reused(
                    clean.generation.clone(),
                    json!({ "resident": true }),
                ));
            }
            // The background index needs the evaluator a previous request
            // supplied, even when this one starts without it.
            if request.node.as_os_str().is_empty()
                && let Some(last) = &state.last
                && Key::of(last) == Key::of(&request)
            {
                request.node.clone_from(&last.node);
            }
            state.last = Some(request.clone());
        }
        // A reused answer carries no records; the first one here lists them,
        // so later changes have something to be compared with.
        if lock(&shared.announced).is_none() {
            request.cached_generation.clear();
        }
        shared.run(request).await
    }
}

impl Shared {
    async fn run(&self, request: Request) -> Result<Response> {
        let _one = self.running.lock().await;
        let before = self.changes.load(Ordering::SeqCst);
        let key = Key::of(&request);
        let (response, scope) = tokio::task::spawn_blocking(move || {
            let response = index::answer(&request)?;
            let scope = response
                .generation()
                .and_then(|generation| Scope::of(&request, generation));
            anyhow::Ok((response, scope))
        })
        .await??;
        if response.generation().is_some() {
            *lock(&self.scope) = scope;
        }
        if let Some(records) = response.fingerprint() {
            lock(&self.announced).get_or_insert(records);
        }
        if let Some(generation) = response.generation()
            && self.changes.load(Ordering::SeqCst) == before
        {
            lock(&self.state).clean = Some(Clean {
                key,
                generation: generation.to_owned(),
                changes: before,
            });
        }
        Ok(response)
    }

    fn changed(&self) {
        self.changes.fetch_add(1, Ordering::SeqCst);
        // The receiver lives as long as the process.
        let _queued = self.signal.send(());
    }

    /// Watches the request's root, once per root. A watcher that cannot start
    /// stays unhealthy, so every request takes the full index check.
    fn watch(self: &Arc<Self>, state: &mut State, root: &Path) {
        if state
            .watching
            .as_ref()
            .is_some_and(|watching| watching.root == root)
        {
            return;
        }
        state.clean = None;
        let healthy = Arc::new(AtomicBool::new(true));
        let shared = Arc::downgrade(self);
        let flag = healthy.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Some(shared) = shared.upgrade() else {
                return;
            };
            match event {
                Ok(event) if relevant(&event, lock(&shared.scope).as_ref()) => shared.changed(),
                Ok(_) => {}
                Err(_) => {
                    flag.store(false, Ordering::SeqCst);
                    shared.changed();
                }
            }
        })
        .and_then(|mut watcher| {
            watcher.watch(root, RecursiveMode::Recursive)?;
            Ok(watcher)
        });
        if watcher.is_err() {
            healthy.store(false, Ordering::SeqCst);
        }
        state.watching = Some(Watching {
            root: root.to_path_buf(),
            healthy,
            _watcher: watcher.ok(),
        });
    }
}

/// Re-indexes after changes settle and reports an answer that differs.
async fn settle(shared: Weak<Shared>, mut settled: mpsc::UnboundedReceiver<()>) {
    while settled.recv().await.is_some() {
        loop {
            tokio::select! {
                more = settled.recv() => if more.is_none() { return },
                () = tokio::time::sleep(SETTLE) => break,
            }
        }
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let request = lock(&shared.state).last.clone();
        let Some(mut request) = request else { continue };
        // The full list, not a reuse, so it can be compared. A new generation
        // alone is not a change: editing a preview's source renews it while
        // the page tree stays the same.
        request.cached_generation.clear();
        let report = match shared.run(request).await {
            Ok(response) if response.needs_evaluator() => true,
            Ok(response) => {
                let current = response.fingerprint();
                let mut announced = lock(&shared.announced);
                let differs = current.is_some() && current != *announced;
                if differs {
                    *announced = current;
                }
                differs
            }
            // The Node server asks again and reports the error itself.
            Err(_) => true,
        };
        if report {
            let _sent = shared.output.changed().await;
        }
    }
}

/// A change that can alter the answer: a rescan, which means events were
/// dropped, or a change that is not a read, not Git's own bookkeeping, and
/// inside the answer's scope when one is known.
fn relevant(event: &Event, scope: Option<&Scope>) -> bool {
    if event.need_rescan() {
        return true;
    }
    !matches!(event.kind, EventKind::Access(_))
        && event.paths.iter().any(|path| {
            !path
                .components()
                .any(|part| part == Component::Normal(".git".as_ref()))
                && scope.is_none_or(|scope| scope.contains(path))
        })
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{Scope, relevant};
    use notify::{
        Event, EventKind,
        event::{AccessKind, CreateKind, Flag},
    };
    use std::path::PathBuf;

    fn at(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }
    const CREATE: EventKind = EventKind::Create(CreateKind::File);

    #[test]
    fn without_a_scope_every_change_but_reads_and_git_counts() {
        assert!(relevant(&at(CREATE, "/acme/src/a.workbench.ts"), None));
        assert!(relevant(
            &at(CREATE, "/acme/node_modules/acme/titles.js"),
            None
        ));
        assert!(!relevant(&at(CREATE, "/acme/.git/index.lock"), None));
        assert!(!relevant(
            &at(
                EventKind::Access(AccessKind::Any),
                "/acme/src/a.workbench.ts"
            ),
            None
        ));
    }

    #[test]
    fn a_scope_counts_its_folders_and_the_files_definitions_read() {
        let scope = Scope {
            root: PathBuf::from("/acme"),
            rules: vec![],
            folders: vec![PathBuf::from("/acme/demo")],
            files: [
                PathBuf::from("/acme/src/titles.ts"),
                PathBuf::from("/acme/workbench.config.ts"),
            ]
            .into(),
        };
        assert!(relevant(
            &at(CREATE, "/acme/demo/new/b.workbench.ts"),
            Some(&scope)
        ));
        assert!(relevant(&at(CREATE, "/acme/src/titles.ts"), Some(&scope)));
        assert!(relevant(
            &at(CREATE, "/acme/workbench.config.ts"),
            Some(&scope)
        ));
        assert!(!relevant(
            &at(CREATE, "/acme/target/debug/deps/x.o"),
            Some(&scope)
        ));
        assert!(!relevant(&at(CREATE, "/acme/src/other.ts"), Some(&scope)));
        // The walk never enters these, so only a file a definition read counts.
        assert!(!relevant(
            &at(CREATE, "/acme/demo/node_modules/x.js"),
            Some(&scope)
        ));
        assert!(!relevant(&at(CREATE, "/acme/demo/.cache/x"), Some(&scope)));
        // Dropped events: whatever path the rescan names.
        let rescan = Event::new(EventKind::Other)
            .set_flag(Flag::Rescan)
            .add_path(PathBuf::from("/acme"));
        assert!(relevant(&rescan, Some(&scope)));
    }

    #[test]
    fn ignored_paths_are_outside_the_scope_and_gitignore_edits_inside() {
        let root = std::env::temp_dir().join(format!("workbench-scope-{}", std::process::id()));
        let _cleared = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        let scope = Scope {
            root: root.clone(),
            folders: vec![root.clone()],
            files: [root.join(".gitignore")].into(),
            rules: crate::discovery::gitignore_in(&root).into_iter().collect(),
        };
        let path = |file: &str| root.join(file).to_string_lossy().into_owned();
        assert!(!relevant(
            &at(CREATE, &path("target/debug/x.o")),
            Some(&scope)
        ));
        assert!(!relevant(
            &at(CREATE, &path("demo/build.log")),
            Some(&scope)
        ));
        assert!(relevant(
            &at(CREATE, &path("demo/a.workbench.ts")),
            Some(&scope)
        ));
        assert!(relevant(&at(CREATE, &path(".gitignore")), Some(&scope)));
        assert!(relevant(
            &at(CREATE, &path("demo/.gitignore")),
            Some(&scope)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
