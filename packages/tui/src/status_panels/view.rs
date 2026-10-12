//! Semantic section rules, bounded wrapping, notice attention and compact host summaries.

use super::{Glyph, Kind, Mode, Severity, State, StatusPanels, Target};
use crate::{
    text::{clean, elide, wrapped},
    theme::{Presentation, Symbols},
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
};
use std::fmt::Write;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn style(p: Presentation, severity: Severity) -> Style {
    match severity {
        Severity::Error => p.error().add_modifier(Modifier::BOLD),
        Severity::Warning => p.warning(),
        Severity::Success => p
            .tokens()
            .success
            .map_or_else(Style::default, |c| Style::default().fg(c)),
        Severity::Info => p.muted(),
    }
}
fn glyph(g: Glyph, ascii: bool) -> &'static str {
    match (g, ascii) {
        (Glyph::Done, true) => "ok",
        (Glyph::Done, false) => "✓",
        (Glyph::Working, true) => "...",
        (Glyph::Working, false) => "···",
        (Glyph::Queued | Glyph::Transition, true) => "o",
        (Glyph::Queued | Glyph::Transition, false) => "○",
        (Glyph::Connected, true) => "*",
        (Glyph::Connected, false) => "●",
        (Glyph::Failed, true) => "x",
        (Glyph::Failed, false) => "✕",
        (Glyph::Modified, _) => "M",
        (Glyph::Added, _) => "A",
        (Glyph::Deleted, _) => "D",
        _ => "",
    }
}
fn severity_glyph(s: Severity, ascii: bool) -> &'static str {
    match s {
        Severity::Info => "i",
        Severity::Warning => "!",
        Severity::Success => {
            if ascii {
                "ok"
            } else {
                "✓"
            }
        }
        Severity::Error => {
            if ascii {
                "x"
            } else {
                "✕"
            }
        }
    }
}
fn safe(s: &str, p: Presentation) -> String {
    let mut display = String::new();
    for c in s.chars() {
        if c.is_control() && c != '\n' {
            let _ = write!(display, "<{:02X}>", u32::from(c));
        } else {
            display.push(c);
        }
    }
    let s = clean(&display, 65536 * 12, true);
    if p.symbols == Symbols::Ascii {
        s.replace('·', "|")
            .replace("↑↓", "up/down")
            .replace("←→", "left/right")
    } else {
        s
    }
}
struct Visual {
    target: Target,
    text: String,
    header: Option<String>,
    tone: Severity,
}
fn section_rows(panels: &StatusPanels, compact: bool, p: Presentation) -> Vec<Visual> {
    let mut output = vec![];
    let ascii = p.symbols == Symbols::Ascii;
    for section in &panels.snapshot.sections {
        let target = Target::Section {
            id: section.id.clone(),
            row: None,
        };
        let closed = compact || panels.folded.contains(&section.id);
        let mark = if closed {
            if ascii { ">" } else { "▸" }
        } else if ascii {
            "v"
        } else {
            "▾"
        };
        let summary = match section.state {
            State::Unknown => if ascii { "?" } else { "—" }.into(),
            State::Loading => if ascii { "..." } else { "···" }.into(),
            State::Empty => "0".into(),
            State::Error => if ascii { "x" } else { "✕" }.into(),
            State::Ready => section.summary.clone(),
        };
        let tone = if section.state == State::Error {
            Severity::Error
        } else if section.warning {
            Severity::Warning
        } else {
            Severity::Info
        };
        output.push(Visual {
            target: target.clone(),
            text: format!("{mark} {}", section.title),
            header: Some(summary),
            tone,
        });
        if closed {
            continue;
        }
        if let Some(n) = section.bar {
            output.push(Visual {
                target: target.clone(),
                text: format!(
                    "  {}{}",
                    if ascii { "#" } else { "█" }.repeat(usize::from(n)),
                    if ascii { "-" } else { "░" }.repeat(usize::from(10 - n))
                ),
                header: None,
                tone,
            });
        }
        if !section.detail.is_empty() {
            output.push(Visual {
                target: target.clone(),
                text: format!("  {}", section.detail),
                header: None,
                tone,
            });
        }
        if section.state == State::Ready {
            append_ready_rows(panels, section, ascii, &mut output);
        }
        if let Some(reason) = panels.failures.get(&target) {
            output.push(Visual {
                target: target.clone(),
                text: format!("  couldn't act · {reason}"),
                header: None,
                tone: Severity::Error,
            });
        }
        if panels.pending.contains_key(&target) {
            output.push(Visual {
                target: target.clone(),
                text: "  waiting for host".into(),
                header: None,
                tone,
            });
        }
        if section.state == State::Error && section.retry.is_some() {
            output.push(Visual {
                target,
                text: "  r retry".into(),
                header: None,
                tone,
            });
        }
    }
    output
}
struct Projection {
    row: Visual,
    height: usize,
}

