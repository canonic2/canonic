//! HTML previews and Astro renders as modules whose default export is the
//! page's HTML. Pure transformation, used by the compiler's HTML loaders:
//!
//! - linked local stylesheets and inline `<style>` blocks become CSS imports,
//!   so they enter the build, resolve their `url()` assets and update in place;
//! - inline scripts and local `<script src>` become modules of the build, so
//!   they can import local modules, packages and CSS, and their top-level
//!   declarations stay private. Each is replaced by a marker the HTML adapter
//!   runs on every render, in document order, with its imports evaluated once;
//! - local images and media become asset URLs;
//! - `<title>`, `<meta>` and `<base>` are dropped.
//!
//! JSON data blocks and external scripts stay in the HTML. `wbfile:<absolute
//! path>` names a file the server generated, such as an Astro resource. The
//! parts are served through resource queries on the same file.
use super::entries::{digest, quote};
use crate::paths::resolve;
use regex::{Captures, Regex};
use std::path::Path;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("valid HTML pattern")
}

static EXTERNAL: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^(?:[a-z][a-z0-9+.-]*:|//|#)"));
static INLINE_STYLE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?is)<style\b[^>]*>(.*?)</style>"));
static SCRIPT: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?is)<script\b([^>]*)>(.*?)</script>"));
static MODULE: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(?i)\btype\s*=\s*["']module["']"#));
static SRC: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)\bsrc\s*=\s*(?:"([^"']+)"|'([^"']+)')"#));
static DATA: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)\btype\s*=\s*["']application/(?:ld\+)?json["']"#));
/// An Astro client script: a module script that only imports a generated resource.
static ASTRO_SCRIPT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"^\s*import\s*["'](wbfile:[^"']+)["'];?\s*$"#));
static LINK: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)<link\b[^>]*>"));
static STYLESHEET: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)\brel\s*=\s*["']?stylesheet"#));
static HREF: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)\bhref\s*=\s*(?:"([^"']+)"|'([^"']+)')"#));
static SRC_OR_TYPE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)\s(?:src|type)\s*=\s*(?:"[^"']*"|'[^"']*')"#));
static TAG_OR_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?is)<!--.*?-->|<[a-z](?:[^"'<>]|"[^"]*"|'[^']*')*>"#));
static MEDIA: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)\b(src|poster)\s*=\s*(?:"([^"']+)"|'([^"']+)')"#));
static SRCSET: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)\bsrcset\s*=\s*(?:"([^"']+)"|'([^"']+)')"#));
static DROPPED: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?is)<title>.*?</title>|<meta\b[^>]*>|<base\b[^>]*>"));
static URL_MARK: LazyLock<Regex> = LazyLock::new(|| pattern(r"__WB_URL_(\d+)__"));
static LEADING_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"^\s*import\s(?:[^;'"]*?\sfrom\s*)?["'][^"']+["']\s*;?"#));

fn external(value: &str) -> bool {
    EXTERNAL.is_match(value) && !value.starts_with("wbfile:")
}

/// The first of two alternative capture groups that matched.
fn either<'a>(captures: &Captures<'a>, first: usize) -> Option<&'a str> {
    captures
        .get(first)
        .or_else(|| captures.get(first + 1))
        .map(|found| found.as_str())
}

fn src_of(attributes: &str) -> Option<String> {
    SRC.captures(attributes)
        .and_then(|captures| either(&captures, 1).map(str::to_owned))
}

/// The quote character a two-alternative attribute match used.
fn quote_of(captures: &Captures<'_>, first: usize) -> char {
    if captures.get(first).is_some() {
        '"'
    } else {
        '\''
    }
}

/// Inline scripts the build compiles, in document order, as both loaders count them.
fn inline_scripts(source: &str) -> Vec<String> {
    SCRIPT
        .captures_iter(source)
        .filter(|captures| {
            let attributes = &captures[1];
            src_of(attributes).is_none()
                && !DATA.is_match(attributes)
                && !(MODULE.is_match(attributes) && ASTRO_SCRIPT.is_match(&captures[2]))
        })
        .map(|captures| captures[2].to_owned())
        .collect()
}

fn inline_styles(source: &str) -> Vec<String> {
    INLINE_STYLE
        .captures_iter(source)
        .map(|captures| captures[1].to_owned())
        .collect()
}

/// A script as a module that runs again on each render: its imports stay at
/// the top, evaluated once, and the rest becomes the default export.
pub(crate) fn runnable_script(source: &str) -> String {
    let mut imports = vec![];
    let mut body = source;
    while let Some(found) = LEADING_IMPORT.find(body) {
        imports.push(found.as_str().trim().to_owned());
        body = &body[found.end()..];
    }
    format!(
        "{}\nexport default async function workbenchScript() {{\n{body}\n}}\n",
        imports.join("\n")
    )
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The module for an HTML page or rendered Astro HTML at `resource`.
#[expect(
    clippy::too_many_lines,
    reason = "one pass per part of the page, in the loader's order"
)]
pub(crate) fn transform_html(source: &str, resource: &str, root: &Path) -> String {
    let directory = Path::new(resource).parent().unwrap_or(root);
    let request = |value: &str| -> String {
        let clean = value.split(['?', '#']).next().unwrap_or_default();
        if let Some(file) = clean.strip_prefix("wbfile:") {
            return file.to_owned();
        }
        if let Some(rooted) = clean.strip_prefix('/') {
            return display(&resolve(root, rooted));
        }
        display(&resolve(directory, clean))
    };
    let id = digest(resource);
    let mut imports: Vec<String> = vec![];
    let mut scripts: Vec<String> = vec![];
    let mut urls: Vec<String> = vec![];
    let html = LINK.replace_all(source, |captures: &Captures<'_>| {
        let tag = &captures[0];
        if !STYLESHEET.is_match(tag) {
            return tag.to_owned();
        }
        match HREF.captures(tag).and_then(|href| either(&href, 1)) {
            Some(href) if !external(href) => {
                imports.push(format!("import {};", quote(&request(href))));
                String::new()
            }
            _ => tag.to_owned(),
        }
    });
    let mut styles = 0;
    let html = INLINE_STYLE.replace_all(&html, |_: &Captures<'_>| {
        imports.push(format!(
            "import {};",
            quote(&format!("{resource}?workbench-style={styles}"))
        ));
        styles += 1;
        String::new()
    });
    let mut inline = 0;
    let html = SCRIPT.replace_all(&html, |captures: &Captures<'_>| {
        let (whole, attributes, body) = (&captures[0], &captures[1], &captures[2]);
        // Astro hoists its client scripts into module scripts that import a resource.
        if MODULE.is_match(attributes)
            && let Some(astro) = ASTRO_SCRIPT.captures(body)
        {
            imports.push(format!("import {};", quote(&request(&astro[1]))));
            return String::new();
        }
        if DATA.is_match(attributes) {
            return whole.to_owned();
        }
        let target = match src_of(attributes) {
            Some(src) if !external(&src) => format!("{}?workbench-run", request(&src)),
            Some(_) => return whole.to_owned(),
            None => {
                let target = format!("{resource}?workbench-script={inline}");
                inline += 1;
                target
            }
        };
        scripts.push(format!("() => import({})", quote(&target)));
        // Other attributes stay on the marker, for scripts that read them.
        let kept = SRC_OR_TYPE.replace_all(attributes, "");
        format!(
            "<script{kept} data-workbench-script=\"{id}:{}\"></script>",
            scripts.len() - 1
        )
    });
    let mut placeholder = |value: &str| -> String {
        urls.push(request(value));
        format!("__WB_URL_{}__", urls.len() - 1)
    };
    // Attribute-like text in Markdown code samples is not an asset reference.
    let html = TAG_OR_COMMENT.replace_all(&html, |captures: &Captures<'_>| {
        let tag = &captures[0];
        if tag.starts_with("<!--") {
            return tag.to_owned();
        }
        let tag = MEDIA.replace_all(tag, |media: &Captures<'_>| {
            let value = either(media, 2).unwrap_or_default();
            if external(value) {
                return media[0].to_owned();
            }
            let mark = quote_of(media, 2);
            format!("{}={mark}{}{mark}", &media[1], placeholder(value))
        });
        SRCSET
            .replace_all(&tag, |set: &Captures<'_>| {
                let mark = quote_of(set, 1);
                let items = either(set, 1)
                    .unwrap_or_default()
                    .split(',')
                    .map(|item| {
                        let mut parts = item.split_whitespace();
                        let url = parts.next().unwrap_or_default();
                        let size: Vec<&str> = parts.collect();
                        let url = if external(url) {
                            url.to_owned()
                        } else {
                            placeholder(url)
                        };
                        if size.is_empty() {
                            url
                        } else {
                            format!("{url} {}", size.join(" "))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("srcset={mark}{items}{mark}")
            })
            .into_owned()
    });
    let html = DROPPED.replace_all(&html, "");
    let mut pieces = vec![];
    let mut last = 0;
    for captures in URL_MARK.captures_iter(&html) {
        let whole = captures.get(0).expect("whole match");
        pieces.push(quote(&html[last..whole.start()]));
        pieces.push(format!("url{}", &captures[1]));
        last = whole.end();
    }
    pieces.push(quote(&html[last..]));
    let urls = urls
        .iter()
        .enumerate()
        .map(|(index, url)| format!("import url{index} from {};", quote(url)))
        .collect::<Vec<_>>()
        .join("\n");
    let registered = if scripts.is_empty() {
        String::new()
    } else {
        format!(
            "(window.__wbHtmlScripts || (window.__wbHtmlScripts = {{}}))[{}] = [{}];\n",
            quote(&id),
            scripts.join(", ")
        )
    };
    format!(
        "{}\n{urls}\n{registered}export default {};\n",
        imports.join("\n"),
        pieces.join(" + ")
    )
}

