//! The static references a source file makes — imports, requires, URLs and
//! stylesheet imports — read lexically, without the filesystem. Pure.
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// The package a bare specifier names, or `None` for a relative, absolute,
/// subpath-import or scheme specifier.
pub(crate) fn package_name(specifier: &str) -> Option<String> {
    static SCHEME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^[a-z][a-z0-9+.-]*:").expect("scheme pattern"));
    let first = specifier.chars().next()?;
    if matches!(first, '.' | '/' | '#') {
        return None;
    }
    if SCHEME.is_match(specifier) {
        return None;
    }
    let parts: Vec<&str> = specifier.split('/').collect();
    Some(if first == '@' {
        parts.iter().take(2).copied().collect::<Vec<_>>().join("/")
    } else {
        parts[0].to_owned()
    })
}

/// The source with comments blanked, string contents kept.
fn without_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut quote: Option<char> = None;
    let mut at = 0;
    while at < chars.len() {
        let character = chars[at];
        let next = chars.get(at + 1).copied();
        if let Some(open) = quote {
            out.push(character);
            if character == '\\' {
                if let Some(escaped) = next {
                    out.push(escaped);
                    at += 1;
                }
            } else if character == open {
                quote = None;
            }
            at += 1;
            continue;
        }
        if matches!(character, '"' | '\'' | '`') {
            quote = Some(character);
            out.push(character);
        } else if character == '/' && next == Some('/') {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
            out.push('\n');
        } else if character == '/' && next == Some('*') {
            at += 2;
            while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                at += 1;
            }
            at += 1;
            out.push(' ');
        } else {
            out.push(character);
        }
        at += 1;
    }
    out
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("reference pattern")
}

static IMPORTS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        pattern(r#"(?:import|export)\s+(?:type\s+)?(?:[^"'`;]*?\s+from\s+)?["']([^"']+)["']"#),
        pattern(r#"(?:import|require)\s*\(\s*["']([^"']+)["']\s*\)"#),
        pattern(r#"new\s+URL\s*\(\s*["']([^"']+)["']\s*,\s*import\.meta\.url\s*\)"#),
    ]
});
static STYLES: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        pattern(r#"@(?:import|use|forward)\s+(?:url\(\s*)?["']([^"']+)["']"#),
        pattern(r#"url\(\s*["']?([^"')]+)["']?\s*\)"#),
    ]
});
static MARKUP: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?i)(?:src|href)\s*=\s*["']([^"'#]+)["']"#));

const SCRIPTS: [&str; 6] = [".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"];
const STYLESHEETS: [&str; 4] = [".css", ".scss", ".sass", ".less"];

pub(super) fn is_script(extension: &str) -> bool {
    SCRIPTS.contains(&extension)
}

pub(super) fn is_stylesheet(extension: &str) -> bool {
    STYLESHEETS.contains(&extension)
}

/// Every reference in a file of extension `extension`, in first-seen order.
pub(crate) fn parse_references(extension: &str, body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut take = |pattern: &Regex, text: &str| {
        for found in pattern.captures_iter(text) {
            let specifier = &found[1];
            if !out.iter().any(|seen| seen == specifier) {
                out.push(specifier.to_owned());
            }
        }
    };
    if is_script(extension) {
        let stripped = without_comments(body);
        for pattern in IMPORTS.iter() {
            take(pattern, &stripped);
        }
    }
    if is_stylesheet(extension) {
        for pattern in STYLES.iter() {
            take(pattern, body);
        }
    }
    if extension == ".html" || extension == ".htm" {
        take(&MARKUP, body);
    }
    out
}

/// The specifiers a stylesheet imports, which may name packages.
pub(super) fn stylesheet_imports(body: &str) -> BTreeSet<String> {
    STYLES[0]
        .captures_iter(body)
        .map(|found| found[1].to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_imports_assets_and_package_names() {
        assert_eq!(
            parse_references(
                ".ts",
                "import x from './x'; export { y } from '@scope/pkg/y'; require('plain/z')"
            ),
            ["./x", "@scope/pkg/y", "plain/z"]
        );
        assert_eq!(package_name("@scope/pkg/x").as_deref(), Some("@scope/pkg"));
        assert_eq!(package_name("plain/x").as_deref(), Some("plain"));
        assert_eq!(package_name("./local"), None);
        assert_eq!(package_name("virtual:svg-icons-register"), None);
        assert_eq!(package_name("#internal"), None);
    }

    #[test]
    fn comments_hide_imports_but_strings_keep_their_slashes() {
        let body = "import React from 'react'; import '../shared/tokens.css'; // import '../billing/charge';\n\
                    /* import 'gone'; */ const url = 'http://example.com/x'; import(\"./late\");";
        assert_eq!(
            parse_references(".tsx", body),
            ["react", "../shared/tokens.css", "./late"]
        );
        assert_eq!(
            parse_references(".js", "new URL('../assets/', import.meta.url)"),
            ["../assets/"]
        );
    }

    #[test]
    fn stylesheets_and_markup_name_their_assets() {
        assert_eq!(
            parse_references(
                ".css",
                "@import \"@acme/ui/theme.css\";\n.a{background:url(./dot.svg)}"
            ),
            ["@acme/ui/theme.css", "./dot.svg"]
        );
        assert_eq!(
            parse_references(
                ".html",
                "<link HREF=\"./home.css\"><img src='../dot.svg'><a href=\"#top\">"
            ),
            ["./home.css", "../dot.svg"]
        );
        assert!(stylesheet_imports("@use 'tokens';").contains("tokens"));
        assert_eq!(parse_references(".png", "import 'x'"), Vec::<String>::new());
    }
}
