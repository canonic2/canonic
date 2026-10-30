//! Local Completion Menu host exercising the public Composer/source contracts.
mod extended;
use crate::{
    chat::Chat,
    completion::{Batch, Candidate},
    composer::{CompletionItem, CompletionKind, Intent},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};
use std::time::Duration;

pub(super) struct CompletionDemo {
    chat: Chat,
    scenario: usize,
    presentation_pending: bool,
    age: Duration,
    note: String,
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn commands() -> Vec<CompletionItem> {
    let names = [
        "handoff",
        "code-review",
        "commit",
        "fix-checks",
        "clear",
        "compact",
        "model",
        "resume",
        "review",
        "release-diff",
        "restore",
        "stash",
        "help",
        "status",
        "diff",
        "branch",
        "checkout",
        "log",
        "test",
        "build",
        "format",
        "lint",
        "plan",
        "debug",
        "search",
        "open",
        "close",
        "undo",
        "redo",
        "settings",
        "session",
        "connect",
        "disconnect",
        "quit",
    ];
    names
        .iter()
        .enumerate()
        .map(|(i, name)| CompletionItem {
            id: (*name).into(),
            value: (*name).into(),
            detail: match i {
                0 => "copy a summary for another agent",
                1 => "review the diff for regressions",
                2 => "commit scoped changes after checks",
                3 => "restore failing checks",
                8 => "review a pull request · takes a pull request number",
                _ => "local host command",
            }
            .into(),
            kind: CompletionKind::Command,
            disabled: None,
            touched: i == 0,
        })
        .collect()
}
fn files() -> Vec<CompletionItem> {
    [
        "src/digest/settings.ts",
        "src/digest/settings.test.ts",
        "src/digest/schedule.ts",
        "docs/digest-settings.md",
    ]
    .iter()
    .enumerate()
    .map(|(i, name)| CompletionItem {
        id: (*name).into(),
        value: (*name).into(),
        detail: String::new(),
        kind: CompletionKind::File,
        disabled: None,
        touched: i == 0,
    })
    .collect()
}
fn candidate(item: &CompletionItem) -> Candidate {
    let name = if item.kind == CompletionKind::Command {
        format!("/{}", item.value)
    } else {
        item.value.clone()
    };
    let mut c = Candidate::new(&item.id, &name, &item.value);
    c.description.clone_from(&item.detail);
    c.disabled.clone_from(&item.disabled);
    c.recency = u64::from(item.touched);
    c.tag = if item.touched {
        if item.kind == CompletionKind::File {
            "file · edited"
        } else {
            "skill · recent"
        }
    } else if item.kind == CompletionKind::File {
        ""
    } else if matches!(
        item.value.as_str(),
        "code-review" | "commit" | "fix-checks" | "review" | "release-diff"
    ) {
        "command · project"
    } else {
        ""
    }
    .into();
    c
}
impl CompletionDemo {
    pub(super) fn take_presentation(&mut self, p: Presentation) -> Option<Presentation> {
        std::mem::take(&mut self.presentation_pending).then(|| extended::profile(self.scenario, p))
    }
    pub(super) fn scenario(index: usize) -> Self {
        let mut demo = Self {
            chat: Chat::new(),
            scenario: index,
            presentation_pending: true,
            age: Duration::ZERO,
            note: String::new(),
        };
        demo.chat.push(
            "acme · 00:38",
            "Rebased onto main: 4 commits, no conflicts.\nWorking tree is clean. Nothing pushed.",
        );
        demo.chat.set_status("orbit-api#276 ~chat");
        let mut items = commands();
        if index == 8 {
            for item in &mut items {
                if item.value == "resume" {
                    item.disabled = Some("needs a network".into());
                }
                if item.value == "restore" {
                    item.disabled = Some("no stashes".into());
                }
            }
        }
        if index == 12 {
            items[1].detail = "Review the current diff for correctness, lifecycle ownership and regressions. Explain each actionable finding with a concrete trigger, consequence and source location before recommending a fix.".into();
        }
        items.extend(files());
        demo.chat.composer_mut().set_completions(items);
        if index >= 15 {
            extended::prepare(&mut demo);
            return demo;
        }
        let (text, file) = match index {
            1 | 10 => ("set", true),
            2 | 6 => ("", true),
            3 => ("dig", true),
            4 => ("depoy", false),
            5 => ("setings", true),
            8 => ("re", false),
            9 => ("review", false),
            _ => ("", false),
        };
        if index == 14 {
            demo.chat.set_input("logs in /var/log/digest");
            return demo;
        }
        if file {
            demo.chat.set_input("also check ");
        }
        demo.chat
            .composer_mut()
            .update(&key(KeyCode::Char(if file { '@' } else { '/' })));
        for c in text.chars() {
            demo.chat.composer_mut().update(&key(KeyCode::Char(c)));
        }
        demo.seed();
        if matches!(index, 9 | 10) {
            demo.chat.composer_mut().update(&key(KeyCode::Tab));
        }
        if index == 12 {
            demo.chat.composer_mut().update(&key(KeyCode::Down));
        }
        demo
    }
    fn seed(&mut self) {
        let Some(request) = self.chat.composer().completion_request() else {
            return;
        };
        let composer = self.chat.composer_mut();
        if matches!(self.scenario, 2 | 3 | 5 | 6) {
            composer.set_completion_sources(&request, &["repository"]);
            if self.scenario == 6 {
                composer.complete_source(
                    &request,
                    "repository",
                    Batch {
                        error: Some("git ls-files exited 128 · not a git repository?".into()),
                        ..Batch::default()
                    },
                );
            } else if self.scenario == 5 {
                composer.complete_source(
                    &request,
                    "repository",
                    Batch {
                        complete: true,
                        total: Some(812),
                        ..Batch::default()
                    },
                );
                composer.set_completion_guess(
                    &request,
                    Candidate::new(
                        "closest",
                        "src/digest/settings.ts",
                        "src/digest/settings.ts",
                    ),
                );
            } else if self.scenario == 3 {
                composer.complete_source(
                    &request,
                    "repository",
                    Batch {
                        candidates: files().iter().take(3).map(candidate).collect(),
                        scanned: 240,
                        total: Some(812),
                        ..Batch::default()
                    },
                );
            }
            composer.tick(Duration::from_millis(160));
        } else if matches!(self.scenario, 7 | 13) {
            self.seed_commands(&request);
        } else if matches!(self.scenario, 1 | 10) {
            composer.set_completion_sources(&request, &["repository"]);
            composer.complete_source(
                &request,
                "repository",
                Batch {
                    candidates: files().iter().map(candidate).collect(),
                    complete: true,
                    total: Some(812),
                    scanned: 812,
                    ..Batch::default()
                },
            );
        } else if self.scenario == 9 {
            composer.set_completion_sources(&request, &["host"]);
            let mut c = Candidate::new("review", "/review", "review");
            c.argument_hint = "/review takes a pull request number".into();
            c.argument_placeholder = "<pr>".into();
            composer.complete_source(
                &request,
                "host",
                Batch {
                    candidates: vec![c],
                    complete: true,
                    ..Batch::default()
                },
            );
        } else {
            composer.set_completion_sources(&request, &["host"]);
            let rows=commands().into_iter().map(|mut item| {
                if self.scenario==8 && item.value=="resume" {item.disabled=Some("needs a network".into());}
                if self.scenario==8 && item.value=="restore" {item.disabled=Some("no stashes".into());}
                if self.scenario==12 && item.value=="code-review" {item.detail="Review the current diff for correctness, lifecycle ownership and regressions. Explain each actionable finding with a concrete trigger, consequence and source location before recommending a fix.".into();}
                candidate(&item)
            }).collect();
            composer.complete_source(
                &request,
                "host",
                Batch {
                    candidates: rows,
                    total: Some(34),
                    complete: true,
                    ..Batch::default()
                },
            );
        }
    }
    fn seed_commands(&mut self, request: &crate::composer::CompletionRequest) {
        let composer = self.chat.composer_mut();
        composer.set_completion_sources(
            request,
            &["project", "built-ins", "skills", "release-tools"],
        );
        let rows = commands();
        composer.complete_source(
            request,
            "project",
            Batch {
                candidates: rows[..4].iter().map(candidate).collect(),
                complete: true,
                ..Batch::default()
            },
        );
        let end = if self.scenario == 7 { 31 } else { rows.len() };
        let mut builtins: Vec<_> = rows[4..end].iter().map(candidate).collect();
        for c in &mut builtins {
            c.tag.clear();
        }
        composer.complete_source(
            request,
            "built-ins",
            Batch {
                candidates: builtins,
                complete: true,
                ..Batch::default()
            },
        );
        let skills = if self.scenario == 13 {
            let mut c = Candidate::new("skill-plan", "/plan-digest", "plan-digest");
            c.tag = "skill · project".into();
            vec![c]
        } else {
            Vec::new()
        };
        composer.complete_source(
            request,
            "skills",
            Batch {
                candidates: skills,
                complete: true,
                ..Batch::default()
            },
        );
        composer.complete_source(
            request,
            "release-tools",
            if self.scenario == 7 {
                Batch {
                    error: Some("3 commands failed to load".into()),
                    ..Batch::default()
                }
            } else {
                let mut c = Candidate::new("release-note", "release-note", "release-note");
                c.tag = "plugin · release-tools".into();
                Batch {
                    candidates: vec![c],
                    complete: true,
                    ..Batch::default()
                }
            },
        );
    }
    pub(super) fn input(&self) -> String {
        self.chat.composer().editor().expanded_text()
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if matches!(event,Event::Key(k) if k.kind == KeyEventKind::Press && k.code == KeyCode::F(2))
        {
            *self = Self::scenario((self.scenario + 1) % 30);
            return true;
        }
        if matches!(event,Event::Key(k) if matches!(k.code,KeyCode::F(4|10)) || k.modifiers == KeyModifiers::CONTROL && matches!(k.code,KeyCode::Char('c'|'t')))
        {
            return false;
        }
        if matches!(event,Event::Key(k) if k.code == KeyCode::Esc)
            && !self.chat.composer().overlay_open()
        {
            return false;
        }
        let before = self.chat.composer().completion_request();
        if let Some(submission) = self.chat.composer_mut().update(event) {
            self.chat.push_submission(&submission, "20:10");
            self.chat.acknowledge_submission(&submission.ticket, Ok(()));
            self.note = "Literal message accepted by local host".into();
        }
        for intent in self.chat.composer_mut().take_intents() {
            match intent {
                Intent::Command(id) => {
                    self.note = format!("Host invoked /{id} after Composer Enter");
                }
                Intent::CommandWithArguments { id, arguments } => {
                    self.note = format!("Host invoked /{id} {arguments} after Composer Enter");
                }
                Intent::RetryCompletion(request) => {
                    self.retry(&request);
                }
                _ => {}
            }
        }
        if self.chat.composer().completion_request() != before {
            self.age = Duration::ZERO;
        }
        true
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.chat.tick(elapsed);
        self.age += elapsed;
        // Local async source fixtures eventually finish, using the latest identity.
        if matches!(self.scenario, 2 | 3)
            && self.age >= Duration::from_secs(3)
            && let Some(request) = self.chat.composer().completion_request()
        {
            self.chat.composer_mut().complete_source(
                &request,
                "repository",
                Batch {
                    candidates: files().iter().map(candidate).collect(),
                    complete: true,
                    scanned: 812,
                    total: Some(812),
                    ..Batch::default()
                },
            );
        }
    }
    fn retry(&mut self, request: &crate::composer::CompletionRequest) {
        let sources = self
            .chat
            .composer()
            .completion_menu()
            .map_or(Vec::new(), |menu| {
                menu.pending_sources()
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect::<Vec<_>>()
            });
        for source in sources {
            let candidates = if request.kind == CompletionKind::File {
                files().iter().map(candidate).collect()
            } else {
                let mut row = Candidate::new("redeploy", "/redeploy", "redeploy");
                row.tag = "plugin · release-tools".into();
                vec![row]
            };
            if self.chat.composer_mut().complete_source(
                request,
                &source,
                Batch {
                    candidates,
                    complete: true,
                    ..Batch::default()
                },
            ) {
                self.note = if source == "repository" {
                    "Repository source retried successfully".into()
                } else {
                    format!("{source} source retried successfully")
                };
            }
        }
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, mut area: Rect, p: Presentation) {
        if self.scenario == 11 {
            area.width = area.width.min(40);
        }
        extended::sizing(self.scenario, &mut area);
        self.chat.render_docked_with(frame, area, p);
        if !self.note.is_empty() && area.height > 0 {
            frame.render_widget(
                ratatui::widgets::Paragraph::new(self.note.as_str()).style(p.muted()),
                Rect::new(area.x, area.y, area.width, 1),
            );
        }
    }
}
