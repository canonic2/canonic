//! Live completion edge cases composed through the public draft/source APIs.

use super::{CompletionDemo, candidate, commands, files, key};
use crate::{
    completion::{Batch, Candidate},
    event::{Event, KeyCode},
    theme::{ColorMode, Presentation, Symbols, Theme},
};
use ratatui::layout::Rect;
use std::time::Duration;

fn type_text(demo: &mut CompletionDemo, text: &str) {
    for ch in text.chars() {
        demo.chat.composer_mut().update(&key(KeyCode::Char(ch)));
    }
}
fn source(demo: &mut CompletionDemo, rows: Vec<Candidate>, complete: bool) {
    let request = demo
        .chat
        .composer()
        .completion_request()
        .expect("typed fixture route");
    demo.chat
        .composer_mut()
        .set_completion_sources(&request, &["files"]);
    demo.chat.composer_mut().complete_source(
        &request,
        "files",
        Batch {
            candidates: rows,
            complete,
            scanned: if complete { 812 } else { 240 },
            total: Some(812),
            ..Batch::default()
        },
    );
}
pub(super) fn prepare(demo: &mut CompletionDemo) {
    match demo.scenario {
        15 => {
            demo.chat.set_input("also check ");
            type_text(demo, "@");
            let request = demo
                .chat
                .composer()
                .completion_request()
                .expect("timeout fixture route");
            demo.chat
                .composer_mut()
                .set_completion_sources(&request, &["files"]);
            demo.chat.composer_mut().tick(Duration::from_secs(5));
        }
        17 | 18 => {
            type_text(demo, "!cat src/dig");
            source(demo, files().iter().map(candidate).collect(), true);
            if demo.scenario == 18 {
                demo.chat.composer_mut().update(&key(KeyCode::Tab));
            }
        }
        19..=21 => {
            demo.chat.set_input("also check ");
            type_text(demo, "@set");
            if demo.scenario == 21 {
                for _ in 0..6 {
                    demo.chat.composer_mut().update(&key(KeyCode::Left));
                }
            } else {
                demo.chat.composer_mut().update(&key(KeyCode::Esc));
                type_text(demo, "tin");
                if demo.scenario == 20 {
                    type_text(demo, " and @");
                    source(demo, files().iter().map(candidate).collect(), true);
                }
            }
        }
        22 => {
            demo.chat.composer_mut().update(&Event::Paste(
                "/x-forwarded-for is missing on staging".into(),
            ));
        }
        23 => {
            demo.chat
                .composer_mut()
                .update(&Event::Paste("x".repeat(65_510)));
            type_text(demo, " also check @set");
            source(demo, files().iter().map(candidate).collect(), true);
            demo.chat.composer_mut().update(&key(KeyCode::Tab));
        }
        24 => unavailable(demo),
        25 => {
            demo.chat.set_input("see ");
            type_text(demo, "@設計");
            source(
                demo,
                vec![
                    Candidate::new(
                        "wide",
                        "docs/設計/タイムゾーン設定の移行メモ🦀.md",
                        "docs/設計/タイムゾーン設定の移行メモ🦀.md",
                    ),
                    Candidate::new("readme", "docs/設計/README.md", "docs/設計/README.md"),
                    Candidate::new("digest", "docs/設計/digest.md", "docs/設計/digest.md"),
                ],
                true,
            );
        }
        27 => {
            demo.chat.set_input("also check ");
            type_text(demo, "@dig");
            source(demo, files().iter().take(3).map(candidate).collect(), false);
            demo.chat.composer_mut().tick(Duration::from_millis(160));
        }
        29 => alias(demo),
        _ => {
            if demo.scenario == 26 {
                let mut rows = commands();
                for row in &mut rows {
                    row.disabled = match row.value.as_str() {
                        "resume" => Some("needs a network".into()),
                        "restore" => Some("no stashes".into()),
                        _ => None,
                    };
                }
                demo.chat.composer_mut().set_completions(rows);
                type_text(demo, "/re");
            } else {
                type_text(demo, "/");
            }
        }
    }
}
fn alias(demo: &mut CompletionDemo) {
    type_text(demo, "/kill");
    let request = demo
        .chat
        .composer()
        .completion_request()
        .expect("alias fixture route");
    demo.chat
        .composer_mut()
        .set_completion_sources(&request, &["host"]);
    let mut row = Candidate::new("stop", "/stop", "stop");
    row.aliases = vec!["kill".into()];
    demo.chat.composer_mut().complete_source(
        &request,
        "host",
        Batch {
            candidates: vec![row],
            complete: true,
            ..Batch::default()
        },
    );
}
fn unavailable(demo: &mut CompletionDemo) {
    type_text(demo, "/redeploy");
    let request = demo
        .chat
        .composer()
        .completion_request()
        .expect("unavailable fixture route");
    demo.chat
        .composer_mut()
        .set_completion_sources(&request, &["release-tools"]);
    demo.chat.composer_mut().complete_source(
        &request,
        "release-tools",
        Batch {
            candidates: vec![Candidate::new("redeploy", "/redeploy", "redeploy")],
            complete: true,
            ..Batch::default()
        },
    );
    demo.chat.composer_mut().update(&key(KeyCode::Tab));
    type_text(demo, "preview");
    demo.chat.composer_mut().set_completions(commands());
    demo.chat.composer_mut().update(&key(KeyCode::Enter));
}
pub(super) fn sizing(index: usize, area: &mut Rect) {
    if index == 16 {
        area.width = area.width.min(28);
    } else if index == 25 {
        area.width = area.width.min(32);
    }
}
pub(super) fn profile(index: usize, mut p: Presentation) -> Presentation {
    match index {
        26 => p.colors = ColorMode::NoColor,
        27 => p.symbols = Symbols::Ascii,
        28 => {
            p.colors = ColorMode::TrueColor;
            p.theme = Theme::Light;
        }
        _ => {}
    }
    p
}
