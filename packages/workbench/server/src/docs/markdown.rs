//! Reads a docs page's Markdown for what Workbench needs before rendering it:
//! front matter to strip, the example placements (fenced ```` ```example <id> ````
//! blocks and their options), and the headings that name and anchor them.
//! Rendering the Markdown to HTML is separate; this module only scans it.
//! Fences follow `CommonMark`: three or more backticks or tildes, indented at
//! most three spaces, closed by a run of the same character at least as long.

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// A heading of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocsHeading {
    pub(crate) level: usize,
    pub(crate) text: String,
    /// GitHub-style anchor, unique within the page.
    pub(crate) anchor: String,
    /// 1-based line in the original file.
    pub(crate) line: usize,
}

/// Where the Markdown places one example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExamplePlacement {
    pub(crate) id: String,
    /// 1-based line of the opening fence in the original file.
    pub(crate) line: usize,
    pub(crate) caption: Option<String>,
    /// The nearest heading above the placement, if any.
    pub(crate) heading: Option<DocsHeading>,
    /// The sidebar label: the heading's text, or the ID when there is no
    /// heading or another example shares it.
    pub(crate) label: String,
    /// A second placement of an ID already placed, or a malformed one; it
    /// renders as an error panel.
    pub(crate) duplicate: bool,
    heading_at: Option<usize>,
}

/// A problem with the page's Markdown, by line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocsProblem {
    pub(crate) line: usize,
    pub(crate) message: String,
}

/// A docs page's Markdown as Workbench reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocsMarkdown {
    /// The Markdown without front matter.
    pub(crate) body: String,
    /// How many lines of front matter were removed, so body line + offset = file line.
    pub(crate) body_line_offset: usize,
    pub(crate) headings: Vec<DocsHeading>,
    pub(crate) placements: Vec<ExamplePlacement>,
    pub(crate) problems: Vec<DocsProblem>,
}

static FENCE_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^( {0,3})(`{3,}|~{3,})(.*)$").expect("fence pattern"));
static FENCE_CLOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}(`+|~+)[ \t]*$").expect("closing fence pattern"));
static ATX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$").expect("heading pattern")
});
static SETEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}(=+|-+)[ \t]*$").expect("setext pattern"));
static OPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([A-Za-z][\w-]*)\s*:\s*(.*?)\s*$").expect("option pattern"));
static KEBAB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("kebab pattern"));
static CODE_SPAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]*)`").expect("code"));
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!?\[([^\]]*)\]\([^)]*\)").expect("link"));
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("tag"));
static EMPHASIS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\*\*(.+?)\*\*|__(.+?)__|\*(.+?)\*|_(.+?)_|~~(.+?)~~").expect("emphasis")
});
static PUNCTUATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\s_-]").expect("punctuation"));

/// Example IDs are kebab-case, like scenario IDs.
pub(crate) fn is_example_id(value: &str) -> bool {
    KEBAB.is_match(value)
}

struct PendingExample {
    id: String,
    line: usize,
    caption: Option<String>,
    invalid: bool,
}

struct Fence {
    character: char,
    length: usize,
    start: usize,
    example: Option<PendingExample>,
}

/// Scans a docs page's Markdown.
pub(crate) fn read_docs_markdown(source: &str) -> DocsMarkdown {
    let text = source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let (body, offset) = strip_front_matter(&text);
    let mut headings: Vec<DocsHeading> = vec![];
    let mut placements: Vec<ExamplePlacement> = vec![];
    let mut problems: Vec<DocsProblem> = vec![];
    let mut anchors: HashMap<String, usize> = HashMap::new();
    let mut placed: HashSet<String> = HashSet::new();
    let mut fence: Option<Fence> = None;
    let mut previous_paragraph: Option<(String, usize)> = None;

    for (index, line) in body.split('\n').enumerate() {
        let file_line = index + 1 + offset;
        if let Some(open) = fence.as_mut() {
            if is_closing_fence(line, open.character, open.length) {
                if let Some(example) = open.example.take() {
                    placements.push(finish_example(
                        example,
                        &headings,
                        &mut placed,
                        &mut problems,
                    ));
                }
                fence = None;
            } else if let Some(example) = open.example.as_mut() {
                read_option(example, line, file_line, &mut problems);
            }
            continue;
        }

        if let Some(open) = FENCE_OPEN.captures(line) {
            let marker = &open[2];
            let info = &open[3];
            if !(marker.starts_with('`') && info.contains('`')) {
                fence = Some(Fence {
                    character: marker.chars().next().unwrap_or('`'),
                    length: marker.len(),
                    start: file_line,
                    example: start_example(info.trim(), file_line, &mut problems),
                });
                previous_paragraph = None;
                continue;
            }
        }

        if let Some(atx) = ATX.captures(line) {
            let raw = atx.get(2).map_or("", |text| text.as_str());
            headings.push(heading(atx[1].len(), raw, file_line, &mut anchors));
            previous_paragraph = None;
            continue;
        }

        if let Some(setext) = SETEXT.captures(line)
            && let Some((text, at)) = previous_paragraph.take()
        {
            let level = if setext[1].starts_with('=') { 1 } else { 2 };
            headings.push(heading(level, &text, at, &mut anchors));
            continue;
        }

        previous_paragraph = (!line.trim().is_empty() && !line.starts_with("    "))
            .then(|| (line.trim().to_owned(), file_line));
    }

    if let Some(open) = fence
        && let Some(example) = open.example
    {
        problems.push(DocsProblem {
            line: open.start,
            message: format!("example “{}” has no closing fence", example.id),
        });
        placements.push(finish_example(
            example,
            &headings,
            &mut placed,
            &mut problems,
        ));
    }

    label_placements(&mut placements);
    DocsMarkdown {
        body: body.to_owned(),
        body_line_offset: offset,
        headings,
        placements,
        problems,
    }
}

