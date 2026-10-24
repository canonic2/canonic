//! A Storybook's story index: read at the Storybook's address, checked to be
//! one, and asked which stories carry a title and where their code is. Also
//! finding a running Storybook when its address is `auto`.

use super::fetch;
use crate::refused::refused;
use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// How long a Storybook the user runs may take to answer.
pub(crate) const REMOTE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a Storybook the server runs may take: its first request starts it.
pub(crate) const OWNED_TIMEOUT: Duration = Duration::from_secs(130);

/// The ports a Storybook usually takes, tried after any a project script names.
const DEFAULT_PORTS: [u16; 5] = [6006, 6007, 6008, 6009, 6010];

/// A Storybook implementation, as the resolved config describes it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Storybook<'a> {
    /// Its address: its own, or its proxy's for one the server runs.
    pub(crate) url: &'a str,
    /// Whether the server runs it, starting it on the first request.
    pub(crate) owned: bool,
    /// The folder it runs in, which story paths are relative to.
    pub(crate) root: Option<&'a Path>,
}

impl<'a> Storybook<'a> {
    /// The Storybook a resolved implementation describes: `url`, `owned` and `root`.
    pub(crate) fn of(implementation: &'a Value) -> Option<Self> {
        Some(Self {
            url: implementation.get("url")?.as_str()?,
            owned: implementation.get("owned").and_then(Value::as_bool) == Some(true),
            root: implementation
                .get("root")
                .and_then(Value::as_str)
                .map(Path::new),
        })
    }
}

/// One story under a title, with the files its code is in.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Story {
    pub(crate) id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<Value>,
    pub(crate) scenario: String,
    #[serde(rename = "importPath")]
    pub(crate) import_path: Option<Value>,
    #[serde(rename = "componentPath")]
    pub(crate) component_path: Option<Value>,
    pub(crate) code: Vec<PathBuf>,
}

/// The scenario a story ID names: what follows `--`, or the whole ID.
pub(crate) fn story_scenario(id: &str) -> &str {
    id.find("--").map_or(id, |at| &id[at + 2..])
}

/// The Storybook's index, checked to be one. Failures say what is wrong
/// with the status the route answers.
///
/// # Errors
/// 502 when it answers with something else, 503 when it isn't answering.
pub(crate) async fn index(storybook: Storybook<'_>) -> Result<Value> {
    let address = format!("{}/index.json", storybook.url);
    // A Storybook the server runs starts on this first request, so it may
    // take a while, and its failures are Workbench's to explain.
    if storybook.owned {
        let root = storybook
            .root
            .map_or_else(String::new, |root| root.display().to_string());
        let answer = fetch::get(&address, "*/*", OWNED_TIMEOUT)
            .await
            .map_err(|error| {
                refused(
                    503,
                    format!("Workbench’s Storybook in {root} isn’t answering: {error:#}"),
                )
            })?;
        if !(200..300).contains(&answer.status) {
            return Err(refused(
                502,
                format!(
                    "Workbench couldn’t run Storybook in {root}: {}",
                    answer.body.trim()
                ),
            ));
        }
        let body = serde_json::from_str(&answer.body).unwrap_or(Value::Null);
        entries(&body, storybook.url)?;
        return Ok(body);
    }
    let answer = fetch::get(&address, "application/json", REMOTE_TIMEOUT)
        .await
        .map_err(|_| {
            refused(
                503,
                format!(
                    "Storybook at {} isn’t answering — is it running?",
                    storybook.url
                ),
            )
        })?;
    if !(200..300).contains(&answer.status) {
        return Err(refused(
            502,
            format!(
                "Storybook at {} answered {} for /index.json.",
                storybook.url, answer.status
            ),
        ));
    }
    let body = serde_json::from_str(&answer.body).unwrap_or(Value::Null);
    entries(&body, storybook.url)?;
    Ok(body)
}

/// The index's stories: entries of type `story` with a string ID and title.
///
/// # Errors
/// 502 when `index` isn't a Storybook index.
pub(crate) fn entries<'v>(index: &'v Value, url: &str) -> Result<Vec<&'v Map<String, Value>>> {
    let all: Vec<&Value> = match index.get("entries") {
        Some(Value::Object(entries)) => entries.values().collect(),
        Some(Value::Array(entries)) => entries.iter().collect(),
        _ => {
            return Err(refused(
                502,
                format!("Storybook at {url} answered with something that isn’t a Storybook index."),
            ));
        }
    };
    Ok(all
        .into_iter()
        .filter_map(Value::as_object)
        .filter(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("story")
                && entry.get("id").is_some_and(Value::is_string)
                && entry.get("title").is_some_and(Value::is_string)
        })
        .collect())
}

