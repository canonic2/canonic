//! Safe bounded viewport projection and semantic code/unified-diff rendering.

use super::{Field, Inspector, Kind, Origin, State};
use crate::{
    editor::EditorView,
    text,
    theme::{Presentation, Symbols},
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn render(v: &mut Inspector, f: &mut Frame<'_>, area: Rect, p: Presentation) {
    let area = area.intersection(f.area());
    if area.is_empty() {
        return;
    }
    f.render_widget(Block::default().style(p.base()), area);
    title(v, f, area, p);
    if area.height < 3 {
        return;
    }
    let tiny = area.height <= 3;
    let bottom = if tiny {
        1
    } else {
        3 + u16::from(v.field.is_some())
    };
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1 + bottom).max(1),
    );
    v.page_rows = usize::from(body.height);
    if v.width != body.width || v.symbols != p.symbols {
        v.top_part = 0;
    }
    v.width = body.width;
    v.symbols = p.symbols;
    v.visible.clear();
    if tiny {
        v.top = v.cursor;
        v.top_part = 0;
    }
    match &v.state {
        State::Ready(_) => {
            if v.rows().is_empty() {
                f.render_widget(Paragraph::new("empty content").style(p.muted()), body);
            } else {
                if v.cursor < v.top {
                    v.top = v.cursor;
                    v.top_part = 0;
                }
                body_rows(v, f, body, p);
                if !v.visible.iter().any(|(row, _)| *row == v.cursor) {
                    v.top = v.cursor;
                    v.top_part = 0;
                    f.render_widget(Block::default().style(p.base()), body);
                    v.visible.clear();
                    body_rows(v, f, body, p);
                }
            }
        }
        State::Loading(reason) => message(f, body, &format!("loading · {reason}"), p.muted()),
        State::Failed(reason) => message(f, body, &format!("couldn't load · {reason}"), p.error()),
        State::Unavailable { reason, preview } => message(
            f,
            body,
            &format!(
                "{reason}{}",
                if preview.is_some() {
                    "\nenter show supplied part"
                } else {
                    ""
                }
            ),
            p.muted(),
        ),
    }
    footer(v, f, area, p, tiny);
}

fn title(v: &Inspector, f: &mut Frame<'_>, area: Rect, p: Presentation) {
    let title = if area.width < 50 {
        v.item.title.rsplit('/').next().unwrap_or(&v.item.title)
    } else {
        &v.item.title
    };
    let mut meta = match &v.state {
        State::Loading(_) => "loading".into(),
        State::Failed(_) => "couldn't load".into(),
        State::Unavailable { .. } => v.item.summary.clone(),
        State::Ready(_) => {
            if v.loaded.is_diff {
                format!(
                    "{} · {}",
                    v.item.summary,
                    if v.whole {
                        "whole file".into()
                    } else {
                        format!("{} hunks", v.loaded.hunks.len())
                    }
                )
            } else {
                format!("{} lines · {}", v.rows().len(), v.item.language)
            }
        }
    };
    if v.partial {
        meta.push_str(" · partial");
    }
    // Preserve the filename at tiny widths; metadata yields before orientation.
    let meta = if area.width < 30 {
        String::new()
    } else {
        text::elide(&text::clean(&meta, 4096, false), area.width / 2, p.symbols)
    };
    rule(
        f,
        Rect::new(area.x, area.y, area.width, 1),
        &text::elide(
            &text::clean(title, 4096, false),
            area.width
                .saturating_sub(u16::try_from(meta.width()).unwrap_or(0) + 1),
            p.symbols,
        ),
        &meta,
        p,
    );
}

