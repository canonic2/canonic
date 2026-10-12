//! Builds cached logical transcript rows and renders a fixed Composer dock.

use super::{
    Anchor, Chat, Page, PendingKind,
    content::{self, Ink},
    layout::{self, Cached, Located, Row},
    model::{End, Mode},
};
use crate::{
    composer::State,
    editor::EditorView,
    theme::{ColorMode, Presentation, Symbols, Theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

impl Chat {
    pub(super) fn refresh(&mut self) {
        self.parse_parts();
        self.recompute_search();
        self.layout_parts();
        self.build_rows();
        if self.follow {
            self.scroll = self.maximum();
        } else if let Some(anchor) = self.anchor.clone() {
            if let Some(index) = self.locate(&anchor) {
                self.scroll = index.min(self.maximum());
            } else {
                self.scroll = self.scroll.min(self.maximum());
                self.say("reading position is outside loaded history");
            }
        } else {
            self.scroll = self.scroll.min(self.maximum());
        }
        self.capture_anchor();
    }
    fn parse_parts(&mut self) {
        let changed: Vec<_> = self
            .messages
            .iter()
            .flat_map(|m| {
                m.turn.parts.iter().map(|p| {
                    (
                        (m.turn.id.clone(), p.id.clone()),
                        p.revision,
                        p.content.clone(),
                    )
                })
            })
            .filter(|(key, revision, _)| {
                self.cache.get(key).is_none_or(|c| c.revision != *revision)
            })
            .collect();
        for (key, _, content) in changed {
            self.sync_tools(&key, &content);
        }
        for message in &self.messages {
            let mut budget = 1024usize;
            for part in &message.turn.parts {
                let key = (message.turn.id.clone(), part.id.clone());
                if let Some(cache) = self.cache.get(&key).filter(|c| c.revision == part.revision) {
                    budget = budget.saturating_sub(cache.blocks.len());
                    continue;
                }
                let mut blocks = content::blocks(&part.content, budget);
                if let Some(view) = self.tools.get(&key) {
                    view.anchor_blocks(&mut blocks);
                }
                budget = budget.saturating_sub(blocks.len());
                self.metrics.parses += 1;
                self.cache.insert(
                    key,
                    Cached {
                        revision: part.revision,
                        blocks,
                        width: 0,
                        expanded: false,
                        rows: Arc::new(vec![]),
                        bytes: 0,
                        memory_limited: false,
                        budget: 0,
                    },
                );
            }
        }
    }
    fn layout_parts(&mut self) {
        let mut remaining = layout::ROW_LIMIT;
        let mut bytes = layout::BYTE_LIMIT;
        let center = if self.follow {
            self.messages.len().saturating_sub(1)
        } else {
            self.anchor
                .as_ref()
                .and_then(|a| self.messages.iter().position(|m| m.turn.id == a.message))
                .unwrap_or(0)
        };
        let mut order: Vec<_> = (0..self.messages.len()).collect();
        order.sort_by_key(|index| index.abs_diff(center));
        for index in order {
            let message = &self.messages[index];
            for part in &message.turn.parts {
                let key = (message.turn.id.clone(), part.id.clone());
                let expanded = self.expanded.contains(&key)
                    || self
                        .search
                        .as_ref()
                        .is_some_and(|s| s.temporary.contains(&key));
                let Some(cache) = self.cache.get_mut(&key) else {
                    continue;
                };
                if cache.width != self.width
                    || (self.tools.contains_key(&key) && self.search.is_some())
                    || cache.expanded != expanded
                    || cache.rows.len() > remaining
                    || cache.bytes > bytes
                    || (cache.memory_limited && bytes > cache.budget)
                {
                    let mut rows = if let Some(view) = self.tools.get(&key) {
                        view.rows(
                            &cache.blocks,
                            (&message.turn.id, &part.id),
                            self.width,
                            self.tool_symbols,
                            expanded,
                            self.search
                                .as_ref()
                                .and_then(|s| s.matches.get(s.current))
                                .filter(|m| {
                                    m.block.message == message.turn.id && m.block.part == part.id
                                })
                                .map(|m| m.start),
                        )
                    } else if message.shell.as_ref().is_some_and(|(_, open)| !open)
                        && !expanded
                        && let Some(preview) = layout::shell_preview(
                            &cache.blocks,
                            &message.turn.id,
                            &part.id,
                            self.width,
                        )
                    {
                        preview
                    } else {
                        layout::layout(
                            &cache.blocks,
                            &message.turn.id,
                            &part.id,
                            self.width,
                            expanded,
                            remaining,
                        )
                    };
                    cap_rows(&mut rows, remaining, &message.turn.id, &part.id);
                    let mut used = 0;
                    let before = rows.len();
                    rows.retain(|row| {
                        used += row.bytes();
                        used <= bytes
                    });
                    rows.shrink_to_fit();
                    cache.memory_limited =
                        rows.len() < before || (rows.is_empty() && !cache.blocks.is_empty());
                    cache.budget = bytes;
                    cache.bytes = rows.iter().map(Row::bytes).sum();
                    cache.rows = Arc::new(rows);
                    cache.width = self.width;
                    cache.expanded = expanded;
                    self.metrics.layouts += 1;
                }
                remaining = remaining.saturating_sub(cache.rows.len());
                bytes = bytes.saturating_sub(cache.bytes);
            }
        }
        self.metrics.cached_rows = layout::ROW_LIMIT - remaining;
        self.metrics.cached_bytes = layout::BYTE_LIMIT - bytes;
    }
    fn own_row(&mut self, row: Row) {
        self.rows.push(Located {
            rows: Arc::new(vec![row]),
            index: 0,
        });
    }
    fn build_rows(&mut self) {
        self.rows = super::transcript::rows(self);
        if self.messages.is_empty() {
            let title = if self.welcome.title.is_empty() {
                "canonic"
            } else {
                &self.welcome.title
            };
            self.own_row(Row::plain(
                Anchor::default(),
                title.into(),
                Ink {
                    bold: true,
                    accent: true,
                    ..Ink::default()
                },
            ));
            self.own_row(Row::plain(
                Anchor::default(),
                self.welcome.context.clone(),
                Ink {
                    muted: true,
                    ..Ink::default()
                },
            ));
            for text in self.welcome.recent.clone() {
                self.own_row(Row::plain(
                    Anchor::default(),
                    format!("  {text}"),
                    Ink {
                        muted: true,
                        ..Ink::default()
                    },
                ));
            }
            if !self.welcome.suggestions.is_empty() {
                self.own_row(Row::plain(
                    Anchor::default(),
                    "Suggested · from host".into(),
                    Ink {
                        muted: true,
                        ..Ink::default()
                    },
                ));
            }
            for text in self.welcome.suggestions.clone() {
                self.own_row(Row::plain(
                    Anchor::default(),
                    format!("  {text}"),
                    Ink::default(),
                ));
            }
            self.own_row(Row::plain(
                Anchor::default(),
                "Try: / commands · @ files · F1 keys".into(),
                Ink {
                    muted: true,
                    ..Ink::default()
                },
            ));
        }
    }
    pub(super) fn maximum(&self) -> usize {
        self.rows.len().saturating_sub(self.height)
    }
    pub(super) fn locate(&self, anchor: &Anchor) -> Option<usize> {
        let candidates: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.row().anchor.message == anchor.message && row.row().anchor.part == anchor.part
            })
            .collect();
        // Decorations can share an offset; prefer the first content row closest to it.
        candidates
            .iter()
            .filter(|(_, row)| row.row().anchor.offset <= anchor.offset)
            .min_by_key(|(_, row)| anchor.offset - row.row().anchor.offset)
            .or_else(|| candidates.first())
            .map(|(i, _)| *i)
    }
    pub(super) fn capture_anchor(&mut self) {
        self.anchor = self.rows.get(self.scroll).map(|r| r.row().anchor.clone());
    }
    /// Render the framed compatibility composition.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Theme) {
        self.render_with(frame, area, theme.presentation());
    }
    /// Render with explicit color/alphabet capability inputs.
    pub fn render_with(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        self.render_composition(frame, area, theme, true);
    }
    /// Render the design's plain transcript over a fixed Composer and host footer.
    pub fn render_docked_with(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        self.render_composition(frame, area, theme, false);
    }
    fn render_composition(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: Presentation,
        framed: bool,
    ) {
        self.sync_reviews();
        let review_height = self.reviews.active().map_or(0, |r| {
            r.height(area.width, theme)
                .min(area.height.saturating_sub(11))
        });
        let [
            status,
            transcript,
            unread,
            above,
            review,
            activity,
            composer,
            hints,
        ] = Layout::vertical([
            Constraint::Length(u16::from(framed)),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(self.input.above_height_for(area.width)),
            Constraint::Length(review_height),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(format!("Chat · {}", self.status)).style(theme.heading()),
            status,
        );
        self.input.render_above(frame, above, theme);
        let block = if framed {
            Block::bordered().title("Conversation")
        } else {
            Block::default()
        };
        let inner = block.inner(transcript);
        self.width = inner.width;
        if self.tool_symbols != theme.symbols {
            self.tool_symbols = theme.symbols;
            for (key, cache) in &mut self.cache {
                if self.tools.contains_key(key) {
                    cache.width = 0;
                }
            }
        }
        self.height = usize::from(inner.height).max(1);
        self.refresh();
        frame.render_widget(block, transcript);
        let lines: Vec<_> = self
            .rows
            .iter()
            .skip(self.scroll)
            .take(usize::from(inner.height))
            .map(|row| self.render_row(row.row(), theme))
            .collect();
        frame.render_widget(Paragraph::new(lines).style(theme.base()), inner);
        self.render_unread(frame, unread, theme);
        self.render_activity(frame, activity, theme);
        self.input.render_with(
            frame,
            composer,
            EditorView {
                title: "message",
                placeholder: "Write a message",
                presentation: theme,
                footer: if framed {
                    ""
                } else if self.footer.is_empty() {
                    &self.status
                } else {
                    &self.footer
                },
                ..EditorView::default()
            },
            self.mode() == Mode::Compose && self.reviews.active().is_none(),
        );
        if self.mode() != Mode::Compose || self.reviews.active().is_some() {
            frame.buffer_mut().set_style(
                Rect::new(
                    composer.x,
                    composer.y.saturating_add(1),
                    composer.width,
                    composer.height.saturating_sub(2),
                ),
                theme.muted().add_modifier(Modifier::DIM),
            );
        }
        let hint = self.focus_hint(hints.width);
        let hint = self
            .reviews
            .active()
            .map_or(hint, |r| r.hint_for(hints.width));
        self.render_run_hints(frame, hints, theme, &hint);
        self.input.render_overlay(frame, area, theme);
        if let Some(chooser) = &mut self.artifacts {
            chooser.render(frame, area, activity, theme);
        }
        self.render_reviews(frame, review, activity, theme);
        self.reviews.render_overlay(frame, area, theme);
    }
    fn render_reviews(
        &mut self,
        frame: &mut Frame<'_>,
        review: Rect,
        activity: Rect,
        theme: Presentation,
    ) {
        if self.reviews.active().is_some() {
            self.reviews.render_with(frame, review, theme);
        } else if self.reviews.waiting_count() > 0 {
            frame.render_widget(
                Paragraph::new(self.reviews.waiting_label()).style(theme.warning()),
                activity,
            );
        }
    }
    fn focus_hint(&self, width: u16) -> String {
        match self.mode() {
            Mode::Search => if self.query().is_empty() { "esc closes".into() } else if width < 50 { "esc back · ↑↓ match · enter stay".into() } else { "↑↓ prev/next · enter stay here · esc back".into() },
            Mode::Select if self.pending.as_ref().is_some_and(|p| matches!(p.kind, PendingKind::Copy)) => "esc done · copy pending".into(),
            Mode::Select => self.tool_hint().unwrap_or_else(|| match width {
                0..=49 => "↑↓ block · y copy · esc done",
                50..=89 => "↑↓ block · y copy · c code · o open · esc done",
                _ => "↑↓ block · shift+↑↓ extend · m message · y copy · c code · o open · enter fold · esc done",
            }.into()),
            Mode::Compose => self.input.footer_hint_for(width).replace(
                "actions on", &format!("actions {}", self.actions)),
        }
    }
    fn render_run_hints(
        &self,
        frame: &mut Frame<'_>,
        hints: Rect,
        theme: Presentation,
        hint: &str,
    ) {
        let mut connection = self
            .run
            .as_ref()
            .map_or_else(String::new, |run| run.connection_label(theme.symbols));
        if hints.width < 50 {
            connection = connection.replace(" · retry in ", " · retry ");
        }
        let [hint_area, connection_area] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(
                u16::try_from(connection.chars().count())
                    .unwrap_or(u16::MAX)
                    .min(hints.width / 2),
            ),
        ])
        .areas(hints);
        frame.render_widget(
            Paragraph::new(connection).style(theme.muted()),
            connection_area,
        );
        frame.render_widget(
            Paragraph::new(alphabet(hint, theme)).style(theme.muted()),
            hint_area,
        );
    }
    fn render_unread(&self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        let text = match &self.page {
            Some(Page::Loading(_)) => "loading earlier messages…".into(),
            Some(Page::Failed(reason)) => {
                format!("could not load earlier · {reason} · alt+home retry")
            }
            None if self
                .pending
                .as_ref()
                .is_some_and(|p| matches!(p.kind, PendingKind::Latest)) =>
            {
                "loading newer messages…".into()
            }
            None if self.newer_failure.is_some() => format!(
                "alt+end retry · could not load newer · {}",
                self.newer_failure.as_deref().unwrap_or_default()
            ),
            None if !self.unread.is_empty() => {
                if self
                    .messages
                    .iter()
                    .any(|m| self.unread.contains(&m.turn.id) && matches!(m.turn.end, End::Live))
                {
                    "↓ new output · alt+end".into()
                } else {
                    format!("↓ {} new messages · alt+end", self.unread.len())
                }
            }
            None if self.newer > 0 => format!("{} newer messages not loaded · alt+end", self.newer),
            None if self.earlier > 0 => {
                format!("{} earlier messages not loaded · alt+home", self.earlier)
            }
            None => String::new(),
        };
        let style = if self.unread.is_empty() {
            theme.muted()
        } else {
            theme.selection().patch(theme.accent())
        };
        frame.render_widget(Paragraph::new(alphabet(&text, theme)).style(style), area);
    }
    fn render_search(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        if let Some(search) = &mut self.search {
            let count = search.matches.len();
            let scope = format!(
                "{} loaded messages{}",
                self.messages.len(),
                if self.earlier > 0 {
                    " · earlier messages not searched"
                } else {
                    ""
                }
            );
            let position = if search.editor.text().is_empty() {
                format!("search {scope}")
            } else if count == 0 {
                format!("no matches · {scope}")
            } else {
                format!(
                    "{} of {count}{}{}{} · {scope}",
                    search.current + 1,
                    if search.limited {
                        " · first 4096 only"
                    } else {
                        ""
                    },
                    if search.wrapped { " · wrapped" } else { "" },
                    if search.temporary.is_empty() {
                        ""
                    } else {
                        " · opened to show a match"
                    }
                )
            };
            let [label, query, metadata] = Layout::horizontal([
                Constraint::Length(5),
                Constraint::Length(
                    (area.width / 3)
                        .min(area.width.saturating_sub(20).max(1))
                        .min(32),
                ),
                Constraint::Min(0),
            ])
            .areas(area);
            frame.render_widget(Paragraph::new("find ").style(theme.muted()), label);
            search.editor.render_with(
                frame,
                query,
                EditorView {
                    chrome: false,
                    presentation: theme,
                    ..EditorView::default()
                },
                true,
            );
            frame.render_widget(
                Paragraph::new(format!(" {position}")).style(theme.muted()),
                metadata,
            );
        }
    }
    fn render_activity(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        if self.search.is_some() {
            self.render_search(frame, area, theme);
            return;
        }
        let text = if let Some(pending) = &self.pending {
            pending.kind.waiting()
        } else if let Some((notice, _)) = &self.notice {
            notice.clone()
        } else if self.selection.is_some() {
            "select · ↑↓ moves by block".into()
        } else if self.leader.is_some() {
            format!(
                "ctrl+x · v select · t timestamps{}",
                if self.retry { " · r retry" } else { "" }
            )
        } else if let Some(hint) = self.input.completion_hint_for(area.width) {
            hint.into()
        } else if self.run.is_some() {
            self.render_run_activity(frame, area, theme);
            return;
        } else if !self.input.status().is_empty() {
            self.input.status_for(area.width)
        } else if matches!(self.input.state(), State::Pending(_)) {
            String::new()
        } else {
            self.status.clone()
        };
        let style = if (self.input.queue_paused() && !self.input.queue().is_empty())
            || text.starts_with('!')
        {
            theme.warning()
        } else if matches!(self.input.state(), State::Rejected { .. })
            || text.starts_with("failed")
            || text.starts_with('✕')
        {
            theme.error()
        } else if text.starts_with('✓') {
            ink_style(
                Ink {
                    success: true,
                    ..Ink::default()
                },
                theme,
            )
        } else {
            theme.muted()
        };
        frame.render_widget(Paragraph::new(alphabet(&text, theme)).style(style), area);
    }
    pub(crate) fn render_run_activity(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: Presentation,
    ) {
        let Some(run) = &self.run else {
            return;
        };
        let count = self.input.queue().len();
        if count == 0 {
            run.render_with(frame, area, theme);
            return;
        }
        let suffix = format!(" · {count} queued");
        let suffix_width = u16::try_from(suffix.len())
            .unwrap_or(u16::MAX)
            .min(area.width / 2);
        let run_area = Rect {
            width: area.width.saturating_sub(suffix_width),
            ..area
        };
        run.render_with(frame, run_area, theme);
        frame.render_widget(
            Paragraph::new(suffix).style(theme.muted()),
            Rect::new(run_area.right(), area.y, suffix_width, area.height),
        );
    }
    fn render_row(&self, row: &Row, theme: Presentation) -> Line<'static> {
        let selected = row.block.as_ref().is_some_and(|id| self.selected(id))
            || (row.header
                && self
                    .selection
                    .as_ref()
                    .is_some_and(|s| s.whole && s.last.message == row.anchor.message));
        let tool = self
            .tools
            .contains_key(&(row.anchor.message.clone(), row.anchor.part.clone()));
        let reversed_head = selected && tool && theme.colors == ColorMode::NoColor && row.header;
        let prefix = if selected && !(tool && theme.colors == ColorMode::NoColor) {
            if row.prefix.starts_with("  ") {
                format!("▌{}", &row.prefix[1..])
            } else {
                "▌".into()
            }
        } else {
            row.prefix.clone()
        };
        let mut spans = vec![Span::styled(
            alphabet(&prefix, theme),
            if selected {
                theme.accent()
            } else {
                theme.muted()
            },
        )];
        let mut offset = row.anchor.offset;
        let mut displayed = 0;
        for run in &row.runs {
            let mut style = ink_style(run.ink, theme);
            if reversed_head {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if row.surface && theme.tokens().surface.is_some() {
                style = theme.surface().patch(style);
            }
            if selected && tool && matches!(theme.colors, ColorMode::TrueColor | ColorMode::Ansi256)
            {
                style = style.bg(theme.tokens().selection.expect("color profile selection"));
            }
            for grapheme in run.text.graphemes(true) {
                let source = row
                    .map
                    .as_ref()
                    .and_then(|map| map.get(displayed))
                    .copied()
                    .unwrap_or(offset);
                let mut glyph_style = style;
                if row.searchable
                    && !run.generated
                    && let Some(search) = &self.search
                {
                    for (i, found) in search.matches.iter().enumerate() {
                        if row.block.as_ref() == Some(&found.block)
                            && source < found.end
                            && source.saturating_add(grapheme.len()) > found.start
                        {
                            glyph_style = if i == search.current {
                                theme.selection().patch(theme.accent())
                            } else if matches!(
                                theme.colors,
                                crate::theme::ColorMode::Ansi16 | crate::theme::ColorMode::NoColor
                            ) {
                                glyph_style.add_modifier(Modifier::UNDERLINED)
                            } else {
                                theme.surface().add_modifier(Modifier::UNDERLINED)
                            };
                            break;
                        }
                    }
                }
                spans.push(Span::styled(
                    if grapheme == "␛" && theme.symbols == Symbols::Ascii {
                        "^[".into()
                    } else if run.generated && !tool {
                        alphabet(grapheme, theme)
                    } else {
                        grapheme.into()
                    },
                    glyph_style,
                ));
                offset += if run.generated { 0 } else { grapheme.len() };
                displayed += grapheme.len();
            }
        }
        let line = Line::from(spans);
        if selected && tool && matches!(theme.colors, ColorMode::TrueColor | ColorMode::Ansi256) {
            line.style(theme.selection())
        } else {
            line
        }
    }
}