fn text<'v>(entry: &'v Map<String, Value>, key: &str) -> &'v str {
    entry.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Which of the index's stories carry `title`. Exact: a title is what
/// Storybook shows, and "Button" is not "Components/Button", but the answer
/// says so when the difference is only the prefix.
///
/// # Errors
/// 502 when `index` isn't a Storybook index; 404 when no story has the title.
pub(crate) fn stories_titled(
    index: &Value,
    title: &str,
    storybook: Storybook<'_>,
) -> Result<Vec<Story>> {
    let all = entries(index, storybook.url)?;
    let found: Vec<_> = all
        .iter()
        .filter(|entry| text(entry, "title") == title)
        .collect();
    if found.is_empty() {
        let wanted = title.rsplit('/').next().unwrap_or(title);
        let mut near: Vec<&str> = Vec::new();
        for entry in &all {
            let candidate = text(entry, "title");
            if candidate.rsplit('/').next() == Some(wanted) && !near.contains(&candidate) {
                near.push(candidate);
            }
        }
        let tail = if near.is_empty() {
            format!(" at {}.", storybook.url)
        } else {
            format!(" — did you mean “{}”?", near.join("”, “"))
        };
        return Err(refused(404, format!("No stories titled “{title}”{tail}")));
    }
    Ok(found
        .into_iter()
        .map(|entry| {
            let id = text(entry, "id");
            let path = |key| entry.get(key).filter(|value| truthy(value)).cloned();
            Story {
                id: id.to_owned(),
                name: entry.get("name").cloned(),
                scenario: story_scenario(id).to_owned(),
                import_path: path("importPath"),
                component_path: path("componentPath"),
                code: story_code(storybook.root, entry),
            }
        })
        .collect())
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Where a story's code is: its component and story files, relative to the
/// folder Storybook runs in, kept when they exist.
fn story_code(root: Option<&Path>, entry: &Map<String, Value>) -> Vec<PathBuf> {
    let Some(root) = root else { return Vec::new() };
    let mut out = Vec::new();
    for key in ["componentPath", "importPath"] {
        let Some(relative) = entry
            .get(key)
            .and_then(Value::as_str)
            .filter(|rel| !rel.is_empty())
        else {
            continue;
        };
        let file = resolve(root, Path::new(relative));
        if !out.contains(&file) && file.exists() {
            out.push(file);
        }
    }
    out
}

/// `relative` against `root`, with `.` and `..` resolved as `path.resolve` does.
fn resolve(root: &Path, relative: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in root.join(relative).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

static PORT_FLAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:--port(?:=|\s+)|-p\s+)(\d{2,5})").expect("port pattern"));
static STORYBOOK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)storybook").expect("storybook pattern"));

/// The ports to look for a running Storybook on: those Storybook scripts in
/// the project's `package.json` files name, first, then the usual ones.
pub(crate) fn ports(root: &Path) -> Vec<u16> {
    let mut ports = DEFAULT_PORTS.to_vec();
    visit(root, 0, &mut ports);
    ports
}

fn visit(dir: &Path, depth: usize, ports: &mut Vec<u16>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        if name == "node_modules" || name == ".git" || name == ".canonic" {
            continue;
        }
        let file = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            visit(&file, depth + 1, ports);
            continue;
        }
        if name != "package.json" {
            continue;
        }
        // A malformed package is not Storybook detection's error.
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(package) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(scripts) = package.get("scripts").and_then(Value::as_object) else {
            continue;
        };
        for (name, script) in scripts {
            let script = match script {
                Value::String(script) => script.clone(),
                Value::Null | Value::Bool(false) => String::new(),
                other => other.to_string(),
            };
            if !STORYBOOK.is_match(&format!("{name} {script}")) {
                continue;
            }
            for found in PORT_FLAG.captures_iter(&script) {
                if let Ok(port) = found[1].parse::<u16>()
                    && !ports.contains(&port)
                {
                    ports.insert(0, port);
                }
            }
        }
    }
}

