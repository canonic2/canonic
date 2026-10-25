//! Files the server serves as they are: Workbench's browser assets,
//! unchanged; the space's manifest files; and the project's files, with the
//! compatibility script in each of its pages. Markdown pages and preview
//! pages come first from the docs service and the compiler; the fallback
//! serves any file they do not claim, and says when one is missing.
use axum::{
    body::Body,
    http::{HeaderValue, Method, StatusCode, header},
    response::Response,
};
use std::path::{Component, Path, PathBuf};

const PREFIX: &str = "/_workbench/";
const COMPAT_TAG: &str = "<script src=\"/_workbench/preview-compat.js\"></script>";
const LUCIDE_URL: &str = "/node_modules/lucide/dist/umd/lucide.min.js";
/// Workbench documents that moved into the UI's `app/` folder, by the address
/// they keep: callers such as the capture helper still ask for the old one.
const APP_DOCUMENTS: &[(&str, &str)] = &[
    ("/_workbench/capture.html", "ui/src/app/capture/capture.html"),
    (
        "/_workbench/preview-host.html",
        "ui/src/app/preview-host/preview-host.html",
    ),
];

/// Where the space's files and Workbench's own come from.
pub(crate) struct Files {
    root: PathBuf,
    product: PathBuf,
    /// The folder holding the space's `workbench.yaml`.
    space_dir: PathBuf,
    /// Meta tags the canvas reads when the space is not the root's first.
    canvas_head: String,
    compat: String,
}

fn content_type(file: &Path) -> &'static str {
    let extension = file
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn respond(status: StatusCode, body: Vec<u8>, kind: &'static str, head: bool) -> Response {
    let length = body.len();
    let mut response = Response::new(if head {
        Body::empty()
    } else {
        Body::from(body)
    });
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    // The whole point is seeing edits, so nothing is cached.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `%XX` escapes decoded, as `decodeURIComponent` reads a path; `None` when invalid.
fn decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `relative` inside `folder`, or `None` when it reaches outside however it
/// is spelled.
fn inside(folder: &Path, relative: &str) -> Option<PathBuf> {
    let mut file = folder.to_path_buf();
    for part in Path::new(relative.trim_start_matches('/')).components() {
        match part {
            Component::Normal(name) => file.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !file.pop() || !file.starts_with(folder) {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    file.starts_with(folder).then_some(file)
}

/// The page with the compatibility script first in its head.
pub(crate) fn with_compat(html: &str) -> String {
    static HEAD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)<head(\s[^>]*)?>").expect("valid pattern")
    });
    static HTML: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)<html(\s[^>]*)?>").expect("valid pattern")
    });
    match HEAD.find(html).or_else(|| HTML.find(html)) {
        Some(found) => format!(
            "{}{COMPAT_TAG}{}",
            &html[..found.end()],
            &html[found.end()..]
        ),
        None => format!("{COMPAT_TAG}{html}"),
    }
}

