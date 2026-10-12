//! Renders bounded question panels above caller-owned docks, using semantic profiles.

use super::{Kind, QuestionForm, State, model};
use crate::{
    editor::EditorView,
    text::{clean, elide, wrapped},
    theme::{ColorMode, Presentation, Symbols},
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::fmt::Write;

impl QuestionForm {
    /// Compact keys keep confirmation and cancellation before secondary actions.
    #[must_use]
    pub fn hint_for(&self, width: u16) -> &str {
        if width >= 50 {
            return self.hint();
        }
        if self.cancelling() {
            return "esc keeps answering";
        }
        match self.state {
            State::Submitting => "sending · keys paused",
            State::Expired => "esc close · y copy",
            _ if self.step == self.stages.len() && self.review_row.is_none() => {
                "enter send · esc cancel · 1–9 edit"
            }
            _ if self.step == self.stages.len() => "enter edit · esc cancel",
            _ if self.editing => "enter next · esc leave · ctrl+j line",
            _ => "enter next · esc cancel · space mark",
        }
    }
    /// Preferred content height; the host clamps it while retaining transcript rows.
    #[must_use]
    pub fn height(&self, width: u16, p: Presentation) -> u16 {
        let body = if let Some(q) = self.request.questions.get(self.step) {
            let field = self.editing || q.kind == Kind::Text;
            wrapped(&clean(&q.prompt, 65_536, true), width).len()
                + usize::from(!q.description.is_empty())
                    * wrapped(&clean(&q.description, 65_536, true), width)
                        .len()
                        .min(3)
                + if field {
                    self.stages[self.step]
                        .field
                        .visual_rows(
                            width.saturating_sub(usize_to_u16(usize::from(width >= 50) * 3)),
                            p.symbols,
                        )
                        .clamp(1, 6)
                } else {
                    q.choices.len() + usize::from(q.other)
                }
        } else {
            self.request.questions.len() * if width < 50 { 2 } else { 1 }
        };
        usize_to_u16(
            2 + usize::from(self.stages.len() > 1) + body + usize::from(!self.status().is_empty()),
        )
    }
    /// Applicable keys for Chat's fixed footer.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        if self.cancelling() {
            return "dialog · esc keeps answering";
        }
        match self.state {
            State::Submitting => "sending · keys paused",
            State::Expired => "y copy answers · esc close",
            State::Accepted | State::Cancelled => "closed",
            _ if self.step == self.stages.len() && self.review_row.is_some() => {
                "enter edit row · tab returns to review · esc cancel"
            }
            _ if self.step == self.stages.len() => "enter send · 1–9 edit · ↑↓ row · esc cancel",
            _ if self.editing => "enter next · ctrl+j new line · esc leave field",
            _ if self.request.questions[self.step].kind == Kind::Multiple => {
                "space mark · enter next · tab next · esc cancel"
            }
            _ => "1–9 choose · enter choose · tab next · esc cancel",
        }
    }
    fn status(&self) -> String {
        if self.state == State::Submitting {
            return format!("sending answers · {}s", self.elapsed.as_secs());
        }
        if self.state == State::Expired {
            return format!("! expired · {}", self.notice);
        }
        if matches!(self.state, State::Failed | State::Refused) && !self.notice.is_empty() {
            return format!("× not sent · {}", self.notice);
        }
        self.notice.clone()
    }
    /// Render one panel and its optional Dialog in the same caller-owned frame.
    pub fn render_with(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
        waiting: usize,
    ) {
        self.area = area;
        if area.is_empty() {
            return;
        }
        let locked = matches!(self.state, State::Submitting | State::Expired);
        frame.render_widget(Block::default().style(p.base()), area);
        let mut metadata = if self.state == State::Expired {
            "expired".into()
        } else if self.step == self.stages.len() {
            "review".into()
        } else if self.stages.len() == 1 {
            String::new()
        } else if area.width < 50 {
            format!("{}/{}", self.step + 1, self.stages.len())
        } else {
            format!(
                "{} of {}{}",
                self.step + 1,
                self.stages.len(),
                if self.from_review {
                    " · from review"
                } else {
                    ""
                }
            )
        };
        if waiting > 0 {
            let _ = write!(metadata, " · +{waiting} waiting");
        }
        rule(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            &clean(&self.request.title, 4096, false),
            &metadata,
            p,
        );
        if area.height == 1 {
            return;
        }
        let status = clean(&self.status(), 4096, false);
        let steps = u16::from(self.stages.len() > 1 && area.height >= 4);
        if steps > 0 {
            frame.render_widget(
                Paragraph::new(self.steps(p, area.width)).style(p.muted()),
                Rect::new(area.x, area.y + 1, area.width, 1),
            );
        }
        let status_rows = u16::from(!status.is_empty() && area.height > steps + 2);
        let body = Rect::new(
            area.x,
            area.y + 1 + steps,
            area.width,
            area.height.saturating_sub(2 + steps + status_rows),
        );
        let count = if self.step == self.stages.len() {
            self.render_review(frame, body, p);
            String::new()
        } else {
            self.render_question(frame, body, p, !locked && !self.cancelling())
        };
        if status_rows > 0 {
            let style = if matches!(self.state, State::Failed | State::Refused) {
                p.error()
            } else {
                p.warning()
            };
            frame.render_widget(
                Paragraph::new(status).style(style),
                Rect::new(area.x, area.bottom() - 2, area.width, 1),
            );
        }
        rule(
            frame,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            "",
            &count,
            p,
        );
        if locked {
            frame
                .buffer_mut()
                .set_style(body, p.muted().add_modifier(Modifier::DIM));
        }
    }
    fn steps(&self, p: Presentation, width: u16) -> Line<'static> {
        let ascii = p.symbols == Symbols::Ascii;
        let spans = self
            .request
            .questions
            .iter()
            .enumerate()
            .chain(std::iter::once((
                self.stages.len(),
                &self.request.questions[0],
            )))
            .map(|(i, q)| {
                let current = self.step == i;
                let mark = if current {
                    if ascii { ">" } else { "›" }
                } else if i == self.stages.len() {
                    if ascii { "o" } else { "○" }
                } else if model::answer(q, &self.stages[i]).is_some() {
                    if ascii { "ok" } else { "✓" }
                } else if self.stages[i].visited && q.required {
                    "!"
                } else if self.stages[i].visited {
                    "–"
                } else if ascii {
                    "o"
                } else {
                    "○"
                };
                let label = if i == self.stages.len() {
                    "review"
                } else {
                    &q.label
                };
                let text = if current && p.colors == ColorMode::NoColor {
                    format!("[{label}]  ")
                } else if width < 50 && !current {
                    format!("{mark} ")
                } else {
                    format!("{mark} {label}  ")
                };
                Span::styled(
                    clean(&text, 4096, false),
                    if current {
                        p.accent()
                    } else if mark == "!" {
                        p.warning()
                    } else {
                        p.muted()
                    },
                )
            })
            .collect::<Vec<_>>();
        Line::from(spans)
    }
    fn render_question(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
        focused: bool,
    ) -> String {
        let q = &self.request.questions[self.step];
        let prompt = wrapped(&clean(&q.prompt, 65_536, true), area.width);
        let prompt_height = usize_to_u16(prompt.len()).min(area.height.saturating_sub(1));
        frame.render_widget(
            Paragraph::new(
                prompt
                    .into_iter()
                    .take(usize::from(prompt_height))
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            )
            .style(p.heading()),
            Rect::new(area.x, area.y, area.width, prompt_height),
        );
        let descriptions = if q.description.is_empty() {
            vec![]
        } else {
            wrapped(&clean(&q.description, 65_536, true), area.width)
        };
        let description_height = usize_to_u16(descriptions.len().min(3))
            .min(area.height.saturating_sub(prompt_height + 1));
        frame.render_widget(
            Paragraph::new(
                descriptions
                    .into_iter()
                    .take(usize::from(description_height))
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            )
            .style(p.muted()),
            Rect::new(
                area.x,
                area.y + prompt_height,
                area.width,
                description_height,
            ),
        );
        let choices = Rect::new(
            area.x,
            area.y + prompt_height + description_height,
            area.width,
            area.height
                .saturating_sub(prompt_height + description_height),
        );
        self.render_answers(frame, choices, p, focused)
    }
    fn render_answers(
        &mut self,
        frame: &mut Frame<'_>,
        choices: Rect,
        p: Presentation,
        focused: bool,
    ) -> String {
        let q = &self.request.questions[self.step];
        let s = &mut self.stages[self.step];
        if self.editing || q.kind == Kind::Text {
            let indent = u16::from(choices.width >= 50) * 3;
            s.field.render_with(
                frame,
                Rect::new(
                    choices.x + indent.min(choices.width),
                    choices.y,
                    choices.width.saturating_sub(indent),
                    choices.height.min(6),
                ),
                EditorView {
                    placeholder: if q.required {
                        "type an answer"
                    } else {
                        "type here, or tab to skip"
                    },
                    presentation: p,
                    chrome: false,
                    ..EditorView::default()
                },
                focused && self.editing,
            );
            let count = s.field.text().chars().count();
            return q
                .characters
                .map_or_else(String::new, |max| format!("{count} of {max}"));
        }
        self.viewport = usize::from(choices.height).max(1);
        if s.cursor < self.offset {
            self.offset = s.cursor;
        }
        if s.cursor >= self.offset + self.viewport {
            self.offset = s.cursor + 1 - self.viewport;
        }
        let total = q.choices.len() + usize::from(q.other);
        self.offset = self.offset.min(total.saturating_sub(self.viewport));
        let lines = (self.offset..total)
            .take(self.viewport)
            .map(|i| choice_line(q, s, i, choices.width, p, focused))
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).style(p.base()), choices);
        let count = if q.kind == Kind::Multiple {
            format!(
                "{}{} marked",
                s.marks.len() + usize::from(s.other),
                q.maximum.map_or_else(String::new, |m| format!(" of {m}"))
            )
        } else {
            String::new()
        };
        let below = total.saturating_sub(self.offset + self.viewport);
        format!(
            "{}{}{}",
            if self.offset > 0 {
                format!("↑ {} more · ", self.offset)
            } else {
                String::new()
            },
            count,
            if below > 0 {
                format!(" · ↓ {below} more")
            } else {
                String::new()
            }
        )
    }
    fn render_review(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        self.viewport = usize::from(area.height / if area.width < 50 { 2 } else { 1 }).max(1);
        if let Some(row) = self.review_row {
            if row < self.offset {
                self.offset = row;
            }
            if row >= self.offset + self.viewport {
                self.offset = row + 1 - self.viewport;
            }
        }
        self.offset = self
            .offset
            .min(self.stages.len().saturating_sub(self.viewport));
        let mut lines = vec![];
        for (i, (q, s)) in self
            .request
            .questions
            .iter()
            .zip(&self.stages)
            .enumerate()
            .skip(self.offset)
            .take(self.viewport)
        {
            let invalid = model::validation(q, s);
            let answer = model::answer(q, s).map_or_else(
                || {
                    if let Some(reason) = &invalid {
                        format!("! required · {reason}")
                    } else {
                        "– skipped · optional".into()
                    }
                },
                |a| model::display(q, &a.value),
            );
            let style = if self.review_row == Some(i) {
                p.selection()
            } else if invalid.is_some() {
                p.warning()
            } else {
                p.base()
            };
            let label = clean(&q.label, 4096, false);
            if area.width < 50 {
                lines.push(Line::from(elide(
                    &format!("{} {label}", i + 1),
                    area.width,
                    p.symbols,
                )));
                lines.push(Line::styled(
                    format!(
                        "  {}",
                        elide(
                            &clean(&answer.replace('\n', " · "), 65_536, false),
                            area.width.saturating_sub(2),
                            p.symbols
                        )
                    ),
                    style,
                ));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} {label}   ", i + 1), p.muted()),
                    Span::styled(
                        elide(
                            &clean(&answer.replace('\n', " · "), 65_536, false),
                            area.width
                                .saturating_sub(usize_to_u16(label.chars().count() + 5)),
                            p.symbols,
                        ),
                        style,
                    ),
                ]));
            }
        }
        frame.render_widget(Paragraph::new(lines).style(p.base()), area);
    }
    /// Render cancellation above the panel using the host's complete modal viewport.
    /// Call after drawing the background and dock; input remains isolated by the form.
    pub fn render_overlay(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        self.area = area;
        self.cancel.render_with(
            frame,
            area,
            p,
            |body, _, _| usize_to_u16(body.choices().len()),
            |body, frame, body_area, _, _| {
                body.render_list(frame, body_area, p, true);
                None
            },
        );
    }
}
fn usize_to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}
fn rule(frame: &mut Frame<'_>, area: Rect, left: &str, right: &str, p: Presentation) {
    frame.render_widget(
        Rule {
            left: Line::styled(
                elide(
                    left,
                    area.width.saturating_sub(usize_to_u16(
                        right.chars().count() + usize::from(!right.is_empty()),
                    )),
                    p.symbols,
                ),
                p.accent(),
            ),
            right: Line::styled(right.to_owned(), p.muted()),
            glyph: if p.symbols == Symbols::Ascii {
                '-'
            } else {
                '─'
            },
            style: p.info(),
        },
        area,
    );
}

