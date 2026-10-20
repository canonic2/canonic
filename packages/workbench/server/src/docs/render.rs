//! A docs page's Markdown as HTML: GitHub Flavored Markdown, with each
//! ```` ```example <id> ```` block replaced by that example's panel.
//!
//! Headings get GitHub-style anchors, relative images and links resolve
//! against the Markdown file, links to other docs pages are marked so the
//! canvas can open them, and fenced code is highlighted. What each panel
//! shows is the caller's decision; this module only lays it out.

use super::highlight::{escape_html, highlight};
use super::markdown::{ExamplePlacement, heading_anchor, read_docs_markdown};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

/// What a placement's panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelStatus {
    Ready,
    /// Waits for the page's script to learn the bundle; it then becomes one of the others.
    Pending,
    Missing,
    Invalid,
    Unavailable,
}

impl PanelStatus {
    fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Pending => "pending",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One panel as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PanelView {
    pub(crate) status: PanelStatus,
    /// Why the example isn't shown, for every status but `Ready` and `Pending`.
    pub(crate) note: Option<String>,
}

impl PanelView {
    pub(crate) fn shown(status: PanelStatus) -> Self {
        Self { status, note: None }
    }

    pub(crate) fn noted(status: PanelStatus, note: impl Into<String>) -> Self {
        Self {
            status,
            note: Some(note.into()),
        }
    }
}

/// What to render.
pub(crate) struct RenderInput<'input> {
    /// The Markdown text.
    pub(crate) source: &'input str,
    /// The Markdown file, relative to the project root, with `/` separators.
    pub(crate) file: &'input str,
    /// The project-relative sources of every declared docs page, for links between them.
    pub(crate) docs_pages: &'input HashSet<String>,
}

/// A rendered docs page.
#[derive(Debug, Clone)]
pub(crate) struct RenderedDocs {
    pub(crate) html: String,
    /// The first level-1 heading's text, for the document title.
    pub(crate) title: Option<String>,
}

/* Lucide's code and copy icons. */
const CODE_ICON: &str = r#"<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/></svg>"#;
const COPY_ICON: &str = r#"<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>"#;

/// Renders a docs page; `panel` decides what each placement shows.
pub(crate) fn render_docs(
    input: &RenderInput<'_>,
    mut panel: impl FnMut(&ExamplePlacement) -> PanelView,
) -> RenderedDocs {
    let model = read_docs_markdown(input.source);
    let mut waiting: HashMap<&str, VecDeque<&ExamplePlacement>> = HashMap::new();
    for placement in &model.placements {
        waiting
            .entry(placement.id.as_str())
            .or_default()
            .push_back(placement);
    }
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<Spanned<'_>> = Parser::new_ext(&model.body, options)
        .into_offset_iter()
        .collect();
    let mut anchors: HashMap<String, usize> = HashMap::new();
    let mut title: Option<String> = None;
    let mut out: Vec<Event<'_>> = Vec::with_capacity(events.len());
    let mut at = 0;
    while at < events.len() {
        let (event, _) = &events[at];
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let end = closing(&events, at);
                let inner = &events[at + 1..end];
                heading_events(
                    &model.body,
                    inner,
                    *level as usize,
                    &mut anchors,
                    &mut title,
                    &mut out,
                );
                at = end + 1;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let end = closing(&events, at);
                let html = code_html(kind, &events[at + 1..end], &mut waiting, &mut panel);
                out.push(Event::Html(html.into()));
                at = end + 1;
            }
            Event::Start(Tag::Image {
                dest_url,
                title: image_title,
                ..
            }) => {
                let end = closing(&events, at);
                let alt = plain(&events[at + 1..end]);
                let mut html = format!(
                    "<img src=\"{}\" alt=\"{}\"",
                    escape_html(&resolve_against(input.file, dest_url)),
                    escape_html(&alt)
                );
                if !image_title.is_empty() {
                    let _ = write!(html, " title=\"{}\"", escape_html(image_title));
                }
                html.push('>');
                out.push(Event::InlineHtml(html.into()));
                at = end + 1;
            }
            Event::Start(Tag::Link {
                dest_url,
                title: link_title,
                ..
            }) => {
                let resolved = resolve_against(input.file, dest_url);
                let mut html = format!("<a href=\"{}\"", escape_html(&resolved));
                if let Some(page) = docs_page_of(&resolved, input.docs_pages) {
                    let _ = write!(html, " data-wb-docs-page=\"{}\"", escape_html(&page));
                }
                if !link_title.is_empty() {
                    let _ = write!(html, " title=\"{}\"", escape_html(link_title));
                }
                html.push('>');
                out.push(Event::InlineHtml(html.into()));
                at += 1;
            }
            Event::End(TagEnd::Link) => {
                out.push(Event::InlineHtml(CowStr::Borrowed("</a>")));
                at += 1;
            }
            other => {
                out.push(other.clone());
                at += 1;
            }
        }
    }
    let mut html = String::with_capacity(model.body.len() * 2);
    pulldown_cmark::html::push_html(&mut html, out.into_iter());
    RenderedDocs { html, title }
}