impl Files {
    pub(crate) fn with_trace(mut self, enabled: bool) -> Self {
        if enabled {
            self.canvas_head
                .push_str("<meta name=\"workbench-trace\" content=\"true\">\n");
        }
        self
    }
    pub(crate) fn new(
        root: &Path,
        product: &Path,
        space_dir: Option<&Path>,
        space_key: Option<&str>,
    ) -> Self {
        let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        let space_dir = space_dir.map_or_else(|| root.clone(), Path::to_path_buf);
        // A space whose config is not at its root, or is not the whole file,
        // tells the canvas, which then reads the file from the manifest route.
        let mut canvas_head = String::new();
        if space_key.is_some() || space_dir != root {
            canvas_head
                .push_str("<meta name=\"canonic-config\" content=\"/_workbench/manifest/\" />\n");
            if let Some(key) = space_key {
                canvas_head.push_str("<meta name=\"canonic-space\" content=\"");
                canvas_head.push_str(key);
                canvas_head.push_str("\" />\n");
            }
        }
        let legacy = product.join("ui/legacy");
        let compat = ["keys.js", "actions.js", "scenarios.js"]
            .iter()
            .map(|name| std::fs::read_to_string(legacy.join(name)).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n;\n");
        Self {
            root,
            product: product.to_path_buf(),
            space_dir,
            canvas_head,
            compat,
        }
    }

    /// A file and how to serve it, or `None` when a route may claim it.
    fn locate(&self, path: &str) -> Option<(PathBuf, Option<&'static str>)> {
        if path == "/_workbench/preview-runtime.js" {
            return Some((self.product.join("ui/preview/browser.js"), None));
        }
        if path == "/_workbench/preview-requests.js" {
            return Some((self.product.join("ui/preview/requests.js"), None));
        }
        if path == LUCIDE_URL {
            return Some((
                self.product
                    .join("node_modules/lucide/dist/umd/lucide.min.js"),
                None,
            ));
        }
        if path == "/_workbench/manifest.js" || path == "/_workbench/yaml.js" {
            return Some((
                self.product.join("contracts").join(&path[PREFIX.len()..]),
                None,
            ));
        }
        if let Some((_, file)) = APP_DOCUMENTS.iter().find(|(served, _)| *served == path) {
            let file = self.product.join(file);
            return file.is_file().then_some((file, Some("ui")));
        }
        let relative = decode(path)?;
        if let Some(rest) = relative.strip_prefix(PREFIX) {
            // Workbench's own files, out of the product's legacy UI folder;
            // anything not there may be one of the space's routes.
            let rest = if rest.is_empty() { "index.html" } else { rest };
            let file = inside(&self.product.join("ui/legacy"), rest)?;
            return file.is_file().then_some((file, Some("ui")));
        }
        // Markdown pages are the docs service's, and preview pages the compiler's.
        if Path::new(&relative)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            || relative.ends_with(".workbench.ts")
            || relative.ends_with(".workbench.tsx")
        {
            return None;
        }
        let relative = if relative.ends_with('/') {
            format!("{relative}index.html")
        } else {
            relative
        };
        let file = inside(&self.root, &relative)?;
        file.is_file().then_some((file, Some("project")))
    }

    /// Answers the request, or `None` when a route may claim it.
    pub(crate) fn answer(&self, method: &Method, path: &str) -> Option<Response> {
        let head = *method == Method::HEAD;
        if *method != Method::GET && !head {
            return None;
        }
        if path == "/_workbench/preview-compat.js" {
            return Some(respond(
                StatusCode::OK,
                self.compat.clone().into_bytes(),
                "text/javascript; charset=utf-8",
                head,
            ));
        }
        if path.starts_with("/_workbench/ui/src/") || path.starts_with("/_workbench/contracts/src/")
        {
            return Some(self.browser_module(path, head));
        }
        if let Some(name) = path.strip_prefix("/_workbench/manifest/") {
            if name != "workbench.yaml" && name != "workbench.local.yaml" {
                return Some(respond(
                    StatusCode::NOT_FOUND,
                    b"Not a Workbench config file.".to_vec(),
                    "text/plain; charset=utf-8",
                    head,
                ));
            }
            let file = self.space_dir.join(name);
            return Some(match std::fs::read(&file) {
                Ok(body) => respond(StatusCode::OK, body, content_type(&file), head),
                Err(_) => respond(
                    StatusCode::NOT_FOUND,
                    format!("Not found: /{name}").into_bytes(),
                    "text/plain; charset=utf-8",
                    head,
                ),
            });
        }
        let (file, kind) = self.locate(path)?;
        let mut body = std::fs::read(&file).ok()?;
        let html = file
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("html"));
        if html && kind == Some("project") {
            body = with_compat(&String::from_utf8_lossy(&body)).into_bytes();
        } else if html && kind == Some("ui") && !self.canvas_head.is_empty() {
            body = String::from_utf8_lossy(&body)
                .replacen("<head>", &format!("<head>\n{}", self.canvas_head), 1)
                .into_bytes();
        }
        Some(respond(StatusCode::OK, body, content_type(&file), head))
    }