pub(super) fn sections(
    panels: &mut StatusPanels,
    frame: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
    compact: bool,
) {
    let area = area.intersection(frame.area());
    if area.is_empty() {
        return;
    }
    frame.render_widget(Block::default().style(p.base()), area);
    let focused = panels.mode == Mode::Status;
    let height = usize::from(area.height.saturating_sub(1));
    panels.viewport = height.max(1);
    let mut rows: Vec<_> = section_rows(panels, compact, p)
        .into_iter()
        .map(|mut row| {
            row.text = safe(&row.text, p);
            row.header = row.header.map(|s| safe(&s, p));
            let height = if row.header.is_some() {
                1
            } else {
                project(&row.text, area.width, None).0
            };
            Projection { row, height }
        })
        .collect();
    if !compact {
        preserve_loading_height(panels, &mut rows);
    }
    let index = rows
        .iter()
        .position(|r| Some(&r.row.target) == panels.cursor.as_ref())
        .unwrap_or(0);
    let selected: usize = rows.iter().take(index).map(|r| r.height).sum();
    if focused {
        let count: usize = rows
            .iter()
            .skip(index)
            .take_while(|r| Some(&r.row.target) == panels.cursor.as_ref())
            .map(|r| r.height)
            .sum();
        panels.details_offset = panels.details_offset.min(count.saturating_sub(1));
        let selected = selected.saturating_add(panels.details_offset);
        panels.offset = panels.offset.min(selected);
        if selected >= panels.offset + height.max(1) {
            panels.offset = selected + 1 - height.max(1);
        }
    }
    let total: usize = rows.iter().map(|r| r.height).sum();
    panels.offset = panels.offset.min(total.saturating_sub(height));
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("no status sections").style(p.muted()), area);
    }
    render_projection(panels, frame, area, p, &rows, height);
    section_footer(panels, frame, area, p, focused);
}

fn render_projection(
    panels: &StatusPanels,
    frame: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
    rows: &[Projection],
    height: usize,
) {
    let mut start = 0usize;
    for projection in rows {
        let end = start.saturating_add(projection.height);
        if end <= panels.offset {
            start = end;
            continue;
        }
        if start >= panels.offset + height {
            break;
        }
        let row = &projection.row;
        let ink = if panels.mode == Mode::Status && Some(&row.target) == panels.cursor.as_ref() {
            p.selection()
        } else {
            style(p, row.tone)
        };
        if let Some(summary) = &row.header {
            let y = area.y.saturating_add(
                u16::try_from(start.saturating_sub(panels.offset)).unwrap_or(u16::MAX),
            );
            frame.render_widget(
                Rule {
                    left: Line::from(elide(&row.text, area.width.saturating_sub(8), p.symbols))
                        .style(ink),
                    right: Line::from(elide(summary, area.width / 2, p.symbols)).style(ink),
                    glyph: p.symbols.rule(),
                    style: p.message_line(),
                },
                Rect::new(area.x, y, area.width, 1),
            );
        } else {
            let first = panels.offset.saturating_sub(start);
            let last = (panels.offset + height)
                .saturating_sub(start)
                .min(projection.height);
            for (i, text) in project(&row.text, area.width, Some((first, last)))
                .1
                .into_iter()
                .enumerate()
            {
                let y = start + first + i - panels.offset;
                frame.render_widget(
                    Paragraph::new(text).style(ink),
                    Rect::new(
                        area.x,
                        area.y.saturating_add(u16::try_from(y).unwrap_or(u16::MAX)),
                        area.width,
                        1,
                    ),
                );
            }
        }
        start = end;
    }
}

// Count all wrapped rows while allocating text only inside the requested viewport.
fn project(text: &str, width: u16, window: Option<(usize, usize)>) -> (usize, Vec<String>) {
    let width = usize::from(width).max(1);
    let mut index = 0;
    let mut output = vec![];
    let visible = |index| window.is_some_and(|(first, last)| first <= index && index < last);
    for line in text.split('\n') {
        let mut row = String::new();
        let mut used = 0;
        let mut occupied = false;
        for g in line.graphemes(true) {
            let cells = g.width();
            if used + cells > width && occupied {
                if visible(index) {
                    output.push(std::mem::take(&mut row));
                }
                index += 1;
                used = 0;
            }
            if cells > width {
                if visible(index) {
                    row.push('\u{fffd}');
                }
                used += 1;
            } else {
                if visible(index) {
                    row.push_str(g);
                }
                used += cells;
            }
            occupied = true;
        }
        if visible(index) {
            output.push(row);
        }
        index += 1;
    }
    (index, output)
}

