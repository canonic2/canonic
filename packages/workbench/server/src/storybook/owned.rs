//! A Storybook the server runs itself.
//!
//! A Storybook implementation without a `url` is the Workbench server's: it
//! starts the project's installed `storybook dev --preview-only` when a page
//! first asks for a story, on a local socket instead of a port (the preload
//! script moves every listen there), and reaches it only through the
//! implementation's proxy. A Storybook the user runs, on any port, is never
//! in the way. The process starts again on the next request after it exits,
//! and stops with the server.

use super::fetch;
use anyhow::{Result, anyhow};
use futures::future::{BoxFuture, FutureExt as _, Shared};
use regex::Regex;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

const FLAGS: [&str; 5] = [
    "--ci",
    "--no-open",
    "--quiet",
    "--no-version-updates",
    "--disable-telemetry",
];
/// How much of the process's output is kept for a failure's explanation.
const KEPT_OUTPUT: usize = 4000;
const SHOWN_OUTPUT: usize = 1500;
const POLL: Duration = Duration::from_millis(150);
const STOP_GRACE: Duration = Duration::from_secs(3);

/// Something to note in the server's diagnostics: an event and its details.
pub(crate) type Log = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// How to run one project's Storybook.
#[derive(Clone)]
pub(crate) struct Options {
    /// The folder Storybook runs in: the one with `.storybook/`.
    pub(crate) root: PathBuf,
    /// The Node runtime that runs it.
    pub(crate) node: PathBuf,
    /// `socket-listen.cjs`, loaded with `--require`.
    pub(crate) preload: PathBuf,
    /// How long a start may take before it is reported.
    pub(crate) timeout: Duration,
    pub(crate) log: Option<Log>,
}

impl Options {
    pub(crate) fn new(root: PathBuf, node: PathBuf, preload: PathBuf) -> Self {
        Self {
            root,
            node,
            preload,
            timeout: Duration::from_mins(2),
            log: None,
        }
    }
}

type Starting = Shared<BoxFuture<'static, Result<PathBuf, String>>>;

struct Run {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct State {
    closed: bool,
    starting: Option<Starting>,
    generation: u64,
    run: Option<Run>,
}

struct Inner {
    options: Options,
    state: Mutex<State>,
}

impl Inner {
    fn log(&self, event: &str, details: Value) {
        if let Some(log) = &self.options.log {
            log(event, details);
        }
    }

    /// Forgets a start once its process is gone, so the next request starts another.
    fn forget(&self, generation: u64) {
        let mut state = self.state.lock().expect("storybook state");
        if state.generation == generation {
            state.starting = None;
        }
    }
}

/// One project's Storybook, started on need and stopped with the server.
#[derive(Clone)]
pub(crate) struct OwnedStorybook(Arc<Inner>);

impl OwnedStorybook {
    pub(crate) fn new(options: Options) -> Self {
        Self(Arc::new(Inner {
            options,
            state: Mutex::default(),
        }))
    }

    /// The socket the running Storybook answers on, starting it, told the
    /// proxy's `port`, when it isn't running.
    ///
    /// # Errors
    /// When it is closed, isn't installed, stops or isn't ready in time.
    pub(crate) async fn socket(&self, port: u16) -> Result<PathBuf> {
        let starting = {
            let mut state = self.0.state.lock().expect("storybook state");
            if state.closed {
                return Err(anyhow!("Workbench has stopped this Storybook"));
            }
            if let Some(starting) = &state.starting {
                starting.clone()
            } else {
                state.generation += 1;
                let (ready, answer) = oneshot::channel();
                let (stop, stopped) = watch::channel(false);
                let task =
                    tokio::spawn(run(self.0.clone(), state.generation, port, ready, stopped));
                state.run = Some(Run { stop, task });
                let starting = async move {
                    answer
                        .await
                        .unwrap_or_else(|_| Err("Workbench has stopped this Storybook".to_owned()))
                }
                .boxed()
                .shared();
                state.starting = Some(starting.clone());
                starting
            }
        };
        starting.await.map_err(|error| anyhow!(error))
    }