    /// Any other file: a project file served as it is, Markdown included,
    /// or Workbench’s own; 404 when it isn’t there and 403 outside its folder.
    pub(crate) fn fallback(&self, method: &Method, path: &str) -> Response {
        let head = *method == Method::HEAD;
        let plain = "text/plain; charset=utf-8";
        if *method != Method::GET && !head {
            return respond(
                StatusCode::METHOD_NOT_ALLOWED,
                b"GET or HEAD only.".to_vec(),
                plain,
                false,
            );
        }
        let Some(relative) = decode(path) else {
            return respond(
                StatusCode::BAD_REQUEST,
                "That address isn’t valid.".as_bytes().to_vec(),
                plain,
                head,
            );
        };
        let (folder, relative, project) = match relative.strip_prefix(PREFIX) {
            Some(rest) => (self.product.join("ui/legacy"), format!("/{rest}"), false),
            None => (self.root.clone(), relative, true),
        };
        let relative = if relative.ends_with('/') {
            format!("{relative}index.html")
        } else {
            relative
        };
        let Some(file) = inside(&folder, &relative) else {
            return respond(
                StatusCode::FORBIDDEN,
                b"Outside the folder.".to_vec(),
                plain,
                head,
            );
        };
        match std::fs::read(&file) {
            Ok(mut body) => {
                let html = file
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("html"));
                if html && project {
                    body = with_compat(&String::from_utf8_lossy(&body)).into_bytes();
                }
                respond(StatusCode::OK, body, content_type(&file), head)
            }
            Err(error) => {
                let status = if error.kind() == std::io::ErrorKind::NotFound || file.is_dir() {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                respond(
                    status,
                    format!("Not found: {relative}").into_bytes(),
                    plain,
                    head,
                )
            }
        }
    }