pub(super) fn notices(
    panels: &mut StatusPanels,
    frame: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
) {
    let area = area.intersection(frame.area());
    if area.is_empty() {
        return;
    }
    frame.render_widget(Block::default().style(p.base()), area);
    let notices = panels.notices();
    let selected = notices
        .iter()
        .position(|n| Some(&n.id) == panels.notice_cursor.as_ref())
        .unwrap_or(0);
    let detail_height = area.height.saturating_sub(2).min(5);
    let height = usize::from(area.height.saturating_sub(detail_height + 1));
    let mut offset = panels.offset.min(selected);
    if selected >= offset + height.max(1) {
        offset = selected + 1 - height.max(1);
    }
    for (i, n) in notices.iter().skip(offset).take(height).enumerate() {
        let line = format!(
            "{} {} {}{}",
            severity_glyph(panels.notice_severity(n), p.symbols == Symbols::Ascii),
            n.label,
            n.timestamp,
            if panels.aside.contains(&n.id) {
                " · set aside"
            } else {
                ""
            }
        );
        let ink = if Some(&n.id) == panels.notice_cursor.as_ref() {
            p.selection()
        } else {
            style(p, panels.notice_severity(n))
        };
        frame.render_widget(
            Paragraph::new(elide(&safe(&line, p), area.width, p.symbols)).style(ink),
            Rect::new(
                area.x,
                area.y + u16::try_from(i).unwrap_or(0),
                area.width,
                1,
            ),
        );
    }
    if notices.is_empty() {
        frame.render_widget(Paragraph::new("no notices").style(p.muted()), area);
    }
    if detail_height > 0
        && let Some(n) = notices.get(selected)
    {
        notice_detail(panels, n, frame, area, p, detail_height);
    }
    let hint = if area.width < 50 {
        notices
            .get(selected)
            .and_then(|n| n.action.as_ref())
            .map_or_else(
                || "d · alt+pg".into(),
                |a| format!("enter {} · d · alt+pg", a.label),
            )
    } else if let Some(n) = notices.get(selected) {
        format!(
            "{}d dismiss · alt+pg details",
            n.action
                .as_ref()
                .map_or(String::new(), |a| format!("enter {} · ", a.label))
        )
    } else {
        "esc back".into()
    };
    frame.render_widget(
        Rule {
            left: Line::from(elide(
                &safe(&hint, p),
                area.width.saturating_sub(9),
                p.symbols,
            ))
            .style(p.muted()),
            right: Line::from("esc back").style(p.muted()),
            glyph: ' ',
            style: p.base(),
        },
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
    );
    panels.offset = offset;
    panels.viewport = height.max(1);
}
pub(super) fn summary(panels: &StatusPanels, width: u16) -> String {
    let context = panels
        .snapshot
        .sections
        .iter()
        .find(|s| s.kind == Kind::Context)
        .map_or("?", |s| {
            if s.state == State::Ready {
                s.summary.as_str()
            } else {
                "?"
            }
        });
    let badge = format!("! {}", panels.unresolved());
    if width < 50 {
        format!("{badge} {context}")
    } else {
        let cost = panels
            .snapshot
            .sections
            .iter()
            .find(|s| s.kind == Kind::Cost)
            .map_or("not reported", |s| {
                if s.state == State::Ready {
                    s.summary.as_str()
                } else {
                    "not reported"
                }
            });
        format!("ctx {context} · {cost} · {badge}")
    }
}
pub(super) fn activity(panels: &StatusPanels, p: Presentation) -> Option<(String, Severity)> {
    let notice = panels.current_notice()?;
    let target = Target::Notice(notice.id.clone());
    let reason = panels.failures.get(&target);
    let pending = panels.pending.contains_key(&target);
    let label = reason.map_or_else(|| notice.label.clone(), |s| format!("action failed · {s}"));
    let action = notice.action.as_ref().map_or(String::new(), |a| {
        if pending {
            format!(" · {} · waiting for host", a.pending)
        } else {
            format!(" · ctrl+g {}", a.label)
        }
    });
    let remaining = if notice.severity < Severity::Warning && reason.is_none() && !pending {
        notice.expires.map_or(String::new(), |due| {
            format!(" · {}s", due.saturating_sub(panels.now).as_secs())
        })
    } else {
        String::new()
    };
    let more = panels
        .notices()
        .iter()
        .filter(|n| !panels.aside.contains(&n.id))
        .count()
        .saturating_sub(1);
    let more = if more > 0 {
        format!(" · +{more} more · ctrl+x n")
    } else {
        String::new()
    };
    Some((
        safe(
            &format!(
                "{} {label}{action}{remaining}{more}",
                severity_glyph(panels.notice_severity(notice), p.symbols == Symbols::Ascii)
            ),
            p,
        ),
        if reason.is_some() {
            Severity::Error
        } else {
            notice.severity
        },
    ))
}

