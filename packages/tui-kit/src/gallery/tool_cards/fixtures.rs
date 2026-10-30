//! Maps every card board state to host-supplied safe, deterministic activity.

use crate::{
    chat::{Content, Part},
    tool_call_card::{Call, Capabilities, Output, State, ToolGroup},
};
use std::time::Duration;

const PATCH: &str = "@@ -18,2 +18,2 @@\n-  json,\n+  json, 'X-Timezone': user.timezone,\n   tz,\n";
const ERROR: &str = "✕ weekly digest › uses the user timezone\n  expected Europe/Lisbon, got UTC\n✕ daily digest › falls back to UTC\n";
pub(super) fn call(
    id: &str,
    verb: &str,
    target: &str,
    state: State,
    summary: &str,
    output: Output,
) -> Call {
    let mut call = Call::new(id, verb, target);
    call.state = state;
    call.summary = summary.into();
    call.output = output;
    call.elapsed = Duration::from_millis(900);
    call.capabilities = Capabilities {
        details: true,
        retry: true,
        copy: true,
    };
    call
}
pub(super) fn run(state: State, summary: &str, output: &str) -> Call {
    call(
        "run-1",
        "run",
        "npm test -- digest",
        state,
        summary,
        Output::Text(output.into()),
    )
}
pub(super) fn part(call: Call) -> Part {
    Part::new(&call.id.clone(), Content::ToolCard(Box::new(call)))
}
pub(super) fn diff() -> Call {
    call(
        "edit-1",
        "edit",
        "src/digest/settings.ts",
        State::Completed,
        "applied · +6 −2 · 2 hunks",
        Output::Diff(PATCH.into()),
    )
}
pub(super) fn lines(count: usize) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for i in 0..count {
        let _ = writeln!(text, "transforming ({i}) src/digest/render.tsx");
    }
    text
}
pub(super) fn group(scenario: usize) -> Part {
    let mut calls = vec![
        call(
            "search",
            "search",
            "X-Timezone",
            State::Running,
            "4 matches in 3 files",
            Output::Text(String::new()),
        ),
        call(
            "read",
            "read",
            "src/digest/settings.ts",
            State::Running,
            "212 lines",
            Output::Text(String::new()),
        ),
        run(State::Running, "6 passed", ""),
    ];
    for c in &mut calls {
        c.elapsed = Duration::from_secs(1);
    }
    if scenario == 21 {
        calls[0].state = State::Completed;
        calls[2].state = State::Failed;
        calls[2].summary = "exit 1 · 2 failed".into();
        calls[2].output = Output::Text(ERROR.into());
    }
    if matches!(scenario, 22 | 23) {
        for c in &mut calls {
            c.state = State::Completed;
            c.elapsed = Duration::from_millis(2100);
        }
    }
    if scenario == 22 {
        calls[2].state = State::Failed;
        calls[2].summary = "exit 1 · 2 failed".into();
        calls[2].output = Output::Text(ERROR.into());
    }
    if scenario == 24 {
        for i in 3..10 {
            calls.push(call(
                &format!("call-{i}"),
                "run",
                &format!("task {i}"),
                State::Running,
                "",
                Output::Text(String::new()),
            ));
        }
    }
    Part::new(
        "concurrent",
        Content::ToolGroup(ToolGroup {
            id: "parallel-1".into(),
            revision: 0,
            calls,
        }),
    )
}
pub(super) fn parts(scenario: usize) -> Vec<Part> {
    if matches!(scenario, 20..=24) {
        return vec![group(scenario)];
    }
    let text = "PASS digest-settings.test.ts (6)\nPASS profile-sync.test.ts (4)";
    let output = if matches!(scenario, 6 | 16 | 17 | 37) {
        ERROR.repeat(14)
    } else {
        text.into()
    };
    let mut c = match scenario {
        1 => run(State::Queued, "", ""),
        2 => { let mut c = run(State::Running, "", ""); c.elapsed = Duration::from_secs(4); c },
        3 => { let mut c = run(State::Running, "", &lines(214)); c.elapsed = Duration::from_secs(12); c },
        4 => call("waiting", "edit", "src/digest/settings.ts", State::Waiting("your approval · answer below".into()), "", Output::Text(String::new())),
        5 => call("waiting", "read", "src/digest/settings.ts", State::Waiting("the index lock".into()), "", Output::Text("Lock owner: another local fixture".into())),
        6 | 16 | 17 | 37 => run(State::Failed, "exit 1 · 2 of 14 failed", &output),
        7 => { let mut c = run(State::Cancelled("you pressed esc".into()), "", ""); c.elapsed = Duration::from_secs(12); c },
        9 => call("read", "read", "src/digest/settings.ts · 14–18", State::Completed, "5 of 212 lines", Output::Code { text: "export function digestHeaders(user) {\n  return {\n    'Content-Type': 'application/json',\n  }\n}".into(), first_line: 14 }),
        10 | 29 | 33 => diff(),
        11 | 25 | 26 => { let mut c = call("build", "run", "npm run build", if scenario == 25 { State::Running } else { State::Completed }, "built · 1,204 lines", Output::Text(lines(if scenario == 25 { 849 } else { 1204 }))); c.elapsed = Duration::from_secs(41); c },
        12 => { let mut c = call("unknown", "tracker", "", State::Completed, "returned 1.2 kB · shown as text", Output::Text("APP-22: digest header".into())); c.source = "tracker".into(); c.name = "get_issue".into(); c.arguments = "id: \"APP-22\"".into(); c },
        27 => call("binary", "read", "assets/digest-header.png", State::Completed, "read", Output::Unavailable("binary, 2.1 MB".into())),
        30 => run(State::Failed, "exit 1 · 2 failed", ERROR),
        41 => run(State::Completed, "text only", "\u{1b}[31mred\u{1b}]8;;https://example.com\u{7}label\u{1b}]8;;\u{7}\r\u{009b}2J"),
        42 => run(State::Completed, "no output", ""),
        43 => run(State::Failed, "error · line wraps", &"A long error with Unicode 界 e\u{301} 👩‍💻 ".repeat(40)),
        _ => run(State::Completed, "6 passed · 2 new", text),
    };
    if matches!(scenario, 6 | 16 | 17 | 30 | 37) {
        c.elapsed = Duration::from_millis(3200);
    }
    if scenario == 34 {
        c.capabilities = Capabilities::default();
    }
    if matches!(scenario, 0 | 28 | 31 | 32) {
        return vec![
            Part::new(
                "intro",
                Content::Markdown("Checking how profile sync sends it first.".into()),
            ),
            part(call(
                "search",
                "search",
                "X-Timezone",
                State::Completed,
                "4 matches in 3 files",
                Output::Text(String::new()),
            )),
            part(diff()),
            part(run(State::Failed, "exit 1 · 2 of 14 failed", ERROR)),
            part(call(
                "lint",
                "run",
                "npm run lint",
                State::Running,
                "",
                Output::Text(String::new()),
            )),
        ];
    }
    vec![part(c)]
}