/// GitHub's heading anchor: lowercase, punctuation dropped, spaces to hyphens.
pub(crate) fn heading_anchor(text: &str) -> String {
    let lower = plain_text(text).to_lowercase();
    PUNCTUATION
        .replace_all(&lower, "")
        .trim()
        .chars()
        .map(|character| {
            if character.is_whitespace() {
                '-'
            } else {
                character
            }
        })
        .collect()
}

fn strip_front_matter(text: &str) -> (&str, usize) {
    let mut lines = text.split('\n');
    let opens = lines.next().is_some_and(|first| {
        first
            .strip_prefix("---")
            .is_some_and(|rest| rest.trim_matches([' ', '\t']).is_empty())
    });
    if !opens || !text.contains('\n') {
        return (text, 0);
    }
    let mut consumed = text.find('\n').map_or(text.len(), |at| at + 1);
    for (index, line) in lines.enumerate() {
        consumed += line.len() + 1;
        let marker = line.trim_end_matches([' ', '\t']);
        if marker == "---" || marker == "..." {
            let body = text.get(consumed.min(text.len())..).unwrap_or_default();
            return (body, index + 2);
        }
    }
    (text, 0)
}

fn is_closing_fence(line: &str, character: char, length: usize) -> bool {
    FENCE_CLOSE.captures(line).is_some_and(|found| {
        let run = &found[1];
        run.starts_with(character) && run.len() >= length
    })
}

fn start_example(
    info: &str,
    line: usize,
    problems: &mut Vec<DocsProblem>,
) -> Option<PendingExample> {
    let words: Vec<&str> = info.split_whitespace().collect();
    if words.first() != Some(&"example") {
        return None;
    }
    let id = words.get(1).copied().unwrap_or_default();
    if id.is_empty() {
        problems.push(DocsProblem {
            line,
            message: "example block names no example; write ```example <id>".into(),
        });
        return Some(PendingExample {
            id: String::new(),
            line,
            caption: None,
            invalid: true,
        });
    }
    if words.len() > 2 {
        problems.push(DocsProblem {
            line,
            message: format!(
                "example “{id}”: unexpected “{}” after the ID",
                words[2..].join(" ")
            ),
        });
    }
    if !is_example_id(id) {
        problems.push(DocsProblem {
            line,
            message: format!("example id “{id}” must be kebab-case"),
        });
        return Some(PendingExample {
            id: id.into(),
            line,
            caption: None,
            invalid: true,
        });
    }
    Some(PendingExample {
        id: id.into(),
        line,
        caption: None,
        invalid: false,
    })
}

fn read_option(
    example: &mut PendingExample,
    line: &str,
    file_line: usize,
    problems: &mut Vec<DocsProblem>,
) {
    if line.trim().is_empty() {
        return;
    }
    let Some(found) = OPTION.captures(line) else {
        problems.push(DocsProblem {
            line: file_line,
            message: format!(
                "example “{}”: expected “key: value”, found “{}”",
                example.id,
                line.trim()
            ),
        });
        return;
    };
    let key = &found[1];
    if key != "caption" {
        problems.push(DocsProblem {
            line: file_line,
            message: format!("example “{}”: unknown key “{key}”", example.id),
        });
        return;
    }
    let value = &found[2];
    example.caption = (!value.is_empty()).then(|| value.to_owned());
}

