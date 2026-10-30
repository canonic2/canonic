//! Projects turn chrome and retained part rows without rewrapping neighbors.

use super::{
    Anchor, Chat, Record,
    content::{Ink, Run},
    layout::{Located, Row},
    model::{End, Role},
};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

fn add(rows: &mut Vec<Located>, row: Row) {
    rows.push(Located {
        rows: Arc::new(vec![row]),
        index: 0,
    });
}
fn quiet(anchor: Anchor, text: String) -> Row {
    Row::plain(
        anchor,
        text,
        Ink {
            muted: true,
            ..Ink::default()
        },
    )
}
pub(super) fn rows(chat: &Chat) -> Vec<Located> {
    let mut rows = Vec::new();
    let mut last_day = String::new();
    for record in &chat.messages {
        let anchor = Anchor {
            message: record.turn.id.clone(),
            part: String::new(),
            offset: 0,
        };
        if !record.turn.day.is_empty() && record.turn.day != last_day {
            last_day.clone_from(&record.turn.day);
            let gap = usize::from(chat.width).saturating_sub(last_day.width() + 2) / 2;
            add(
                &mut rows,
                quiet(
                    anchor.clone(),
                    format!("{} {last_day} {}", "─".repeat(gap), "─".repeat(gap)),
                ),
            );
        }
        if chat.loaded_boundary.as_deref() == Some(&record.turn.id) {
            add(
                &mut rows,
                quiet(anchor.clone(), "  ── earlier messages loaded ──".into()),
            );
        }
        if record.turn.role != Role::Note {
            add(&mut rows, header(record, anchor.clone(), chat.timestamps));
        }
        if record.turn.role == Role::Note {
            // Notes have one quiet line, while their original source remains copyable.
            if let Some((source, text)) = record.turn.parts.iter().find_map(|part| {
                chat.cache
                    .get(&(record.turn.id.clone(), part.id.clone()))
                    .and_then(|cache| cache.rows.first().zip(cache.blocks.first()))
                    .map(|(row, block)| (row, block.markdown.lines().next().unwrap_or_default()))
            }) {
                let mut row = source.clone();
                row.prefix.clear();
                row.surface = false;
                row.map = None;
                row.runs = vec![Run {
                    text: text.into(),
                    generated: false,
                    ink: Ink {
                        muted: true,
                        ..Ink::default()
                    },
                }];
                add(&mut rows, row);
            }
        } else {
            parts(chat, record, &mut rows, &anchor);
        }
        if let Some((_, Some(reason))) = &record.delivery {
            add(
                &mut rows,
                Row::plain(
                    anchor.clone(),
                    format!("  ✕ {reason}"),
                    Ink {
                        error: true,
                        ..Ink::default()
                    },
                ),
            );
        }
        if record.turn.role == Role::Assistant
            && let Some(row) = ending(record, anchor.clone(), chat.retry)
        {
            add(&mut rows, row);
        }
        if record.reloaded {
            add(
                &mut rows,
                quiet(
                    anchor.clone(),
                    "  ↻ reloaded this reply · updates arrived out of order".into(),
                ),
            );
        }
        add(&mut rows, Row::plain(anchor, String::new(), Ink::default()));
    }
    rows
}
fn header(record: &Record, anchor: Anchor, timestamps: bool) -> Row {
    let mut row = Row::plain(
        anchor,
        record.turn.label.clone(),
        Ink {
            bold: true,
            accent: record.turn.role == Role::Assistant,
            ..Ink::default()
        },
    );
    row.header = true;
    let mut suffix = |text| {
        row.runs.push(Run {
            generated: true,
            text,
            ink: Ink {
                muted: true,
                ..Ink::default()
            },
        });
    };
    if timestamps && !record.turn.timestamp.is_empty() {
        suffix(format!(" · {}", record.turn.timestamp));
    }
    let duration = if !record.turn.duration.is_empty() {
        record.turn.duration.as_str()
    } else if let End::Done(duration) = &record.turn.end {
        duration
    } else {
        ""
    };
    if !matches!(record.turn.end, End::Live) && !duration.is_empty() {
        suffix(format!(" · {duration}"));
    }
    if let Some((_, reason)) = &record.delivery {
        row.runs.push(Run {
            generated: true,
            text: format!(
                " · {}",
                if reason.is_some() {
                    "not sent"
                } else {
                    "sending"
                }
            ),
            ink: Ink {
                muted: reason.is_none(),
                error: reason.is_some(),
                ..Ink::default()
            },
        });
    }
    row
}
fn parts(chat: &Chat, record: &Record, rows: &mut Vec<Located>, anchor: &Anchor) {
    for (index, part) in record.turn.parts.iter().enumerate() {
        if index > 0
            && !(matches!(
                part.content,
                super::Content::Tool { .. }
                    | super::Content::ToolCard(_)
                    | super::Content::ToolGroup(_)
            ) && matches!(
                record.turn.parts[index - 1].content,
                super::Content::Tool { .. }
                    | super::Content::ToolCard(_)
                    | super::Content::ToolGroup(_)
            ))
        {
            add(
                rows,
                quiet(
                    Anchor {
                        part: part.id.clone(),
                        ..anchor.clone()
                    },
                    String::new(),
                ),
            );
        }
        if let Some(cache) = chat.cache.get(&(record.turn.id.clone(), part.id.clone())) {
            if cache.rows.is_empty() && !cache.blocks.is_empty() {
                add(
                    rows,
                    quiet(
                        Anchor {
                            part: part.id.clone(),
                            ..anchor.clone()
                        },
                        "  ⋯ loaded part not drawn · layout budget".into(),
                    ),
                );
            }
            let mut part_rows: Vec<_> = (0..cache.rows.len())
                .map(|index| Located {
                    rows: cache.rows.clone(),
                    index,
                })
                .collect();
            // The live marker sits at the last part's text edge without changing cached source.
            if matches!(record.turn.end, End::Live)
                && !matches!(
                    part.content,
                    super::Content::ToolCard(_) | super::Content::ToolGroup(_)
                )
                && record.turn.role == Role::Assistant
                && record.turn.parts.last().is_some_and(|p| p.id == part.id)
                && let Some(last) = part_rows.last_mut()
            {
                let mut row = last.row().clone();
                row.runs.push(Run {
                    generated: true,
                    text: "▌".into(),
                    ink: Ink {
                        accent: true,
                        ..Ink::default()
                    },
                });
                *last = Located {
                    rows: Arc::new(vec![row]),
                    index: 0,
                };
            }
            rows.extend(part_rows);
            if cache.memory_limited && !cache.rows.is_empty() {
                add(
                    rows,
                    quiet(
                        Anchor {
                            part: part.id.clone(),
                            ..anchor.clone()
                        },
                        "  ⋯ display rows not shown · layout memory limit".into(),
                    ),
                );
            }
        }
        omissions(record, rows, anchor, &part.id);
    }
    if let Some((parts, _)) = record.omitted.get("") {
        add(
            rows,
            quiet(
                anchor.clone(),
                format!("  ⋯ {parts} parts not shown · part limit"),
            ),
        );
    }
}
fn omissions(record: &Record, rows: &mut Vec<Located>, anchor: &Anchor, part: &str) {
    if let Some((bytes, lines)) = record.omitted.get(part) {
        add(
            rows,
            quiet(
                Anchor {
                    part: part.into(),
                    ..anchor.clone()
                },
                format!("  ⋯ {lines} lines / {bytes} bytes not shown · message is over 64 KiB"),
            ),
        );
    }
    if let Some(cells) = record.table_omitted.get(part) {
        add(
            rows,
            quiet(
                Anchor {
                    part: part.into(),
                    ..anchor.clone()
                },
                format!("  ⋯ {cells} table cells not shown · table limit"),
            ),
        );
    }
}
fn ending(record: &Record, anchor: Anchor, retry: bool) -> Option<Row> {
    let duration = if record.turn.duration.is_empty() {
        String::new()
    } else {
        format!(" · {}", record.turn.duration)
    };
    let (text, ink) = match &record.turn.end {
        End::Done(duration) => (
            format!(
                "  ✓ done{}",
                if duration.is_empty() {
                    String::new()
                } else {
                    format!(" · {duration}")
                }
            ),
            Ink {
                success: true,
                ..Ink::default()
            },
        ),
        End::Stopped(reason) | End::Cancelled(reason) => (
            format!(
                "  – {}{duration}{}",
                if matches!(record.turn.end, End::Cancelled(_)) {
                    "cancelled"
                } else {
                    "stopped"
                },
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(" · {reason}")
                }
            ),
            Ink {
                muted: true,
                ..Ink::default()
            },
        ),
        End::Interrupted(reason) => (
            format!("  ! interrupted · {reason}{duration}"),
            Ink {
                warning: true,
                ..Ink::default()
            },
        ),
        End::Failed(reason) => (
            format!(
                "  ✕ failed · {reason}{}",
                if retry { " · ctrl+x r retry" } else { "" }
            ),
            Ink {
                error: true,
                ..Ink::default()
            },
        ),
        End::Live | End::Quiet => return None,
    };
    Some(Row::plain(anchor, text, ink))
}