fn choice_line(
    q: &super::Question,
    s: &super::Stage,
    i: usize,
    width: u16,
    p: Presentation,
    focused: bool,
) -> Line<'static> {
    let other = i == q.choices.len();
    let choice = q.choices.get(i);
    let selected = s.cursor == i;
    let marked = if other { s.other } else { s.marks.contains(&i) };
    let disabled = choice.is_some_and(|c| c.disabled.is_some());
    let mark = if p.colors == ColorMode::NoColor || p.symbols == Symbols::Ascii {
        if disabled {
            "[-]"
        } else if marked {
            "[x]"
        } else {
            "[ ]"
        }
    } else if marked {
        if q.kind == Kind::Multiple {
            "◉"
        } else {
            "✓"
        }
    } else {
        "○"
    };
    let number = if q.kind == Kind::Single {
        format!("{} ", i + 1)
    } else {
        String::new()
    };
    let label = if other {
        if s.other {
            format!("\"{}\"", clean(s.field.text(), 4096, false))
        } else {
            "Something else".into()
        }
    } else {
        clean(&q.choices[i].label, 4096, false)
    };
    let detail = if width < 50 {
        ""
    } else {
        choice.map_or("your answer", |c| {
            c.disabled.as_deref().unwrap_or(&c.detail)
        })
    };
    let capped = q.kind == Kind::Multiple
        && !marked
        && q.maximum
            .is_some_and(|m| s.marks.len() + usize::from(s.other) >= m);
    let style = if selected && focused {
        p.selection()
    } else if disabled || capped {
        p.disabled()
    } else {
        p.base()
    };
    Line::from(vec![
        Span::styled(
            format!(
                "{} {number}{mark} {label}",
                if selected && p.symbols == Symbols::Ascii {
                    ">"
                } else {
                    " "
                }
            ),
            style,
        ),
        Span::styled(
            format!("  {}", clean(detail, 4096, false)),
            if selected && focused {
                style
            } else {
                p.muted()
            },
        ),
    ])
}