fn preserve_loading_height(panels: &mut StatusPanels, rows: &mut Vec<Projection>) {
    for section in &panels.snapshot.sections {
        if panels.folded.contains(&section.id) {
            continue;
        }
        let start = rows
            .iter()
            .position(|r| matches!(&r.row.target, Target::Section { id, .. } if id == &section.id));
        let Some(start) = start else {
            continue;
        };
        let count = rows
            .iter()
            .skip(start)
            .take_while(
                |r| matches!(&r.row.target, Target::Section { id, .. } if id == &section.id),
            )
            .map(|r| r.height)
            .sum::<usize>();
        let boundary = start
            + rows
                .iter()
                .skip(start)
                .take_while(
                    |r| matches!(&r.row.target, Target::Section { id, .. } if id == &section.id),
                )
                .count();
        if section.state == State::Loading {
            let retained = panels.heights.get(&section.id).copied().unwrap_or(count);
            for _ in count..retained {
                rows.insert(
                    boundary,
                    Projection {
                        height: 1,
                        row: Visual {
                            target: Target::Section {
                                id: section.id.clone(),
                                row: None,
                            },
                            text: String::new(),
                            header: None,
                            tone: Severity::Info,
                        },
                    },
                );
            }
        } else {
            panels.heights.insert(section.id.clone(), count.min(64));
        }
    }
}

fn append_ready_rows(
    panels: &StatusPanels,
    section: &super::Section,
    ascii: bool,
    output: &mut Vec<Visual>,
) {
    for row in &section.rows {
        let target = Target::Section {
            id: section.id.clone(),
            row: Some(row.id.clone()),
        };
        let failure = panels
            .failures
            .get(&target)
            .map_or(String::new(), |s| format!(" · couldn't act · {s}"));
        let pending = if panels.pending.contains_key(&target) {
            " · waiting for host"
        } else {
            ""
        };
        output.push(Visual {
            target,
            text: format!(
                "  {} {} {}{pending}{failure}",
                glyph(row.glyph, ascii),
                row.label,
                row.detail
            ),
            header: None,
            tone: if row.glyph == Glyph::Failed {
                Severity::Error
            } else {
                Severity::Info
            },
        });
    }
}

fn section_footer(
    panels: &StatusPanels,
    frame: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
    focused: bool,
) {
    let action = panels
        .cursor
        .as_ref()
        .and_then(|t| panels.offered(t, false))
        .map_or(String::new(), |a| format!("enter {} · ", a.label));
    let hint = if focused {
        let retry = panels
            .cursor
            .as_ref()
            .and_then(|t| panels.offered(t, true))
            .map_or("", |_| "r retry · ");
        if area.width < 50 {
            if !action.is_empty() {
                format!("{action}alt+pg")
            } else if !retry.is_empty() {
                format!("{retry}alt+pg")
            } else {
                "<> fold · alt+pg".into()
            }
        } else {
            format!("↑↓ · ←→ fold · {action}{retry}alt+pg details")
        }
    } else {
        format!(
            "! {} · ctrl+x i status · ctrl+x n notices",
            panels.unresolved()
        )
    };
    frame.render_widget(
        Rule {
            left: Line::from(elide(
                &safe(&hint, p),
                area.width.saturating_sub(9),
                p.symbols,
            ))
            .style(p.muted()),
            right: Line::from(if focused { "esc back" } else { "" }).style(p.muted()),
            glyph: ' ',
            style: p.base(),
        },
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
    );
}

fn notice_detail(
    panels: &StatusPanels,
    n: &super::Notice,
    frame: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
    detail_height: u16,
) {
    let failure = panels
        .failures
        .get(&Target::Notice(n.id.clone()))
        .map_or("", String::as_str);
    let pending = if panels.pending.contains_key(&Target::Notice(n.id.clone())) {
        "waiting for host · "
    } else {
        ""
    };
    let disabled = n
        .action
        .as_ref()
        .and_then(|a| a.disabled.as_deref())
        .unwrap_or("");
    let lines = wrapped(
        &safe(&format!("{pending}{} {failure} {disabled}", n.detail), p),
        area.width,
    );
    let offset = panels
        .details_offset
        .min(lines.len().saturating_sub(usize::from(detail_height)));
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(offset)
                .take(usize::from(detail_height))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .style(p.muted()),
        Rect::new(
            area.x,
            area.bottom() - detail_height - 1,
            area.width,
            detail_height,
        ),
    );
}