/// The inline style or script a query names, or a script file made runnable.
pub(crate) fn html_part(source: &str, query: &str) -> Result<String, String> {
    let params: Vec<(&str, &str)> = query
        .trim_start_matches('?')
        .split('&')
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
        .collect();
    let value = |key: &str| {
        params
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    };
    let index = |value: &str| value.parse::<usize>().unwrap_or(usize::MAX);
    if let Some(at) = value("workbench-style") {
        return Ok(inline_styles(source)
            .into_iter()
            .nth(index(at))
            .unwrap_or_default());
    }
    if let Some(at) = value("workbench-script") {
        return Ok(runnable_script(
            &inline_scripts(source)
                .into_iter()
                .nth(index(at))
                .unwrap_or_default(),
        ));
    }
    if value("workbench-run").is_some() {
        return Ok(runnable_script(source));
    }
    Err(format!("Unknown HTML part: {query}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/acme";
    const PAGE: &str = "/acme/pages/landing.html";

    fn transform(source: &str) -> String {
        transform_html(source, PAGE, Path::new(ROOT))
    }

    #[test]
    fn local_styles_and_scripts_enter_the_build_and_external_ones_stay() {
        let html = concat!(
            r#"<!doctype html><html><head><title>Acme</title><meta charset="utf-8"><link rel="stylesheet" href="./landing.css">"#,
            r#"<link rel="stylesheet" href="https://example.com/theme.css"><style>.hero { background: url(./hero.svg); }</style></head>"#,
            r#"<body class="acme"><img src="./hero.svg" srcset="./hero.svg 1x, https://example.com/hero@2x.png 2x"><a href="/account">Account</a>"#,
            r#"<script data-acme="kept">document.body.dataset.classic = "yes";</script><script type="application/json">{"plans": []}</script><script type="module">import { label } from "./label.ts"; document.body.dataset.label = label;</script>"#,
            r#"<script type="module" src="./widget.ts"></script><script src="https://example.com/analytics.js"></script></body></html>"#
        );
        let module = transform(html);
        assert!(module.contains(r#"import "/acme/pages/landing.css";"#));
        assert!(module.contains(&quote(&format!("{PAGE}?workbench-style=0"))));
        assert!(module.contains(&quote(&format!("{PAGE}?workbench-script=0"))));
        assert!(module.contains(&quote(&format!("{PAGE}?workbench-script=1"))));
        assert!(
            module.contains(r#"<script data-acme=\"kept\" data-workbench-script="#),
            "other attributes stay on the marker"
        );
        assert!(module.contains(r#"{\"plans\": []}"#), "JSON data stays");
        assert!(module.contains(r#""/acme/pages/widget.ts?workbench-run""#));
        assert!(module.contains("https://example.com/theme.css"));
        assert!(module.contains("https://example.com/analytics.js"));
        assert!(module.contains(r#"import url0 from "/acme/pages/hero.svg";"#));
        assert!(module.contains("https://example.com/hero@2x.png 2x"));
        assert!(
            module.contains(r#"href=\"/account\""#),
            "links are navigation"
        );
        assert!(
            !module.contains("<title>") && !module.contains("<meta") && !module.contains("<style")
        );
        assert_eq!(module.matches("data-workbench-script=").count(), 3);
    }

    #[test]
    fn inline_parts_are_served_by_query_and_scripts_run_on_each_render() {
        let html = concat!(
            "<style>a { color: red; }</style><style>b { color: blue; }</style>",
            "<script type=\"module\">import \"./one.css\";\nimport { two } from \"./two.ts\"; console.log(two);</script>",
            "<script type=\"module\">import \"wbfile:/acme/.astro/hoisted.ts\";</script>"
        );
        assert_eq!(
            html_part(html, "?workbench-style=1").unwrap(),
            "b { color: blue; }"
        );
        let script = html_part(html, "?workbench-script=0").unwrap();
        assert!(script.starts_with(
            "import \"./one.css\";\nimport { two } from \"./two.ts\";\nexport default async function workbenchScript() {\n console.log(two);\n}"
        ));
        assert!(
            html_part(html, "?other")
                .unwrap_err()
                .contains("Unknown HTML part")
        );
        // Astro's hoisted client script is imported once, not run per render.
        let module = transform(html);
        assert!(module.contains("import \"/acme/.astro/hoisted.ts\";"));
        assert_eq!(module.matches("data-workbench-script=").count(), 1);
    }

    #[test]
    fn escaped_code_examples_and_comments_are_not_assets() {
        let examples = concat!(
            r#"<pre><code>&lt;img src="missing.svg" srcset="missing@2x.svg 2x"&gt;</code></pre>"#,
            r#"<p>Use src="undefined" and poster="..." in your example.</p>"#,
            r#"<!-- <img src="comment-only.svg"> -->"#
        );
        let module = transform(&format!(r#"{examples}<img alt="a > b" src="real.svg">"#));
        let quoted = quote(examples);
        assert!(module.contains(&quoted[1..quoted.len() - 1]));
        assert!(module.contains(r#"import url0 from "/acme/pages/real.svg";"#));
        assert!(!module.contains("import url1"));
        for name in ["missing", "undefined", "comment-only"] {
            assert!(!module.contains(&format!("from \"/acme/pages/{name}")));
        }
    }

    #[test]
    fn only_leading_imports_stay_outside_the_render_function() {
        assert_eq!(
            runnable_script(
                r#"import "./a.css"; import { b } from "./b.ts"; run(b); import("./lazy.ts");"#
            ),
            "import \"./a.css\";\nimport { b } from \"./b.ts\";\nexport default async function workbenchScript() {\n run(b); import(\"./lazy.ts\");\n}\n"
        );
    }

    #[test]
    fn replacement_like_text_in_scripts_is_kept_as_written() {
        let module = transform(r#"<script data-pattern="$& $1">window.x = "$&";</script>"#);
        assert!(module.contains(r#"data-pattern=\"$& $1\""#));
    }
}