    /// Authored browser JavaScript/CSS and the Docs content host, never tests.
    fn browser_module(&self, path: &str, head: bool) -> Response {
        let missing = || {
            respond(
                StatusCode::NOT_FOUND,
                format!("Not a Workbench browser module: {path}").into_bytes(),
                "text/plain; charset=utf-8",
                head,
            )
        };
        let Some(relative) = decode(&path[PREFIX.len()..]) else {
            return missing();
        };
        if relative.contains('\\') || relative.split('/').any(|part| part == "." || part == "..") {
            return missing();
        }
        let extension = Path::new(&relative)
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kind = match extension.as_str() {
            "js" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "html" if relative == "ui/src/app/docs/docs-host.html" => {
                "text/html; charset=utf-8"
            }
            _ => return missing(),
        };
        if relative.ends_with(".test.js") {
            return missing();
        }
        match inside(&self.product, &relative).map(std::fs::read) {
            Some(Ok(body)) => respond(StatusCode::OK, body, kind, head),
            _ => missing(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compatibility_script_goes_first_in_the_head_or_the_document() {
        let page =
            with_compat("<!doctype html><html><head><title>x</title></head><body></body></html>");
        assert!(page.contains(&format!("<head>{COMPAT_TAG}<title>")));
        assert!(
            with_compat("<html><head lang=\"en\"><title>x</title></head></html>")
                .contains(&format!("<head lang=\"en\">{COMPAT_TAG}"))
        );
        // A <header> is no head: the script belongs to <html>, and the header stays.
        let header = with_compat("<html><body><header class=\"a\">hi</header></body></html>");
        assert!(
            header.contains(&format!("<html>{COMPAT_TAG}"))
                && header.contains("<header class=\"a\">hi</header>")
        );
        assert_eq!(
            with_compat("<div>just a fragment</div>"),
            format!("{COMPAT_TAG}<div>just a fragment</div>")
        );
        let untouched = "<html><head></head><body><p>unchanged</p></body></html>";
        assert_eq!(with_compat(untouched).replace(COMPAT_TAG, ""), untouched);
    }

    fn product() -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("acme-files-{}", std::process::id()));
        let product = dir.join("workbench");
        let root = dir.join("acme");
        for (file, body) in [
            (
                "workbench/ui/legacy/index.html",
                "<html><head><title>Workbench</title></head></html>",
            ),
            ("workbench/ui/legacy/keys.js", "keys();"),
            ("workbench/ui/legacy/actions.js", "actions();"),
            ("workbench/ui/legacy/scenarios.js", "scenarios();"),
            ("workbench/ui/src/shell.js", "export {};"),
            ("workbench/ui/src/shell.test.js", "test();"),
            ("workbench/ui/src/shell.ts", "export {};"),
            (
                "workbench/ui/src/app/docs/docs-host.html",
                "<!doctype html><title>Docs</title>",
            ),
            (
                "workbench/ui/src/app/capture/capture.html",
                "<html><head><title>Capture</title></head></html>",
            ),
            (
                "workbench/ui/src/app/preview-host/preview-host.html",
                "<html><head><title>Preview</title></head></html>",
            ),
            (
                "acme/pages/landing.html",
                "<!doctype html><html><head><title>Acme</title></head><body></body></html>",
            ),
            ("acme/pages/notes.md", "# Notes"),
            ("acme/assets/logo.svg", "<svg/>"),
            ("acme/assets/icons.mjs", "export const check = [];"),
            ("acme/workbench.yaml", "name: Acme\n"),
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        (root, product)
    }

    async fn text(response: Response) -> (StatusCode, String) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn project_pages_carry_the_compatibility_script_and_ui_files_stay_unchanged() {
        let (root, product) = product();
        let files = Files::new(&root, &product, None, None);
        let get = |path: &str| files.answer(&Method::GET, path);
        let module = get("/assets/icons.mjs").unwrap();
        assert_eq!(
            module.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(text(module).await.1, "export const check = [];");
        let (status, page) = text(get("/pages/landing.html").unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(page.starts_with(&format!(
            "<!doctype html><html><head>{COMPAT_TAG}<title>Acme"
        )));
        assert_eq!(
            text(get("/_workbench/preview-compat.js").unwrap()).await.1,
            "keys();\n;\nactions();\n;\nscenarios();"
        );
        assert_eq!(
            text(get("/_workbench/").unwrap()).await.1,
            "<html><head><title>Workbench</title></head></html>"
        );
        assert_eq!(
            text(get("/_workbench/capture.html").unwrap()).await.1,
            "<html><head><title>Capture</title></head></html>",
            "a document moved into the app folder keeps its address"
        );
        assert_eq!(
            text(get("/_workbench/preview-host.html").unwrap()).await.1,
            "<html><head><title>Preview</title></head></html>"
        );
        assert_eq!(
            text(get("/_workbench/ui/src/shell.js").unwrap()).await.0,
            StatusCode::OK
        );
        let docs = get("/_workbench/ui/src/app/docs/docs-host.html").unwrap();
        assert_eq!(
            docs.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(text(docs).await.1, "<!doctype html><title>Docs</title>");
        assert_eq!(
            text(get("/_workbench/ui/src/shell.test.js").unwrap())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            text(get("/_workbench/ui/src/shell.ts").unwrap()).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            text(get("/_workbench/ui/src/../legacy/keys.js").unwrap())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert!(
            get("/pages/notes.md").is_none(),
            "Markdown pages are the docs service's"
        );
        assert!(
            get("/_workbench/config").is_none(),
            "API routes are the space's"
        );
        assert!(get("/missing.html").is_none());
        assert!(get("/../outside.txt").is_none());
        assert!(get("/%2e%2e/outside.txt").is_none());
        assert!(files.answer(&Method::POST, "/pages/landing.html").is_none());
        let head = files.answer(&Method::HEAD, "/assets/logo.svg").unwrap();
        assert_eq!(head.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(head.headers()[header::CONTENT_LENGTH], "6");
        assert_eq!(
            text(get("/_workbench/manifest/other.yaml").unwrap())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        let keyed = Files::new(&root, &product, Some(&root), Some("web"));
        let (_, canvas) = text(keyed.answer(&Method::GET, "/_workbench/").unwrap()).await;
        assert!(canvas.contains("<head>\n<meta name=\"canonic-config\" content=\"/_workbench/manifest/\" />\n<meta name=\"canonic-space\" content=\"web\" />"));
        assert_eq!(
            text(
                keyed
                    .answer(&Method::GET, "/_workbench/manifest/workbench.yaml")
                    .unwrap()
            )
            .await
            .1,
            "name: Acme\n"
        );
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
}