fn footer(v: &mut Inspector, f: &mut Frame<'_>, area: Rect, p: Presentation, tiny: bool) {
    let keys = if tiny {
        String::new()
    } else {
        hints(v, area.width)
    };
    let back = if tiny {
        "esc"
    } else if v.field.is_some() {
        "esc cancel"
    } else if v.anchor.is_some() {
        "esc cancel selection"
    } else if !v.query.is_empty() {
        "esc clear find"
    } else if v.item.origin == Origin::Tree {
        "tab / esc to files"
    } else {
        "esc back to chat"
    };
    let keys = keys
        .split(" · esc")
        .next()
        .unwrap_or(&keys)
        .split(" · tab / esc")
        .next()
        .unwrap_or(&keys);
    f.render_widget(
        Rule {
            left: Line::from(chrome(keys, p)).style(p.muted()),
            right: Line::from(back).style(p.muted()),
            glyph: ' ',
            style: p.base(),
        },
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
    );
    if tiny {
        return;
    }
    if area.height >= 4 {
        let status = position(v);
        rule(
            f,
            Rect::new(area.x, area.bottom() - 3, area.width, 1),
            "",
            &text::elide(&status, area.width, p.symbols),
            p,
        );
        let notice = if v.pending.is_some() {
            "waiting for the host"
        } else {
            &v.notice
        };
        f.render_widget(
            Paragraph::new(text::elide(
                &text::clean(notice, 65536, false),
                area.width,
                p.symbols,
            ))
            .style(p.info()),
            Rect::new(area.x, area.bottom() - 2, area.width, 1),
        );
    }
    if v.field.is_some() && area.height >= 5 {
        let label = if v.field == Some(Field::Find) {
            format!(
                "find {} · ",
                if v.matches.is_empty() {
                    "no match".into()
                } else {
                    format!("{} matches", v.matches.len())
                }
            )
        } else {
            "go to line · ".into()
        };
        let field_y = area.bottom() - 4;
        let field_width = u16::try_from(label.width())
            .unwrap_or(u16::MAX)
            .min(area.width / 2);
        f.render_widget(
            Paragraph::new(label).style(p.info()),
            Rect::new(area.x, field_y, field_width, 1),
        );
        v.editor.render_with(
            f,
            Rect::new(area.x + field_width, field_y, area.width - field_width, 1),
            EditorView {
                presentation: p,
                chrome: false,
                ..EditorView::default()
            },
            v.focused,
        );
    }
}

