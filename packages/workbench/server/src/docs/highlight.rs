//! Syntax highlighting for docs pages: fenced code in the Markdown and
//! example source for Show code.
//!
//! A fixed set of languages, with highlight.js's names and aliases; anything
//! else is shown as escaped text. Tokens come from syntect's grammars (the
//! extended set two-face ships, which has TypeScript, TSX and SCSS) and are
//! marked with the `hljs-*` classes the docs page stylesheet colors.

use std::sync::LazyLock;
use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Text safe in HTML text and attribute values.
pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' | '<' | '>' | '"' | '\'' => {
                out.push_str("&#");
                out.push_str(&u32::from(character).to_string());
                out.push(';');
            }
            other => out.push(other),
        }
    }
    out
}

/// The way highlight.js escapes the text of a highlighted token.
fn escape_token(text: &str, out: &mut String) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
}

/// The highlight.js language a name or alias stands for.
fn language(name: &str) -> Option<&'static str> {
    Some(match name {
        "bash" | "sh" | "shell" | "zsh" => "bash",
        "css" => "css",
        "diff" => "diff",
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" => "json",
        "markdown" | "md" => "markdown",
        "scss" => "scss",
        "typescript" | "ts" | "tsx" | "mts" | "cts" => "typescript",
        "xml" | "html" | "svg" | "vue" | "astro" => "xml",
        "yaml" | "yml" => "yaml",
        _ => return None,
    })
}

/// The grammar for a name: JSX and TSX for those aliases, HTML for markup.
fn syntax(name: &str) -> Option<&'static SyntaxReference> {
    let extensions: &[&str] = match (name, language(name)?) {
        ("jsx", _) => &["jsx", "js"],
        ("tsx", _) => &["tsx", "ts"],
        ("xml" | "svg", _) => &["xml", "html"],
        (_, "bash") => &["sh", "bash"],
        (_, "css") => &["css"],
        (_, "diff") => &["diff", "patch"],
        (_, "javascript") => &["js", "mjs"],
        (_, "json") => &["json"],
        (_, "markdown") => &["md", "markdown"],
        (_, "scss") => &["scss"],
        (_, "typescript") => &["ts", "mts"],
        (_, "xml") => &["html", "htm"],
        (_, "yaml") => &["yaml", "yml"],
        _ => return None,
    };
    extensions
        .iter()
        .find_map(|extension| SYNTAXES.find_syntax_by_extension(extension))
}

/// Highlighted HTML for `code`, or the escaped text when the language is unknown.
pub(crate) fn highlight(code: &str, language_name: Option<&str>) -> String {
    let name = language_name.unwrap_or_default().trim().to_lowercase();
    match (!name.is_empty()).then(|| syntax(&name)).flatten() {
        Some(grammar) => highlighted(code, grammar).unwrap_or_else(|| escape_html(code)),
        None => escape_html(code),
    }
}

/// The language a source file is highlighted as, from its extension.
pub(crate) fn language_of(file: &str) -> String {
    let extension = file
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_lowercase())
        .filter(|extension| {
            !extension.is_empty() && extension.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or_default();
    if language(&extension).is_some() {
        extension
    } else {
        "plaintext".into()
    }
}

fn highlighted(code: &str, grammar: &SyntaxReference) -> Option<String> {
    let mut state = ParseState::new(grammar);
    let mut stack = ScopeStack::new();
    let mut out = String::with_capacity(code.len() * 2);
    let mut open: Option<&'static str> = None;
    let mut pending = String::new();
    for line in LinesWithEndings::from(code) {
        let operations = state.parse_line(line, &SYNTAXES).ok()?;
        let mut at = 0;
        for (position, operation) in operations {
            if position > at {
                token(
                    &line[at..position],
                    class_of(&stack),
                    &mut open,
                    &mut pending,
                    &mut out,
                );
                at = position;
            }
            apply(&mut stack, &operation)?;
        }
        if at < line.len() {
            token(
                &line[at..],
                class_of(&stack),
                &mut open,
                &mut pending,
                &mut out,
            );
        }
    }
    flush(&mut open, &mut pending, &mut out);
    Some(out)
}

fn apply(stack: &mut ScopeStack, operation: &ScopeStackOp) -> Option<()> {
    stack.apply(operation).ok()
}

/// Adds a token, joining it to the previous one when they share a class.
fn token(
    text: &str,
    class: Option<&'static str>,
    open: &mut Option<&'static str>,
    pending: &mut String,
    out: &mut String,
) {
    if class != *open {
        flush(open, pending, out);
        *open = class;
    }
    pending.push_str(text);
}

fn flush(open: &mut Option<&'static str>, pending: &mut String, out: &mut String) {
    if pending.is_empty() {
        return;
    }
    let trailing = if open.is_some() {
        let kept = pending.trim_end().len();
        pending.split_off(kept)
    } else {
        String::new()
    };
    if let Some(class) = open {
        out.push_str("<span class=\"");
        out.push_str(class);
        out.push_str("\">");
        escape_token(pending, out);
        out.push_str("</span>");
    } else {
        escape_token(pending, out);
    }
    escape_token(&trailing, out);
    pending.clear();
    *open = None;
}

/// Scope prefixes and the highlight.js class each stands for, most specific first.
const CLASSES: &[(&str, &str)] = &[
    ("comment", "hljs-comment"),
    ("markup.quote", "hljs-quote"),
    ("string.regexp", "hljs-regexp"),
    ("string", "hljs-string"),
    ("constant.numeric", "hljs-number"),
    ("constant.language", "hljs-literal"),
    ("constant.other.color", "hljs-number"),
    ("keyword.operator", ""),
    ("keyword", "hljs-keyword"),
    ("storage.type", "hljs-keyword"),
    ("storage.modifier", "hljs-keyword"),
    ("variable.language", "hljs-variable language_"),
    ("variable.other.normal.shell", "hljs-variable"),
    ("variable.other.special.shell", "hljs-variable"),
    ("variable.other.positional.shell", "hljs-variable"),
    ("variable.other.bracket.shell", "hljs-variable"),
    ("variable.css", "hljs-variable"),
    ("variable.scss", "hljs-variable"),
    ("entity.name.function", "hljs-title function_"),
    ("support.function", "hljs-built_in"),
    ("entity.name.class", "hljs-title class_"),
    ("entity.name.type", "hljs-title class_"),
    ("entity.other.inherited-class", "hljs-title class_"),
    ("entity.name.section", "hljs-section"),
    ("markup.heading", "hljs-section"),
    ("entity.other.attribute-name.class", "hljs-selector-class"),
    ("entity.other.attribute-name.id", "hljs-selector-id"),
    ("entity.other.attribute-name", "hljs-attr"),
    ("support.type.property-name", "hljs-attribute"),
    ("meta.mapping.key", "hljs-attr"),
    ("entity.name.tag.yaml", "hljs-attr"),
    ("entity.name.tag.css", "hljs-selector-tag"),
    ("entity.name.tag.scss", "hljs-selector-tag"),
    ("entity.name.tag", "hljs-name"),
    ("support.type", "hljs-type"),
    ("support.class", "hljs-built_in"),
    ("support.constant", "hljs-built_in"),
    ("markup.inserted", "hljs-addition"),
    ("markup.deleted", "hljs-deletion"),
    ("meta.diff.range", "hljs-meta"),
    ("meta.diff.header", "hljs-meta"),
    ("meta.preprocessor", "hljs-meta"),
    ("markup.bold", "hljs-strong"),
    ("markup.italic", "hljs-emphasis"),
    ("markup.underline.link", "hljs-link"),
];

/// Scopes that start embedded code: a token inside one takes no color from outside it.
const BOUNDARIES: &[&str] = &[
    "meta.embedded",
    "meta.template.expression",
    "source.",
    "text.",
];

/// The class of a token: the innermost scope that maps to one.
fn class_of(stack: &ScopeStack) -> Option<&'static str> {
    for scope in stack.as_slice().iter().rev() {
        let name = scope_name(*scope);
        if let Some((_, class)) = CLASSES
            .iter()
            .find(|(prefix, _)| starts_scope(&name, prefix))
        {
            return (!class.is_empty()).then_some(*class);
        }
        if BOUNDARIES.iter().any(|boundary| name.starts_with(boundary)) {
            return None;
        }
    }
    None
}