fn cap_rows(rows: &mut Vec<Row>, remaining: usize, message: &str, part: &str) {
    let limit = remaining.min(layout::PART_ROWS);
    if rows.len() > limit {
        rows.truncate(limit.saturating_sub(1));
        if limit > 0 {
            rows.push(Row::plain(
                Anchor {
                    message: message.into(),
                    part: part.into(),
                    offset: 0,
                },
                "  ⋯ tool display rows not shown · layout limit".into(),
                Ink {
                    muted: true,
                    ..Ink::default()
                },
            ));
        }
    }
}

fn ink_style(ink: Ink, theme: Presentation) -> Style {
    let mut style = theme.base();
    if ink.muted {
        style = theme.muted();
    }
    if ink.accent {
        style = theme.accent();
    }
    if ink.link {
        style = theme
            .tokens()
            .info
            .map_or(style, |c| style.fg(c))
            .add_modifier(Modifier::UNDERLINED);
    }
    if ink.info {
        style = theme.tokens().info.map_or(style, |c| style.fg(c));
    }
    if ink.success {
        style = theme.tokens().success.map_or(style, |c| style.fg(c));
    }
    if ink.error {
        style = theme.error();
    }
    if ink.warning {
        style = theme.warning();
    }
    if ink.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if ink.italic {
        style = style.add_modifier(
            if matches!(theme.colors, ColorMode::NoColor | ColorMode::Ansi16) {
                Modifier::UNDERLINED
            } else {
                Modifier::ITALIC
            },
        );
    }
    if ink.surface {
        style = if theme.tokens().surface.is_some() {
            theme.surface().patch(style)
        } else {
            style.add_modifier(Modifier::UNDERLINED)
        };
    }
    style
}
fn alphabet(text: &str, theme: Presentation) -> String {
    if theme.symbols != Symbols::Ascii {
        return text.into();
    }
    text.replace('─', "-")
        .replace(['│', '▌'], "|")
        .replace('▾', "v")
        .replace('▸', ">")
        .replace('▤', "file")
        .replace('✓', "+")
        .replace('✕', "x")
        .replace('↗', "> ")
        .replace('↻', "~")
        .replace('↩', "\\")
        .replace(['⋯', '…'], "...")
        .replace('↑', "up")
        .replace('↓', "down")
        .replace(['−', '–'], "-")
        .replace('·', "/")
}