fn finish_example(
    example: PendingExample,
    headings: &[DocsHeading],
    placed: &mut HashSet<String>,
    problems: &mut Vec<DocsProblem>,
) -> ExamplePlacement {
    let duplicate = !example.invalid && placed.contains(&example.id);
    if duplicate {
        problems.push(DocsProblem {
            line: example.line,
            message: format!("example “{}” is placed more than once", example.id),
        });
    }
    if !example.invalid {
        placed.insert(example.id.clone());
    }
    ExamplePlacement {
        label: example.id.clone(),
        id: example.id,
        line: example.line,
        caption: example.caption,
        heading: headings.last().cloned(),
        heading_at: headings.len().checked_sub(1),
        duplicate: duplicate || example.invalid,
    }
}

fn label_placements(placements: &mut [ExamplePlacement]) {
    let mut per_heading: HashMap<usize, usize> = HashMap::new();
    for placement in placements.iter() {
        if let Some(at) = placement.heading_at
            && !placement.duplicate
        {
            *per_heading.entry(at).or_default() += 1;
        }
    }
    for placement in placements.iter_mut() {
        let shared = placement
            .heading_at
            .is_none_or(|at| per_heading.get(&at).copied().unwrap_or_default() > 1);
        placement.label = match &placement.heading {
            Some(heading) if !shared => heading.text.clone(),
            _ => placement.id.clone(),
        };
    }
}

fn heading(
    level: usize,
    raw: &str,
    line: usize,
    anchors: &mut HashMap<String, usize>,
) -> DocsHeading {
    let text = plain_text(raw);
    let mut base = heading_anchor(raw);
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
    DocsHeading {
        level,
        text,
        anchor,
        line,
    }
}

/// Heading text without inline Markdown: code spans, links, images,
/// emphasis, and HTML tags.
fn plain_text(raw: &str) -> String {
    let text = CODE_SPAN.replace_all(raw, "$1");
    let text = LINK.replace_all(&text, "$1");
    let text = TAG.replace_all(&text, "");
    let text = EMPHASIS.replace_all(&text, |found: &regex::Captures<'_>| {
        (1..=5)
            .find_map(|group| found.get(group))
            .map_or_else(String::new, |inner| inner.as_str().to_owned())
    });
    text.trim().to_owned()
}

/// A placement's panel in one lens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelStatus {
    /// The lens exports it.
    Ready,
    /// Placed, but this lens has no such example.
    Missing,
    /// A duplicate or malformed placement.
    Invalid,
}

/// One placement's panel, matched with a lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExamplePanel {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) caption: Option<String>,
    /// 1-based line of the placement in the Markdown file.
    pub(crate) line: usize,
    pub(crate) status: PanelStatus,
}

/// A page's placements matched with what one lens exports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LensMatch {
    pub(crate) panels: Vec<ExamplePanel>,
    /// IDs the lens exports that no placement names, in the lens's order.
    pub(crate) unplaced: Vec<String>,
}