fn rule(f: &mut Frame<'_>, area: Rect, left: &str, right: &str, p: Presentation) {
    f.render_widget(
        Rule {
            left: Line::from(left).style(p.heading()),
            right: Line::from(chrome(right, p)).style(p.muted()),
            glyph: if p.symbols == Symbols::Ascii {
                '-'
            } else {
                '─'
            },
            style: p.message_line(),
        },
        area,
    );
}
fn chrome(text: &str, p: Presentation) -> String {
    if p.symbols == Symbols::Ascii {
        text.replace('·', "|").replace("↑↓", "up/down")
    } else {
        text.into()
    }
}
fn message(f: &mut Frame<'_>, area: Rect, s: &str, style: Style) {
    f.render_widget(
        Paragraph::new(text::wrapped(&text::clean(s, 65536, true), area.width).join("\n"))
            .style(style),
        area,
    );
}
fn hints(v: &Inspector, width: u16) -> String {
    let back = if v.item.origin == Origin::Chat {
        "esc back to chat"
    } else {
        "tab / esc to files"
    };
    if v.field.is_some() {
        return "enter go · esc cancel".into();
    }
    match &v.state {
        State::Loading(_) => format!("{back} · cancels nothing"),
        State::Failed(_) => format!("r retry · {back}"),
        State::Unavailable { preview, .. } => format!(
            "{}{}{back}",
            if preview.is_some() {
                "enter show part · "
            } else {
                ""
            },
            if v.item.editor { "e editor · " } else { "" }
        ),
        State::Ready(_) => {
            if v.anchor.is_some() {
                format!("y copy patch/text · c new code · esc cancel · {back}")
            } else if width < 50 {
                format!(
                    "{} · {back}",
                    if v.loaded.is_diff && !v.whole {
                        "] [ hunk · / find"
                    } else {
                        "/ find · : line"
                    }
                )
            } else if !v.loaded.is_diff {
                format!("↑↓ line · / find · : line · v select · w wrap · {back}")
            } else {
                format!(
                    "↑↓ line · ] [ hunk · / find · : line · v select · w wrap · {}{back}",
                    if v.loaded.is_diff && v.item.origin == Origin::Chat {
                        "tab file · "
                    } else {
                        ""
                    }
                )
            }
        }
    }
}
fn position(v: &Inspector) -> String {
    if let Some(r) = v.selection() {
        return format!("{} lines selected", r.len());
    }
    let line = v.line().unwrap_or(0);
    let total = v
        .rows()
        .iter()
        .rev()
        .find_map(|r| r.new.or(r.old))
        .unwrap_or(0);
    let hunk = v
        .rows()
        .get(v.cursor)
        .and_then(|r| r.hunk)
        .map_or(String::new(), |h| {
            format!("hunk {} of {} · ", h + 1, v.loaded.hunks.len())
        });
    let find = if v.query.is_empty() {
        String::new()
    } else {
        format!(
            "match {} of {} · ",
            v.current_match.map_or(0, |i| i + 1),
            v.matches.len()
        )
    };
    format!(
        "{find}{hunk}ln {line} of {total} · {}%",
        line.saturating_mul(100).checked_div(total).unwrap_or(0)
    )
}
fn body_rows(v: &mut Inspector, f: &mut Frame<'_>, area: Rect, p: Presentation) {
    let indices = v.projection();
    let top = v.top;
    let mut y = area.y;
    for i in indices.into_iter().filter(|i| *i >= top) {
        if y >= area.bottom() {
            break;
        }
        if let Some((_, range)) = v.fold(i) {
            let label = format!(
                "{} {} unchanged lines · enter shows them",
                if p.symbols == Symbols::Ascii {
                    "..."
                } else {
                    "⋯"
                },
                range.len()
            );
            let style = if i == v.cursor && v.focused {
                p.selection()
            } else {
                p.muted()
            };
            f.render_widget(
                Paragraph::new(text::elide(&label, area.width, p.symbols)).style(style),
                Rect::new(area.x, y, area.width, 1),
            );
            v.visible.push((i, 0));
            y += 1;
            continue;
        }
        y = source_row(v, f, area, p, i, y);
    }
}
/// Stream glyphs into only the requested viewport chunks; source controls are visible data.
#[derive(Clone, Copy)]
struct ChunkView {
    width: u16,
    wrap: bool,
    horizontal: usize,
    skip: usize,
    height: usize,
    symbols: Symbols,
}
fn chunks(text: &str, view: ChunkView, style: impl Fn(usize) -> Style) -> Vec<Vec<Span<'static>>> {
    let ChunkView {
        width,
        wrap,
        horizontal,
        skip,
        height,
        symbols,
    } = view;
    let width = usize::from(width);
    let mut result = vec![];
    let mut line = vec![];
    let mut used = 0;
    let mut part = 0;
    let mut column = 0;
    for (byte, g) in text.grapheme_indices(true) {
        let s = if g == "\t" {
            " ".repeat(4 - column % 4)
        } else {
            g.chars()
                .map(|c| {
                    if c.is_control() {
                        format!("<{:02X}>", u32::from(c))
                    } else {
                        c.to_string()
                    }
                })
                .collect::<String>()
        };
        let cells = s.width().max(1);
        if wrap && used + cells > width && used > 0 {
            if part >= skip {
                result.push(std::mem::take(&mut line));
                if result.len() >= height {
                    return result;
                }
            }
            part += 1;
            used = 0;
        }
        if !wrap && column + cells <= horizontal {
            column += cells;
            continue;
        }
        if !wrap && used + cells > width {
            let marker = if symbols == Symbols::Ascii {
                ">"
            } else {
                "›"
            };
            if used == width {
                line.pop();
            }
            line.push(Span::styled(marker.to_owned(), style(byte)));
            break;
        }
        if part >= skip {
            line.push(Span::styled(
                if cells > width { "?".into() } else { s },
                style(byte),
            ));
        }
        used += cells.min(width);
        column += cells;
    }
    if part >= skip && result.len() < height {
        result.push(line);
    }
    if result.is_empty() {
        result.push(vec![Span::raw("end of line")]);
    }
    result
}

fn source_row(
    v: &mut Inspector,
    f: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
    index: usize,
    mut y: u16,
) -> u16 {
    let row = &v.rows()[index];
    let narrow = area.width < 50;
    let numbers = !v.loaded.is_diff || v.whole || !narrow;
    let selected = v.selection().is_some_and(|r| r.contains(&index));
    let band = if index == v.cursor && v.focused {
        p.selection()
    } else {
        p.base()
    };
    let gutter = gutter(v, row, narrow, numbers, selected, p);
    let gutter = text::elide(&gutter, area.width.saturating_sub(1), p.symbols);
    let cells = u16::try_from(gutter.width()).unwrap_or(0);
    let width = area.width.saturating_sub(cells).max(1);
    let text = if row.kind == Kind::Header && narrow {
        row.text
            .split(" @@")
            .nth(1)
            .map_or_else(|| "@@ @@".into(), |name| format!("@@{name} @@"))
    } else {
        row.text.clone()
    };
    let skip = if index == v.top { v.top_part } else { 0 };
    let matches: Vec<_> = v
        .matches
        .iter()
        .filter(|(r, _)| *r == index)
        .map(|(_, range)| range)
        .collect();
    let spans = chunks(
        &text,
        ChunkView {
            width,
            wrap: v.wrap || narrow,
            horizontal: if v.wrap || narrow { 0 } else { v.horizontal },
            skip,
            height: usize::from(area.bottom() - y),
            symbols: p.symbols,
        },
        |byte| row_style(row, p, byte, band, &matches),
    );
    for (part, line) in spans.into_iter().enumerate() {
        let gutter_text = if part + skip == 0 {
            gutter.clone()
        } else {
            format!(
                "{}{}",
                " ".repeat(usize::from(cells.saturating_sub(1))),
                if p.symbols == Symbols::Ascii {
                    ">"
                } else {
                    "↩"
                }
            )
        };
        f.render_widget(
            Paragraph::new(gutter_text).style(band.patch(p.muted())),
            Rect::new(area.x, y, cells, 1),
        );
        f.render_widget(
            Paragraph::new(Line::from(line)).style(band),
            Rect::new(area.x + cells, y, width, 1),
        );
        v.visible.push((index, part + skip));
        y += 1;
    }
    y
}