    /// Stops the Storybook and removes its socket folder.
    pub(crate) async fn close(&self) {
        let run = {
            let mut state = self.0.state.lock().expect("storybook state");
            state.closed = true;
            state.starting = None;
            state.run.take()
        };
        if let Some(run) = run {
            let _sent = run.stop.send(true);
            let _done = run.task.await;
        }
    }
}

/// The project's Storybook command line script, from its own installation,
/// found as Node finds `storybook/package.json` from the folder.
///
/// # Errors
/// When Storybook isn't installed there or has no command line script.
pub(crate) fn storybook_bin(root: &Path) -> Result<PathBuf> {
    let not_installed = || {
        anyhow!(
            "Storybook isn’t installed in {}. Install storybook and a framework there, or set url to a Storybook you run.",
            root.display()
        )
    };
    let manifest = root
        .ancestors()
        .filter(|dir| dir.file_name().is_none_or(|name| name != "node_modules"))
        .map(|dir| dir.join("node_modules/storybook/package.json"))
        .find(|manifest| manifest.is_file())
        .ok_or_else(not_installed)?;
    let manifest = std::fs::canonicalize(&manifest).unwrap_or(manifest);
    let text = std::fs::read_to_string(&manifest).map_err(|_| not_installed())?;
    let package: Value =
        serde_json::from_str(&text).map_err(|error| anyhow!("{}: {error}", manifest.display()))?;
    let named = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|file| !file.is_empty())
            .map(str::to_owned)
    };
    let bin = package.get("bin");
    let file = match bin {
        Some(Value::String(file)) if !file.is_empty() => Some(file.clone()),
        Some(Value::Object(bins)) => named(bins.get("storybook")).or_else(|| named(bins.get("sb"))),
        _ => None,
    }
    .ok_or_else(|| anyhow!("The installed storybook package has no command line script."))?;
    let folder = manifest.parent().unwrap_or(root);
    Ok(folder.join(file))
}

enum Launch {
    Ready(Child, PathBuf, PathBuf),
    /// This Storybook predates `--preview-only`.
    Rejected,
    Failed(String),
    Stopped,
}

async fn run(
    inner: Arc<Inner>,
    generation: u64,
    port: u16,
    ready: oneshot::Sender<Result<PathBuf, String>>,
    mut stop: watch::Receiver<bool>,
) {
    let mut preview_only = true;
    loop {
        match launch(&inner, port, preview_only, &mut stop).await {
            Launch::Rejected => preview_only = false,
            Launch::Failed(error) => {
                inner.forget(generation);
                let _sent = ready.send(Err(error));
                return;
            }
            Launch::Stopped => {
                let _sent = ready.send(Err("Workbench has stopped this Storybook".to_owned()));
                return;
            }
            Launch::Ready(mut child, dir, socket) => {
                let _sent = ready.send(Ok(socket));
                tokio::select! {
                    status = child.wait() => {
                        remove(&dir);
                        inner.log("storybook.exited", json!({ "code": status.ok().and_then(|status| status.code()), "root": inner.options.root }));
                        // A Storybook that was running starts again on the next request.
                        inner.forget(generation);
                    }
                    _ = stop.changed() => {
                        terminate(&mut child).await;
                        remove(&dir);
                    }
                }
                return;
            }
        }
    }
}

static REJECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)unknown option|unknown argument").expect("rejection pattern")
});

async fn launch(
    inner: &Inner,
    port: u16,
    preview_only: bool,
    stop: &mut watch::Receiver<bool>,
) -> Launch {
    let options = &inner.options;
    let bin = match storybook_bin(&options.root) {
        Ok(bin) => bin,
        Err(error) => return Launch::Failed(error.to_string()),
    };
    let dir = match private_folder() {
        Ok(dir) => dir,
        Err(error) => {
            return Launch::Failed(format!(
                "Couldn’t make a folder for Storybook’s socket: {error}"
            ));
        }
    };
    let mut command = Command::new(&options.node);
    command
        .arg("--require")
        .arg(&options.preload)
        .arg(&bin)
        .args(["dev", "-p", &port.to_string(), "--exact-port"])
        .args(preview_only.then_some("--preview-only"))
        .args(FLAGS)
        .env("WORKBENCH_STORYBOOK_SOCKETS", &dir)
        // The bundled Electron runs scripts as Node only when told to; Node ignores it.
        .env("ELECTRON_RUN_AS_NODE", "1")
        .current_dir(&options.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            remove(&dir);
            return Launch::Failed(format!(
                "Couldn’t start Storybook with {}: {error}",
                options.node.display()
            ));
        }
    };
    let output = Arc::new(Mutex::new(String::new()));
    let readers = [
        child.stdout.take().map(|out| keep(out, output.clone())),
        child.stderr.take().map(|err| keep(err, output.clone())),
    ];
    inner.log(
        "storybook.starting",
        json!({ "root": options.root, "port": port, "previewOnly": preview_only }),
    );
    let tail = |output: &Mutex<String>| {
        let text = output.lock().expect("storybook output");
        let trimmed: Vec<char> = text.trim().chars().collect();
        trimmed[trimmed.len().saturating_sub(SHOWN_OUTPUT)..]
            .iter()
            .collect::<String>()
    };
    let mut poll = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            status = child.wait() => {
                // What it said before stopping is the explanation.
                for reader in readers.into_iter().flatten() {
                    let _read = tokio::time::timeout(Duration::from_secs(1), reader).await;
                }
                remove(&dir);
                let code = status.ok().and_then(|status| status.code());
                inner.log("storybook.exited", json!({ "code": code, "root": options.root }));
                let said = output.lock().expect("storybook output").clone();
                if preview_only && said.contains("preview-only") && REJECTED.is_match(&said) {
                    return Launch::Rejected;
                }
                let code = code.map_or_else(|| "null".to_owned(), |code| code.to_string());
                return Launch::Failed(format!("Storybook stopped before it was ready (exit {code}):\n{}", tail(&output)));
            }
            _ = stop.changed() => {
                terminate(&mut child).await;
                remove(&dir);
                return Launch::Stopped;
            }
            _ = poll.tick() => {
                if started.elapsed() > options.timeout {
                    let _killed = child.kill().await;
                    remove(&dir);
                    return Launch::Failed(format!(
                        "Storybook didn’t become ready within {}s:\n{}",
                        (options.timeout.as_secs_f64()).round(),
                        tail(&output)
                    ));
                }
                let Ok(socket) = std::fs::read_to_string(dir.join("server")) else { continue };
                let socket = PathBuf::from(socket);
                if answers(&socket).await {
                    inner.log("storybook.ready", json!({ "root": options.root, "ms": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX) }));
                    return Launch::Ready(child, dir, socket);
                }
            }
        }
    }
}