type Spanned<'source> = (Event<'source>, std::ops::Range<usize>);

/// A heading with its anchor; the first level-1 heading titles the page.
fn heading_events<'source>(
    body: &str,
    inner: &[Spanned<'source>],
    depth: usize,
    anchors: &mut HashMap<String, usize>,
    title: &mut Option<String>,
    out: &mut Vec<Event<'source>>,
) {
    let mut base = heading_anchor(&raw_text(body, inner));
    if base.is_empty() {
        base = "section".into();
    }
    let count = anchors.entry(base.clone()).or_default();
    let anchor = if *count == 0 {
        base.clone()
    } else {
        format!("{base}-{count}")
    };
    *count += 1;
    if depth == 1 && title.is_none() {
        *title = Some(plain(inner));
    }
    out.push(Event::Html(
        format!("<h{depth} id=\"{}\">", escape_html(&anchor)).into(),
    ));
    out.extend(inner.iter().map(|(event, _)| event.clone()));
    out.push(Event::Html(format!("</h{depth}>\n").into()));
}

/// A code block: an example's panel, or highlighted code.
fn code_html(
    kind: &CodeBlockKind<'_>,
    inner: &[Spanned<'_>],
    waiting: &mut HashMap<&str, VecDeque<&ExamplePlacement>>,
    panel: &mut impl FnMut(&ExamplePlacement) -> PanelView,
) -> String {
    let mut text: String = inner
        .iter()
        .filter_map(|(event, _)| {
            if let Event::Text(text) = event {
                Some(text.as_ref())
            } else {
                None
            }
        })
        .collect();
    if text.ends_with('\n') {
        text.pop();
    }
    let info = match kind {
        CodeBlockKind::Fenced(info) => info.trim().to_owned(),
        CodeBlockKind::Indented => String::new(),
    };
    let words: Vec<&str> = info.split_whitespace().collect();
    let first = words.first().copied().unwrap_or_default();
    if first == "example" {
        let id = words.get(1).copied().unwrap_or_default();
        let placed = waiting
            .get_mut(id)
            .and_then(VecDeque::pop_front)
            .or_else(|| waiting.get_mut("").and_then(VecDeque::pop_front));
        if let Some(placement) = placed {
            return panel_html(placement, &panel(placement));
        }
    }
    let class = if first.is_empty() {
        String::new()
    } else {
        format!(" class=\"language-{}\"", escape_html(first))
    };
    let language = (!first.is_empty()).then_some(first);
    format!(
        "<pre class=\"wb-docs-code\"><code{class}>{}</code></pre>\n",
        highlight(&text, language)
    )
}

/// The index of the event that closes the one at `start`.
fn closing(events: &[Spanned<'_>], start: usize) -> usize {
    let mut depth = 0usize;
    for (index, (event, _)) in events.iter().enumerate().skip(start) {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return index;
                }
            }
            _ => {}
        }
    }
    events.len() - 1
}

