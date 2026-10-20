//! The source text of one named export, for Show code on a single-file
//! example source. A whole-file example shows its file; a named export shows
//! only its own statement, with the comment and decorators directly above it.
//!
//! The statement is found by layout, not by parsing: it starts at its
//! column-0 `export` (or, for `export { local as name }`, the column-0
//! declaration of `local`) and ends before the next column-0 line that starts
//! a new statement. Formatted source keeps every top-level statement at
//! column 0 and everything inside one indented or closing (`}`, `)`, `]`), so
//! this holds without a TypeScript and JSX parser, which a lexer can't safely
//! approximate: an apostrophe in JSX text is not a string quote. A
//! multi-line template literal whose lines start at column 0 can end a
//! statement early.

use regex::Regex;
use std::sync::LazyLock;

static EXPORT_LIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^export\s*(?:type\s*)?\{").expect("export list"));
static BRACES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([^}]*)\}\s*(from\b)?").expect("braces"));
static SPECIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:type\s+)?([\w$]+)(?:\s+as\s+([\w$]+))?\s*$").expect("specifier")
});
static LEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(//|/\*|\*|@)").expect("leading"));

/// The statement that declares export `name`, or `None` when there isn't one.
pub(crate) fn export_source(source: &str, name: &str) -> Option<String> {
    let text = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let (line, reexport) = find_declaration(&lines, name)?;
    if reexport {
        return Some(lines[line].to_owned());
    }
    let first = leading_start(&lines, line);
    let last = statement_end(&lines, line);
    Some(lines[first..=last].join("\n"))
}

fn declaration(prefix: &str, name: &str) -> Regex {
    let id = regex::escape(name);
    Regex::new(&format!(
        r"^{prefix}(?:(?:async\s+)?function\*?\s+{id}\b|(?:const|let|var)\s+{id}\b|(?:abstract\s+)?class\s+{id}\b)"
    ))
    .expect("declaration pattern")
}

fn find_declaration(lines: &[&str], name: &str) -> Option<(usize, bool)> {
    let direct = declaration(r"export\s+(?:declare\s+)?", name);
    if let Some(line) = lines.iter().position(|line| direct.is_match(line)) {
        return Some((line, false));
    }
    let (line, local, from) = export_list(lines, name)?;
    if from {
        return Some((line, true));
    }
    let local = declaration("", &local);
    lines
        .iter()
        .position(|line| local.is_match(line))
        .map(|line| (line, false))
}

/// `export { local as name }` or `export { name } from '…'`, which may span lines.
fn export_list(lines: &[&str], name: &str) -> Option<(usize, String, bool)> {
    for (index, first) in lines.iter().enumerate() {
        if !EXPORT_LIST.is_match(first) {
            continue;
        }
        let mut text = (*first).to_owned();
        let mut end = index;
        while !text.contains('}') && end + 1 < lines.len() {
            end += 1;
            text.push('\n');
            text.push_str(lines[end]);
        }
        let Some(braces) = BRACES.captures(&text) else {
            continue;
        };
        for part in braces[1].split(',') {
            if let Some(found) = SPECIFIER.captures(part) {
                let exported = found
                    .get(2)
                    .unwrap_or_else(|| found.get(1).expect("local name"));
                if exported.as_str() == name {
                    return Some((index, found[1].to_owned(), braces.get(2).is_some()));
                }
            }
        }
    }
    None
}

/// The first line of the comment block and decorators directly above a statement.
fn leading_start(lines: &[&str], line: usize) -> usize {
    let mut first = line;
    while first > 0 {
        let above = lines[first - 1];
        if LEADING.is_match(above) && !above.trim().is_empty() {
            first -= 1;
        } else {
            break;
        }
    }
    first
}

fn statement_end(lines: &[&str], line: usize) -> usize {
    let mut last = line;
    for (index, text) in lines.iter().enumerate().skip(line + 1) {
        if text.trim().is_empty() {
            continue;
        }
        if text.starts_with(char::is_whitespace) || text.starts_with(['}', ']', ')']) {
            last = index;
            continue;
        }
        break;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "import { Card } from './card.tsx';\n\n/** The surface alone. */\nexport const basic = () => <Card>This is a card</Card>;\n\nexport const withNestedContent = () => (\n  <Card>\n    <h4>The card's title</h4>\n\n    <p>More complex children.</p>\n  </Card>\n);\n\n// Belongs to the next export.\nexport function WithCustomStyle() {\n  const style = { borderColor: '#0b72ff' };\n  return <Card style={style}>Custom</Card>;\n}\n\nfunction large() {\n  return <Card>Large</Card>;\n}\n\nexport { large as sizeLarge };\nexport { small } from './sizes.tsx';";

    #[test]
    fn a_single_line_export_comes_with_the_comment_above_it() {
        assert_eq!(
            export_source(SOURCE, "basic").as_deref(),
            Some(
                "/** The surface alone. */\nexport const basic = () => <Card>This is a card</Card>;"
            )
        );
    }

    #[test]
    fn a_multi_line_export_ends_at_its_closing_line_through_blank_lines_and_apostrophes_in_jsx() {
        assert_eq!(
            export_source(SOURCE, "withNestedContent").as_deref(),
            Some(
                "export const withNestedContent = () => (\n  <Card>\n    <h4>The card's title</h4>\n\n    <p>More complex children.</p>\n  </Card>\n);"
            )
        );
    }

    #[test]
    fn a_function_export_keeps_its_own_leading_comment_not_the_previous_statement() {
        assert_eq!(
            export_source(SOURCE, "WithCustomStyle").as_deref(),
            Some(
                "// Belongs to the next export.\nexport function WithCustomStyle() {\n  const style = { borderColor: '#0b72ff' };\n  return <Card style={style}>Custom</Card>;\n}"
            )
        );
    }

    #[test]
    fn an_export_list_points_at_the_local_declaration_and_a_reexport_shows_its_line() {
        assert_eq!(
            export_source(SOURCE, "sizeLarge").as_deref(),
            Some("function large() {\n  return <Card>Large</Card>;\n}")
        );
        assert_eq!(
            export_source(SOURCE, "small").as_deref(),
            Some("export { small } from './sizes.tsx';")
        );
        assert_eq!(export_source(SOURCE, "missing"), None);
        assert_eq!(
            export_source("export const $money = 1;", "$money").as_deref(),
            Some("export const $money = 1;")
        );
    }
}
