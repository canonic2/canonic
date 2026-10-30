//! Additional host snapshots for progress, tasks, artifacts and run acknowledgement races.

use super::fixtures::{call, diff, group, part, run};
use crate::{
    chat::Part,
    tool_call_card::{Artifact, ChildTask, Output, Progress, State},
};
use std::time::Duration;

pub(super) fn parts(scenario: usize) -> Vec<Part> {
    let mut c = match scenario {
        44 => run(State::Proposed, "", ""),
        45 => run(State::Interrupted("run interrupted".into()), "", ""),
        46 | 47 => {
            let mut c = call("index", "index", "src/", State::Running, "", Output::Text(String::new()));
            c.progress = Some(Progress { completed: 12, total: if scenario == 46 { Some(40) } else { None }, unit: "files".into() });
            c
        }
        48 => call("redacted", "fetch", "api.example.com/v2/digest token: •••", State::Completed, "200 · 1.1 kB", Output::Text("url   https://api.example.com/v2/digest\ntoken ••• redacted by host\ntz    Europe/Lisbon".into())),
        49 => run(State::Failed, "exit 1 · 2 of 14 failed", "\x1b[32mPASS\x1b[0m digest-settings.test.ts\n\x1b[31mFAIL\x1b[0m digest-schedule.test.ts\n\x1b]8;;file:///src/digest\x1b\\"),
        50 => {
            let mut c = diff();
            c.artifacts = vec![Artifact { id: "patch".into(), label: "settings.diff".into(), target: "fixture:settings.diff".into(), open_available: true }, Artifact { id: "coverage".into(), label: "coverage".into(), target: "fixture:coverage".into(), open_available: true }];
            c
        }
        51 | 52 | 58 => {
            let mut c = call("child", "task", "update digest tests", State::Running, "", Output::Text(String::new()));
            c.child_task = Some(ChildTask { id: "child-session".into(), latest_calls: vec!["edit src/digest/schedule.test.ts ✓".into(), "run npm test -- schedule ···".into()], total_calls: 2 });
            c
        }
        53 | 54 => {
            let mut p = group(20);
            if let crate::chat::Content::ToolGroup(g) = &mut p.content {
                g.calls[0].state = State::Completed;
                g.calls[1].state = State::Completed;
                g.calls[1].output = Output::Code { text: "export function digestHeaders(user) {".into(), first_line: 1 };
                if scenario == 54 { g.calls.rotate_right(1); }
            }
            return vec![p];
        }
        55 => {
            let mut c = call("build", "run", "npm run build", State::Completed, "built", Output::Text(super::fixtures::lines(1204)));
            c.capabilities.details = false;
            c
        }
        _ => run(State::Running, "", ""),
    };
    c.elapsed = Duration::from_secs(if matches!(scenario, 51 | 52 | 58) {
        41
    } else {
        9
    });
    if matches!(scenario, 56 | 57) {
        c.verb = "index".into();
        c.target = "src/".into();
        c.progress = Some(Progress {
            completed: 12,
            total: Some(40),
            unit: "files".into(),
        });
        let mut cancelled = run(State::Cancelled("you pressed esc".into()), "", "");
        cancelled.id = "lint".into();
        return vec![
            part(c),
            part(cancelled),
            part(run_with_id("proposed", State::Proposed)),
        ];
    }
    vec![part(c)]
}
fn run_with_id(id: &str, state: State) -> crate::tool_call_card::Call {
    let mut c = run(state, "", "");
    c.id = id.into();
    c
}