fn row_style(
    row: &super::Row,
    p: Presentation,
    byte: usize,
    band: Style,
    matches: &[&std::ops::Range<usize>],
) -> Style {
    let mut style = match row.kind {
        Kind::Added => p.diff_line(true),
        Kind::Removed => p.diff_line(false),
        Kind::Header => p.info(),
        _ => p.base(),
    };
    if row.changed.contains(&byte) {
        style = style.patch(p.diff_word(row.kind == Kind::Added));
    }
    let next = matches.partition_point(|range| range.end <= byte);
    if matches.get(next).is_some_and(|range| range.contains(&byte)) {
        style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
    }
    style.patch(band)
}

fn gutter(
    v: &Inspector,
    row: &super::Row,
    narrow: bool,
    numbers: bool,
    selected: bool,
    p: Presentation,
) -> String {
    let sign = match row.kind {
        Kind::Added => "+",
        Kind::Removed => {
            if p.symbols == Symbols::Ascii {
                "-"
            } else {
                "−"
            }
        }
        _ => " ",
    };
    let bar = if selected {
        if p.symbols == Symbols::Ascii {
            "|"
        } else {
            "▌"
        }
    } else {
        " "
    };
    let largest = v
        .rows()
        .iter()
        .filter_map(|r| r.new.or(r.old))
        .max()
        .unwrap_or(1);
    let digits = usize::try_from(largest.ilog10() + 1)
        .unwrap_or(20)
        .max(if narrow { 2 } else { 3 });
    let number =
        |n: Option<usize>| n.map_or_else(|| " ".repeat(digits), |n| format!("{n:>digits$}"));
    if matches!(row.kind, Kind::Header | Kind::Meta) {
        String::new()
    } else if numbers {
        if v.loaded.is_diff && !v.whole {
            format!("{bar}{} {} {sign} ", number(row.old), number(row.new))
        } else {
            format!("{bar}{} {sign} ", number(row.new))
        }
    } else {
        format!("{sign}{bar}")
    }
}
pub(super) fn previous_page(v: &Inspector, mut steps: usize) -> (usize, usize) {
    let indices = v.projection();
    let mut index = indices.iter().position(|i| *i == v.cursor).unwrap_or(0);
    let mut part = if v.top == v.cursor { v.top_part } else { 0 };
    while steps > 0 {
        if steps <= part {
            part -= steps;
            break;
        }
        steps -= part;
        if index == 0 {
            part = 0;
            break;
        }
        index -= 1;
        steps -= 1;
        part = visual_parts(v, indices[index]).saturating_sub(1);
    }
    (indices.get(index).copied().unwrap_or(0), part)
}
fn visual_parts(v: &Inspector, index: usize) -> usize {
    if !(v.wrap || v.width < 50) || v.fold(index).is_some() {
        return 1;
    }
    let row = &v.rows()[index];
    let p = Presentation {
        symbols: v.symbols,
        ..Presentation::default()
    };
    let prefix = gutter(
        v,
        row,
        v.width < 50,
        !v.loaded.is_diff || v.whole || v.width >= 50,
        false,
        p,
    );
    let width = usize::from(v.width).saturating_sub(prefix.width()).max(1);
    let mut parts = 1;
    let mut used = 0;
    let mut column = 0;
    for g in row.text.graphemes(true) {
        let cells = if g == "\t" {
            4 - column % 4
        } else {
            g.chars()
                .map(|c| {
                    if c.is_control() {
                        format!("<{:02X}>", u32::from(c))
                    } else {
                        c.to_string()
                    }
                })
                .collect::<String>()
                .width()
                .max(1)
        };
        if used + cells > width && used > 0 {
            parts += 1;
            used = 0;
        }
        used += cells.min(width);
        column += cells;
    }
    parts
}