fn scope_name(scope: Scope) -> String {
    scope.build_string()
}

/// Whether `name` is `prefix` or a scope under it.
fn starts_scope(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.') || prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_languages_mark_tokens_with_highlight_js_classes() {
        let html = highlight("const card: string = \"<Card>\"; // note", Some("ts"));
        assert!(
            html.starts_with("<span class=\"hljs-keyword\">const</span>"),
            "{html}"
        );
        assert!(html.contains("&quot;&lt;Card&gt;&quot;"), "{html}");
        assert!(
            html.contains("<span class=\"hljs-comment\">// note</span>"),
            "{html}"
        );
        let markup = highlight("<a href=\"/x\">Hi</a>", Some("html"));
        assert!(markup.contains("hljs-name"), "{markup}");
        assert!(markup.contains("hljs-attr"), "{markup}");
        let yaml = highlight("title: Card\ncount: 2\n", Some("yml"));
        assert!(yaml.contains("hljs-attr"), "{yaml}");
        assert!(yaml.contains("hljs-number"), "{yaml}");
        let diff = highlight("+added\n-removed\n", Some("diff"));
        assert!(
            diff.contains("hljs-addition") && diff.contains("hljs-deletion"),
            "{diff}"
        );
    }

    #[test]
    fn every_language_and_alias_has_a_grammar() {
        for name in [
            "bash",
            "sh",
            "shell",
            "zsh",
            "css",
            "diff",
            "javascript",
            "js",
            "jsx",
            "mjs",
            "cjs",
            "json",
            "markdown",
            "md",
            "scss",
            "typescript",
            "ts",
            "tsx",
            "mts",
            "cts",
            "xml",
            "html",
            "svg",
            "vue",
            "astro",
            "yaml",
            "yml",
        ] {
            assert!(syntax(name).is_some(), "{name}");
        }
    }

    #[test]
    fn highlighting_keeps_the_text_and_unknown_languages_are_escaped() {
        let code = "export const basic = () => <Card>Basic</Card>;\n\nfunction f(a) {\n  return `x ${a}`;\n}";
        let html = highlight(code, Some("tsx"));
        let text = regex::Regex::new("<[^>]+>")
            .expect("tags")
            .replace_all(&html, "");
        let decoded = text
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&amp;", "&");
        assert_eq!(decoded, code);
        assert_eq!(
            highlight("a < b & \"c\"", Some("cobol")),
            "a &#60; b &#38; &#34;c&#34;"
        );
        assert_eq!(highlight("x", None), "x");
    }

    #[test]
    fn a_files_language_comes_from_its_extension() {
        assert_eq!(language_of("docs/card.examples.tsx"), "tsx");
        assert_eq!(language_of("docs/Card.VUE"), "vue");
        assert_eq!(language_of("docs/card.rs"), "plaintext");
        assert_eq!(language_of("Makefile"), "plaintext");
    }
}