/// The Markdown source a run of inline events came from.
fn raw_text(body: &str, inner: &[Spanned<'_>]) -> String {
    let start = inner.iter().map(|(_, range)| range.start).min();
    let end = inner.iter().map(|(_, range)| range.end).max();
    match (start, end) {
        (Some(start), Some(end)) => body.get(start..end).unwrap_or_default().trim().to_owned(),
        _ => String::new(),
    }
}

/// The text of a run of inline events, without markup.
fn plain(inner: &[Spanned<'_>]) -> String {
    inner
        .iter()
        .filter_map(|(event, _)| match event {
            Event::Text(text) | Event::Code(text) => Some(text.as_ref()),
            Event::SoftBreak | Event::HardBreak => Some(" "),
            _ => None,
        })
        .collect()
}

fn panel_html(placement: &ExamplePlacement, view: &PanelView) -> String {
    let id = escape_html(&placement.id);
    let mut out = format!(
        "<figure class=\"wb-docs-example\" id=\"example-{id}\" data-wb-example=\"{id}\" data-status=\"{}\">",
        view.status.name()
    );
    if matches!(view.status, PanelStatus::Ready | PanelStatus::Pending) {
        out.push_str("<div class=\"wb-docs-example-stage\" data-wb-example-stage></div>");
        let _ = write!(
            out,
            "<div class=\"wb-docs-example-tools\"><button type=\"button\" class=\"wb-docs-tool\" data-wb-show-code aria-expanded=\"false\" aria-label=\"Show code\" title=\"Show code\">{CODE_ICON}</button><button type=\"button\" class=\"wb-docs-tool\" data-wb-copy-code aria-label=\"Copy code\" title=\"Copy code\">{COPY_ICON}</button></div>"
        );
        out.push_str("<div class=\"wb-docs-example-code\" data-wb-example-code hidden></div>");
    } else {
        let note = view
            .note
            .as_deref()
            .unwrap_or("This example isn’t available.");
        let _ = write!(
            out,
            "<p class=\"wb-docs-example-note\">{}</p>",
            escape_html(note)
        );
    }
    out.push_str("</figure>");
    if let Some(caption) = &placement.caption {
        let _ = write!(
            out,
            "<p class=\"wb-docs-caption\">{}</p>",
            escape_html(caption)
        );
    }
    out.push('\n');
    out
}

fn has_scheme(href: &str) -> bool {
    let Some((scheme, _)) = href.split_once(':') else {
        return false;
    };
    let mut characters = scheme.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '.' | '-')
        })
}

/// A relative URL in the Markdown, as an address on the project server.
/// Absolute URLs, root paths, and in-page fragments stay as they are, as does
/// a path that would climb above the project root.
pub(crate) fn resolve_against(file: &str, href: &str) -> String {
    if href.is_empty() || href.starts_with('#') || href.starts_with('/') || has_scheme(href) {
        return href.to_owned();
    }
    let split = href.find(['?', '#']).unwrap_or(href.len());
    let (path_part, suffix) = href.split_at(split);
    let mut segments: Vec<&str> = file.split('/').collect();
    segments.pop();
    for segment in path_part.split('/') {
        match segment {
            ".." => {
                if segments.pop().is_none() {
                    return href.to_owned();
                }
            }
            "" | "." => {}
            other => segments.push(other),
        }
    }
    let encoded: Vec<String> = segments
        .iter()
        .map(|segment| encode_uri_component(&decode_safely(segment)))
        .collect();
    format!("/{}{suffix}", encoded.join("/"))
}

/// `encodeURIComponent`: everything but unreserved characters, percent-encoded as UTF-8.
pub(crate) fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// `decodeURIComponent`, or the text itself when it isn't valid percent-encoded UTF-8.
pub(crate) fn decode_safely(text: &str) -> String {
    decode(text).unwrap_or_else(|| text.to_owned())
}