/// Every placement keeps its panel in every lens, so switching lenses never
/// moves the text: an example the lens lacks becomes a placeholder, and an
/// example the Markdown never places is a problem and is not shown.
pub(crate) fn match_lens(placements: &[ExamplePlacement], exported: &[String]) -> LensMatch {
    let mut placed: HashSet<&str> = HashSet::new();
    let panels = placements
        .iter()
        .map(|placement| {
            if !placement.duplicate {
                placed.insert(&placement.id);
            }
            let status = if placement.duplicate {
                PanelStatus::Invalid
            } else if exported.contains(&placement.id) {
                PanelStatus::Ready
            } else {
                PanelStatus::Missing
            };
            ExamplePanel {
                id: placement.id.clone(),
                label: placement.label.clone(),
                caption: placement.caption.clone(),
                line: placement.line,
                status,
            }
        })
        .collect();
    let unplaced = exported
        .iter()
        .filter(|id| !placed.contains(id.as_str()))
        .cloned()
        .collect();
    LensMatch { panels, unplaced }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(lines: &[&str]) -> DocsMarkdown {
        read_docs_markdown(&lines.join("\n"))
    }

    #[test]
    fn example_blocks_become_placements_labelled_by_their_heading() {
        let doc = read(&[
            "# Card",
            "",
            "A plain surface.",
            "",
            "## Basic",
            "",
            "```example basic",
            "caption: children",
            "```",
            "",
            "The surface alone.",
            "",
            "## With custom style",
            "",
            "```example with-custom-style",
            "```",
        ]);
        assert_eq!(doc.problems, Vec::<DocsProblem>::new());
        let found: Vec<(&str, usize, Option<&str>, &str, bool)> = doc
            .placements
            .iter()
            .map(|p| {
                (
                    p.id.as_str(),
                    p.line,
                    p.caption.as_deref(),
                    p.label.as_str(),
                    p.duplicate,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("basic", 7, Some("children"), "Basic", false),
                ("with-custom-style", 15, None, "With custom style", false),
            ]
        );
    }

    #[test]
    fn examples_sharing_a_heading_or_with_none_are_labelled_by_id() {
        let doc = read(&[
            "```example lone",
            "```",
            "## Sizes",
            "```example small",
            "```",
            "```example large",
            "```",
        ]);
        let labels: Vec<&str> = doc.placements.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["lone", "small", "large"]);
    }

    #[test]
    fn front_matter_is_stripped_and_lines_still_refer_to_the_file() {
        let doc = read(&[
            "---",
            "title: Card",
            "---",
            "# Card",
            "```example basic",
            "```",
        ]);
        assert!(doc.body.starts_with("# Card"));
        assert_eq!(doc.body_line_offset, 3);
        assert_eq!(doc.headings[0].line, 4);
        assert_eq!(doc.placements[0].line, 5);
        let unclosed = read(&["---", "title: Card", "# Card"]);
        assert_eq!(unclosed.body_line_offset, 0);
        assert!(unclosed.body.starts_with("---"));
    }

    #[test]
    fn code_fences_that_are_not_examples_are_skipped_even_holding_example_like_text() {
        let doc = read(&[
            "````md",
            "```example basic",
            "```",
            "````",
            "~~~ts",
            "# not a heading",
            "~~~",
        ]);
        assert_eq!(doc.placements, Vec::<ExamplePlacement>::new());
        assert_eq!(doc.headings, Vec::<DocsHeading>::new());
    }

    #[test]
    fn placement_problems_name_their_line() {
        let doc = read(&[
            "```example",
            "```",
            "```example Basic",
            "```",
            "```example basic extra",
            "title: Nope",
            "just text",
            "```",
            "```example basic",
            "```",
            "```example open",
        ]);
        let problems: Vec<(usize, &str)> = doc
            .problems
            .iter()
            .map(|p| (p.line, p.message.as_str()))
            .collect();
        assert_eq!(
            problems,
            [
                (1, "example block names no example; write ```example <id>"),
                (3, "example id “Basic” must be kebab-case"),
                (5, "example “basic”: unexpected “extra” after the ID"),
                (6, "example “basic”: unknown key “title”"),
                (
                    7,
                    "example “basic”: expected “key: value”, found “just text”"
                ),
                (9, "example “basic” is placed more than once"),
                (11, "example “open” has no closing fence"),
            ]
        );
        let placements: Vec<(&str, bool)> = doc
            .placements
            .iter()
            .map(|p| (p.id.as_str(), p.duplicate))
            .collect();
        assert_eq!(
            placements,
            [
                ("", true),
                ("Basic", true),
                ("basic", false),
                ("basic", true),
                ("open", false)
            ]
        );
    }

    #[test]
    fn headings_get_unique_github_style_anchors_including_setext_headings() {
        let doc = read(&[
            "# Card `div`",
            "Usage",
            "-----",
            "## Usage",
            "### [Props](#props) **table**",
        ]);
        let headings: Vec<(usize, &str, &str)> = doc
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.anchor.as_str()))
            .collect();
        assert_eq!(
            headings,
            [
                (1, "Card div", "card-div"),
                (2, "Usage", "usage"),
                (2, "Usage", "usage-1"),
                (3, "Props table", "props-table")
            ]
        );
        assert_eq!(heading_anchor("Color & Variant"), "color--variant");
        assert_eq!(heading_anchor("!!!"), "");
    }

    #[test]
    fn example_ids_are_kebab_case() {
        assert!(is_example_id("with-custom-style"));
        assert!(is_example_id("size2x"));
        assert!(!is_example_id("WithCustomStyle"));
        assert!(!is_example_id("trailing-"));
        assert!(!is_example_id(""));
    }

    fn placed_twice() -> DocsMarkdown {
        read(&[
            "## Basic",
            "```example basic",
            "```",
            "## Nested",
            "```example nested",
            "```",
            "```example basic",
            "```",
        ])
    }

    #[test]
    fn every_placement_keeps_a_panel_and_a_lens_without_the_example_shows_it_missing() {
        let matched = match_lens(&placed_twice().placements, &["basic".into()]);
        let panels: Vec<(&str, PanelStatus)> = matched
            .panels
            .iter()
            .map(|p| (p.id.as_str(), p.status))
            .collect();
        assert_eq!(
            panels,
            [
                ("basic", PanelStatus::Ready),
                ("nested", PanelStatus::Missing),
                ("basic", PanelStatus::Invalid)
            ]
        );
        assert_eq!(matched.unplaced, Vec::<String>::new());
    }

    #[test]
    fn exported_examples_the_markdown_never_places_are_reported_not_shown() {
        let exported: Vec<String> = ["basic", "nested", "with-icon", "large"]
            .map(String::from)
            .into();
        let matched = match_lens(&placed_twice().placements, &exported);
        assert_eq!(matched.panels.len(), 3);
        assert_eq!(matched.unplaced, ["with-icon", "large"]);
    }
}