/// The address of a Storybook running for the project, from the ports its
/// scripts name and the usual ones, on `127.0.0.1` then `localhost`.
///
/// # Errors
/// 503 when none answers with a Storybook index.
pub(crate) async fn detect(root: &Path) -> Result<String> {
    for port in ports(root) {
        for host in ["127.0.0.1", "localhost"] {
            let address = format!("http://{host}:{port}");
            let storybook = Storybook {
                url: &address,
                owned: false,
                root: None,
            };
            if index(storybook).await.is_ok() {
                return Ok(address);
            }
        }
    }
    Err(refused(
        503,
        "couldn’t automatically find a running Storybook; start it or set its URL explicitly",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refused::status_of;
    use serde_json::json;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    /// A stand-in Storybook: answers every request with `status` and `body`,
    /// recording the paths it was asked for.
    async fn stub(
        status: u16,
        body: &'static str,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = format!(
            "http://127.0.0.1:{}",
            listener.local_addr().expect("address").port()
        );
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let record = record.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        let Ok(read) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..read]);
                    }
                    let line = String::from_utf8_lossy(&request)
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    record
                        .lock()
                        .expect("seen")
                        .push(line.split(' ').nth(1).unwrap_or_default().to_owned());
                    let response = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _written = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (address, seen)
    }

    const INDEX: &str = r#"{"v":5,"entries":{
        "components-button--default":{"id":"components-button--default","title":"Components/Button","name":"Default","type":"story","importPath":"./src/button/button.stories.tsx","componentPath":"./src/button/button.tsx"},
        "components-button--icon-only":{"id":"components-button--icon-only","title":"Components/Button","name":"Icon Only","type":"story","importPath":"./src/button/button.stories.tsx","componentPath":"./src/button/nowhere.tsx"},
        "components-button--docs":{"id":"components-button--docs","title":"Components/Button","name":"Docs","type":"docs"},
        "button--default":{"id":"button--default","title":"Button","name":"Default","type":"story"}}}"#;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wb-storybook-{name}-{}", std::process::id()));
        let _gone = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    #[test]
    fn a_story_id_names_its_scenario_after_the_double_dash() {
        assert_eq!(story_scenario("components-button--icon-only"), "icon-only");
        assert_eq!(story_scenario("plain"), "plain");
    }

    #[test]
    fn lists_a_titles_stories_with_the_code_that_exists() {
        let product = temp("titled");
        std::fs::create_dir_all(product.join("src/button")).expect("folders");
        std::fs::write(product.join("src/button/button.tsx"), "").expect("component");
        std::fs::write(product.join("src/button/button.stories.tsx"), "").expect("stories");
        let index: Value = serde_json::from_str(INDEX).expect("index");
        let storybook = Storybook {
            url: "http://127.0.0.1:6006",
            owned: false,
            root: Some(&product),
        };
        let stories = stories_titled(&index, "Components/Button", storybook).expect("stories");
        let named: Vec<_> = stories
            .iter()
            .map(|story| (story.id.as_str(), story.scenario.as_str()))
            .collect();
        assert_eq!(
            named,
            [
                ("components-button--default", "default"),
                ("components-button--icon-only", "icon-only")
            ]
        );
        assert_eq!(
            stories[0].code,
            [
                product.join("src/button/button.tsx"),
                product.join("src/button/button.stories.tsx")
            ]
        );
        assert_eq!(
            stories[1].code,
            [product.join("src/button/button.stories.tsx")]
        );
        let shown = serde_json::to_value(&stories[0]).expect("json");
        assert_eq!(shown["name"], "Default");
        assert_eq!(shown["importPath"], "./src/button/button.stories.tsx");

        let missing =
            stories_titled(&index, "Button/Primary", storybook).expect_err("no such title");
        assert_eq!(status_of(&missing, 500), 404);
        assert_eq!(
            missing.to_string(),
            "No stories titled “Button/Primary” at http://127.0.0.1:6006."
        );
        assert_eq!(
            stories_titled(&index, "Button", storybook)
                .expect("exact")
                .len(),
            1
        );
        let _gone = std::fs::remove_dir_all(product);
    }

    #[test]
    fn suggests_the_prefixed_title_when_only_the_prefix_differs() {
        let index = json!({ "v": 5, "entries": { "components-button--default": {
            "id": "components-button--default", "title": "Components/Button", "name": "Default", "type": "story" } } });
        let storybook = Storybook {
            url: "http://127.0.0.1:6006",
            owned: false,
            root: None,
        };
        let error = stories_titled(&index, "Button", storybook).expect_err("prefix differs");
        assert_eq!(
            error.to_string(),
            "No stories titled “Button” — did you mean “Components/Button”?"
        );
    }

    #[test]
    fn something_without_entries_is_not_a_storybook_index() {
        let error = entries(&Value::Null, "http://127.0.0.1:6006").expect_err("not an index");
        assert_eq!(status_of(&error, 500), 502);
        assert!(error.to_string().contains("isn’t a Storybook index"));
    }

    #[test]
    fn finds_ports_storybook_scripts_name_before_the_usual_ones() {
        let root = temp("ports");
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"storybook":"storybook dev --port 7123","dev":"vite --port 5173"}}"#,
        )
        .expect("package");
        std::fs::create_dir_all(root.join("apps/docs")).expect("folders");
        std::fs::write(
            root.join("apps/docs/package.json"),
            r#"{"scripts":{"sb":"storybook dev -p 7124"}}"#,
        )
        .expect("nested");
        std::fs::create_dir_all(root.join("node_modules/acme")).expect("dependency");
        std::fs::write(
            root.join("node_modules/acme/package.json"),
            r#"{"scripts":{"storybook":"storybook dev -p 7999"}}"#,
        )
        .expect("ignored");
        std::fs::create_dir_all(root.join("broken")).expect("folder");
        std::fs::write(root.join("broken/package.json"), "{").expect("malformed package");
        let found = ports(&root);
        assert_eq!(&found[..2], [7123, 7124]);
        assert!(!found.contains(&5173) && !found.contains(&7999));
        assert_eq!(&found[2..], DEFAULT_PORTS);
        let _gone = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn reads_a_running_storybooks_index() {
        let (url, seen) = stub(200, INDEX).await;
        let storybook = Storybook {
            url: &url,
            owned: false,
            root: None,
        };
        let index = index(storybook).await.expect("index");
        assert_eq!(entries(&index, &url).expect("entries").len(), 3);
        assert_eq!(seen.lock().expect("seen").as_slice(), ["/index.json"]);
    }

    #[tokio::test]
    async fn says_when_the_storybook_is_not_running_answers_wrongly_or_is_not_one() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let gone = format!(
            "http://127.0.0.1:{}",
            listener.local_addr().expect("address").port()
        );
        drop(listener);
        let error = index(Storybook {
            url: &gone,
            owned: false,
            root: None,
        })
        .await
        .expect_err("gone");
        assert_eq!(status_of(&error, 500), 503);
        assert_eq!(
            error.to_string(),
            format!("Storybook at {gone} isn’t answering — is it running?")
        );

        let (odd, _) = stub(200, "<html>").await;
        let error = index(Storybook {
            url: &odd,
            owned: false,
            root: None,
        })
        .await
        .expect_err("not one");
        assert_eq!(status_of(&error, 500), 502);
        assert!(error.to_string().contains("isn’t a Storybook index"));

        let (failing, _) = stub(500, "{}").await;
        let error = index(Storybook {
            url: &failing,
            owned: false,
            root: None,
        })
        .await
        .expect_err("500");
        assert_eq!(status_of(&error, 500), 502);
        assert_eq!(
            error.to_string(),
            format!("Storybook at {failing} answered 500 for /index.json.")
        );
    }

    #[tokio::test]
    async fn a_storybook_the_server_runs_explains_its_own_failures() {
        let root = Path::new("/acme/product");
        let (failing, _) = stub(502, "Storybook isn’t installed in /acme/product.\n").await;
        let error = index(Storybook {
            url: &failing,
            owned: true,
            root: Some(root),
        })
        .await
        .expect_err("can't run");
        assert_eq!(status_of(&error, 500), 502);
        assert_eq!(
            error.to_string(),
            "Workbench couldn’t run Storybook in /acme/product: Storybook isn’t installed in /acme/product."
        );

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let gone = format!(
            "http://127.0.0.1:{}",
            listener.local_addr().expect("address").port()
        );
        drop(listener);
        let error = index(Storybook {
            url: &gone,
            owned: true,
            root: Some(root),
        })
        .await
        .expect_err("gone");
        assert_eq!(status_of(&error, 500), 503);
        assert!(
            error
                .to_string()
                .starts_with("Workbench’s Storybook in /acme/product isn’t answering: ")
        );
    }

    #[tokio::test]
    async fn detects_a_running_storybook_from_the_project_script() {
        let (url, _) = stub(200, INDEX).await;
        let port = url.rsplit(':').next().expect("port");
        let root = temp("detect");
        std::fs::write(
            root.join("package.json"),
            format!(r#"{{"scripts":{{"storybook":"storybook dev -p {port}"}}}}"#),
        )
        .expect("package");
        assert_eq!(detect(&root).await.expect("detected"), url);
        let _gone = std::fs::remove_dir_all(root);
    }
}