/// `decodeURIComponent`, or `None` when the text isn't valid percent-encoded UTF-8.
pub(crate) fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = text
                .get(at + 1..at + 3)
                .and_then(|digits| u8::from_str_radix(digits, 16).ok());
            out.push(hex?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn docs_page_of(href: &str, pages: &HashSet<String>) -> Option<String> {
    if !href.starts_with('/') || href.starts_with("//") {
        return None;
    }
    let path = href.split(['?', '#']).next().unwrap_or_default();
    let page = decode_safely(&path[1..]);
    pages.contains(&page).then_some(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "---\ntitle: ignored\n---\n# Card `div`\n\nA plain surface. See [Button](../button/button.md#sizes), [the guide](guide.html), and [home](https://example.com).\n\n![Anatomy](images/anatomy.png \"Parts\")\n\n## Basic\n\n```example basic\ncaption: children\n```\n\n## Basic\n\n```example nested\n```\n\n```example basic\n```\n\n| Prop | Type |\n| --- | --- |\n| `children` | `ReactNode` |\n\n```ts\nconst card: string = \"<Card>\";\n```";

    fn render(panel: impl FnMut(&ExamplePlacement) -> PanelView) -> RenderedDocs {
        let pages: HashSet<String> = ["docs/button/button.md".to_owned()].into();
        render_docs(
            &RenderInput {
                source: SOURCE,
                file: "docs/card/card.md",
                docs_pages: &pages,
            },
            panel,
        )
    }

    fn matches(pattern: &str, html: &str) {
        assert!(
            regex::Regex::new(pattern).expect("pattern").is_match(html),
            "{pattern} not in {html}"
        );
    }

    #[test]
    fn example_blocks_become_panels_in_the_state_the_lens_gives_them_followed_by_their_caption() {
        let mut seen = vec![];
        let rendered = render(|placement| {
            seen.push(format!(
                "{}{}",
                placement.id,
                if placement.duplicate { " (again)" } else { "" }
            ));
            if placement.duplicate {
                PanelView::noted(PanelStatus::Invalid, "basic is placed more than once.")
            } else if placement.id == "basic" {
                PanelView::shown(PanelStatus::Ready)
            } else {
                PanelView::noted(PanelStatus::Missing, "Not available in Dark")
            }
        });
        assert_eq!(seen, ["basic", "nested", "basic (again)"]);
        let html = &rendered.html;
        matches(
            r#"<figure class="wb-docs-example" id="example-basic" data-wb-example="basic" data-status="ready"><div class="wb-docs-example-stage" data-wb-example-stage></div>"#,
            html,
        );
        matches(
            r#"(?s)aria-label="Show code".*aria-label="Copy code".*</figure><p class="wb-docs-caption">children</p>"#,
            html,
        );
        matches(
            r#"data-wb-example="nested" data-status="missing"><p class="wb-docs-example-note">Not available in Dark</p></figure>"#,
            html,
        );
        matches(
            r#"data-status="invalid"><p class="wb-docs-example-note">basic is placed more than once.</p>"#,
            html,
        );
        assert!(!html.contains("title: ignored") && !html.contains("caption: children"));
    }

    #[test]
    fn headings_get_unique_anchors_and_the_first_one_titles_the_page() {
        let rendered = render(|_| PanelView::shown(PanelStatus::Ready));
        matches(
            r#"<h1 id="card-div">Card <code>div</code></h1>"#,
            &rendered.html,
        );
        matches(
            r#"(?s)<h2 id="basic">Basic</h2>.*<h2 id="basic-1">Basic</h2>"#,
            &rendered.html,
        );
        assert_eq!(rendered.title.as_deref(), Some("Card div"));
    }

    #[test]
    fn relative_links_and_images_resolve_against_the_markdown_file_and_links_to_docs_pages_are_marked()
     {
        let html = render(|_| PanelView::shown(PanelStatus::Ready)).html;
        matches(
            r#"<a href="/docs/button/button.md#sizes" data-wb-docs-page="docs/button/button.md">Button</a>"#,
            &html,
        );
        matches(r#"<a href="/docs/card/guide.html">the guide</a>"#, &html);
        matches(r#"<a href="https://example.com">home</a>"#, &html);
        matches(
            r#"<img src="/docs/card/images/anatomy.png" alt="Anatomy" title="Parts">"#,
            &html,
        );
    }

    #[test]
    fn tables_render_and_fenced_code_is_highlighted_and_escaped() {
        let html = render(|_| PanelView::shown(PanelStatus::Ready)).html;
        matches(r"(?s)<table>.*<code>children</code>.*</table>", &html);
        matches(
            r#"<pre class="wb-docs-code"><code class="language-ts"><span class="hljs-keyword">const</span>"#,
            &html,
        );
        matches(r"&#34;&lt;Card&gt;&#34;|&quot;&lt;Card&gt;&quot;", &html);
    }

    #[test]
    fn a_pending_panel_has_its_stage_and_an_example_without_an_id_takes_the_unnamed_placement() {
        let pages = HashSet::new();
        let source = "```example\n```\n\n```example pending\n```";
        let rendered = render_docs(
            &RenderInput {
                source,
                file: "card.md",
                docs_pages: &pages,
            },
            |placement| {
                if placement.duplicate {
                    PanelView::noted(PanelStatus::Invalid, "This example block names no example.")
                } else {
                    PanelView::shown(PanelStatus::Pending)
                }
            },
        );
        matches(
            r#"data-wb-example="" data-status="invalid"><p class="wb-docs-example-note">This example block names no example.</p>"#,
            &rendered.html,
        );
        matches(
            r#"data-wb-example="pending" data-status="pending"><div class="wb-docs-example-stage""#,
            &rendered.html,
        );
    }

    #[test]
    fn relative_addresses_never_climb_above_the_project_root() {
        assert_eq!(
            resolve_against("docs/card.md", "../../outside.png"),
            "../../outside.png"
        );
        assert_eq!(
            resolve_against("docs/card.md", "./a b.png?x=1"),
            "/docs/a%20b.png?x=1"
        );
        assert_eq!(
            resolve_against("docs/card.md", "a.png?x=1#top"),
            "/docs/a.png?x=1#top"
        );
        assert_eq!(
            resolve_against("docs/card.md", "%E2%9C%93.png"),
            "/docs/%E2%9C%93.png"
        );
        assert_eq!(resolve_against("docs/card.md", "#props"), "#props");
        assert_eq!(
            resolve_against("card.md", "mailto:hi@example.com"),
            "mailto:hi@example.com"
        );
    }
}