/// Whether the Storybook on `socket` serves its story index.
async fn answers(socket: &Path) -> bool {
    fetch::get_socket(socket, "/index.json", Duration::from_secs(2))
        .await
        .is_ok_and(|answer| answer.status == 200)
}

fn keep(
    mut stream: impl AsyncRead + Unpin + Send + 'static,
    output: Arc<Mutex<String>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut chunk = [0u8; 4096];
        while let Ok(read) = stream.read(&mut chunk).await {
            if read == 0 {
                break;
            }
            let mut text = output.lock().expect("storybook output");
            text.push_str(&String::from_utf8_lossy(&chunk[..read]));
            let count = text.chars().count();
            if count > KEPT_OUTPUT {
                *text = text.chars().skip(count - KEPT_OUTPUT).collect();
            }
        }
    })
}

/// Asks the process to stop, then makes it after a grace period.
async fn terminate(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        let asked = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .await;
        if asked.is_ok_and(|status| status.success())
            && tokio::time::timeout(STOP_GRACE, child.wait()).await.is_ok()
        {
            return;
        }
    }
    let _killed = child.kill().await;
}

/// A new folder only this user can open, with a name short enough that a
/// socket path inside it stays under the platform's length limit.
fn private_folder() -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir();
    let mut attempt = 0;
    loop {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.subsec_nanos())
            ^ std::process::id().rotate_left(13)
            ^ u32::try_from(NEXT.fetch_add(1, Ordering::Relaxed) % u64::from(u32::MAX))
                .unwrap_or(0)
                .rotate_left(7);
        let dir = base.join(format!("wb-sb-{seed:08x}"));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 16 => {
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

fn remove(dir: &Path) {
    let _gone = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRELOAD: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/node/src/modules/storybook/socket-listen.cjs"
    );

    /// A project with a stand-in `storybook` package: its script listens on
    /// the port it is given, answers /index.json with that port, and records
    /// how it was run.
    struct Project {
        root: PathBuf,
    }

    impl Project {
        fn new(name: &str, reject_preview_only: bool) -> Self {
            let root =
                std::env::temp_dir().join(format!("wb-storybook-{name}-{}", std::process::id()));
            let _gone = std::fs::remove_dir_all(&root);
            let package = root.join("node_modules/storybook");
            std::fs::create_dir_all(package.join("bin")).expect("package folder");
            std::fs::write(
                root.join("package.json"),
                r#"{"name":"acme","private":true}"#,
            )
            .expect("project");
            std::fs::write(
                package.join("package.json"),
                r#"{"name":"storybook","version":"10.0.0","bin":"./bin/storybook.cjs"}"#,
            )
            .expect("package");
            let runs = serde_json::to_string(&root.join("runs.log")).expect("path");
            std::fs::write(
                package.join("bin/storybook.cjs"),
                format!(
                    r"
                    const http = require('node:http');
                    const fs = require('node:fs');
                    const args = process.argv.slice(2);
                    if ({reject_preview_only} && args.includes('--preview-only')) {{ console.error(`error: unknown option '--preview-only'`); process.exit(1); }}
                    const port = Number(args[args.indexOf('-p') + 1]);
                    fs.appendFileSync({runs}, JSON.stringify({{ args, port, pid: process.pid }}) + '\n');
                    http.createServer((req, res) => {{
                      res.writeHead(200, {{ 'Content-Type': 'application/json' }});
                      res.end(JSON.stringify({{ v: 5, entries: {{}}, port, url: req.url }}));
                    }}).listen(port, '127.0.0.1');
                    "
                ),
            )
            .expect("script");
            Self { root }
        }

        fn runs(&self) -> Vec<Value> {
            std::fs::read_to_string(self.root.join("runs.log"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).expect("run"))
                .collect()
        }

        fn storybook(&self) -> OwnedStorybook {
            let mut options = Options::new(
                self.root.clone(),
                PathBuf::from("node"),
                PathBuf::from(PRELOAD),
            );
            options.timeout = Duration::from_secs(20);
            OwnedStorybook::new(options)
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _gone = std::fs::remove_dir_all(&self.root);
        }
    }

    async fn index_port(socket: &Path) -> u64 {
        let answer = fetch::get_socket(socket, "/index.json", Duration::from_secs(5))
            .await
            .expect("index");
        serde_json::from_str::<Value>(&answer.body).expect("json")["port"]
            .as_u64()
            .expect("port")
    }

    fn has(run: &Value, arg: &str) -> bool {
        run["args"]
            .as_array()
            .expect("args")
            .iter()
            .any(|value| value == arg)
    }

    #[tokio::test]
    async fn runs_the_projects_storybook_on_a_local_socket_told_the_proxys_port() {
        let project = Project::new("socket", false);
        let storybook = project.storybook();
        let socket = storybook.socket(47123).await.expect("started");
        assert_eq!(index_port(&socket).await, 47123);
        assert!(
            tokio::net::TcpStream::connect(("127.0.0.1", 47123))
                .await
                .is_err(),
            "nothing listens on the port it was given"
        );
        assert_eq!(
            storybook.socket(47123).await.expect("again"),
            socket,
            "one Storybook, started once"
        );
        let runs = project.runs();
        assert_eq!(runs.len(), 1);
        assert!(
            has(&runs[0], "--preview-only")
                && has(&runs[0], "--exact-port")
                && has(&runs[0], "--no-open")
        );
        storybook.close().await;
        assert!(!socket.exists(), "closing removes its socket folder");
    }

    #[tokio::test]
    async fn starts_again_after_it_exits_and_stays_stopped_once_closed() {
        let project = Project::new("restart", false);
        let storybook = project.storybook();
        let first = storybook.socket(47124).await.expect("started");
        let pid = project.runs()[0]["pid"].as_u64().expect("pid").to_string();
        Command::new("kill").arg(&pid).status().await.expect("kill");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let second = storybook.socket(47124).await.expect("started again");
        assert_eq!(project.runs().len(), 2, "a second Storybook was started");
        assert_eq!(index_port(&second).await, 47124);
        assert!(!first.exists(), "the old socket is gone");
        storybook.close().await;
        let error = storybook.socket(47124).await.expect_err("closed");
        assert!(error.to_string().contains("stopped this Storybook"));
    }

    #[tokio::test]
    async fn falls_back_to_a_full_dev_server_where_preview_only_is_unknown() {
        let project = Project::new("fallback", true);
        let storybook = project.storybook();
        let socket = storybook.socket(47125).await.expect("started");
        assert_eq!(index_port(&socket).await, 47125);
        let runs = project.runs();
        assert_eq!(runs.len(), 1, "the rejected launch never got to listen");
        assert!(!has(&runs[0], "--preview-only"));
        storybook.close().await;
    }

    #[tokio::test]
    async fn a_storybook_that_stops_before_it_is_ready_says_why_and_starts_again_next_time() {
        let project = Project::new("crash", false);
        std::fs::write(
            project
                .root
                .join("node_modules/storybook/bin/storybook.cjs"),
            "console.error('acme: no framework'); process.exit(3);",
        )
        .expect("failing script");
        let storybook = project.storybook();
        let error = storybook.socket(47126).await.expect_err("stopped");
        assert_eq!(
            error.to_string(),
            "Storybook stopped before it was ready (exit 3):\nacme: no framework"
        );
        let again = storybook.socket(47126).await.expect_err("tried again");
        assert!(again.to_string().contains("exit 3"));
        storybook.close().await;
    }

    #[test]
    fn names_a_project_without_storybook_installed() {
        let root = std::env::temp_dir().join(format!("wb-storybook-none-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("folder");
        let error = storybook_bin(&root).expect_err("not installed");
        assert_eq!(
            error.to_string(),
            format!(
                "Storybook isn’t installed in {}. Install storybook and a framework there, or set url to a Storybook you run.",
                root.display()
            )
        );
        let _gone = std::fs::remove_dir_all(root);
    }

    #[test]
    fn finds_the_command_line_script_an_ancestor_installs() {
        let project = Project::new("ancestor", false);
        let nested = project.root.join("apps/docs");
        std::fs::create_dir_all(&nested).expect("nested");
        let bin = storybook_bin(&nested).expect("found upward");
        assert!(
            bin.ends_with("node_modules/storybook/bin/storybook.cjs"),
            "{}",
            bin.display()
        );
    }
}
