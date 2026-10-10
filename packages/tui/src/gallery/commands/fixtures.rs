//! Supplies deterministic command definitions, remaps and scopes for design review.

use crate::{
    commands::{
        Command, Commands,
        bindings::{Binding, Stroke},
        remaps::{Origin, Remap},
    },
    event::{KeyCode, KeyModifiers},
};

pub(super) const NAMES: [&str; 12] = [
    "palette",
    "filter",
    "filter-key",
    "unavailable",
    "no-match",
    "keys",
    "keys-custom",
    "conflict",
    "pending",
    "pending-hints",
    "pending-miss",
    "empty",
];

fn stroke(code: KeyCode, modifiers: KeyModifiers) -> Stroke {
    Stroke::new(code, modifiers)
}
fn ctrl(c: char) -> Stroke {
    stroke(KeyCode::Char(c), KeyModifiers::CONTROL)
}

type FixtureRow = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<char>,
);
const ROWS: [FixtureRow; 14] = [
    (
        "handoff",
        "copy handoff",
        "summary and references for another agent",
        "session",
        Some('h'),
    ),
    (
        "lens",
        "switch lens",
        "chat, diff, log, handoff",
        "session",
        Some('l'),
    ),
    (
        "actions",
        "actions on or off",
        "apply edits and commands, or only log them",
        "session",
        Some('a'),
    ),
    (
        "reference",
        "copy reference",
        "this session's address",
        "session",
        Some('y'),
    ),
    (
        "stop",
        "stop session",
        "draft and history are kept",
        "session",
        Some('k'),
    ),
    (
        "editor",
        "open in $EDITOR",
        "long message, your editor",
        "session",
        Some('e'),
    ),
    (
        "sessions",
        "go to sessions",
        "every session, needs-you first",
        "go to",
        None,
    ),
    (
        "palette",
        "commands",
        "browse available commands",
        "go to",
        None,
    ),
    ("help", "keys", "every command and its keys", "go to", None),
    (
        "rename",
        "rename branch",
        "rename the example branch",
        "workspace",
        None,
    ),
    (
        "publish",
        "publish branch",
        "needs an open remote",
        "workspace",
        None,
    ),
    ("theme", "theme", "dark or light colors", "appearance", None),
    (
        "tab",
        "new tab",
        "open another local view",
        "appearance",
        None,
    ),
    (
        "export",
        "export chat",
        "copy the chat as Markdown",
        "session",
        None,
    ),
];

fn descriptors(fixture: usize) -> Vec<Command> {
    let mut commands: Vec<_> = ROWS
        .into_iter()
        .map(|(id, label, description, category, key)| Command {
            id: id.into(),
            label: label.into(),
            description: description.into(),
            category: category.into(),
            aliases: vec![
                format!("/{id}"),
                if id == "rename" {
                    "/branch".into()
                } else if id == "publish" {
                    "/push".into()
                } else {
                    String::new()
                },
            ],
            disabled: (id == "publish").then(|| "needs an open remote".into()),
            bindings: key.map_or(vec![], |key| {
                vec![Binding(vec![
                    ctrl('x'),
                    stroke(KeyCode::Char(key), KeyModifiers::NONE),
                ])]
            }),
        })
        .collect();
    for (id, key) in [
        ("palette", ctrl('p')),
        ("help", stroke(KeyCode::F(1), KeyModifiers::NONE)),
        ("sessions", ctrl('g')),
        ("theme", ctrl('t')),
    ] {
        commands
            .iter_mut()
            .find(|c| c.id == id)
            .expect("fixture ID exists")
            .bindings = vec![Binding(vec![key])];
    }
    for i in 0..17 {
        commands.push(Command {
            id: format!("example-{i}"),
            label: format!("example command {i:02}"),
            category: "more".into(),
            description: "host-supplied action".into(),
            aliases: vec![],
            disabled: None,
            bindings: vec![],
        });
    }
    if fixture == 3 {
        for c in &mut commands {
            if c.category == "session" {
                c.bindings = c
                    .bindings
                    .iter()
                    .map(|b| Binding(b.0[1..].to_vec()))
                    .collect();
            }
        }
        commands[14].label = "stop every session".into();
        commands[14].description = "needs admin".into();
        commands[14].disabled = Some("needs admin".into());
        commands[14].bindings = vec![Binding(vec![stroke(
            KeyCode::Char('K'),
            KeyModifiers::SHIFT,
        )])];
    }
    if fixture == 11 {
        commands.clear();
    }
    commands
}
fn overrides(remapped: bool, fixture: usize) -> Vec<Remap> {
    let mut changes = Vec::new();
    if remapped && fixture != 11 {
        changes.push(Remap {
            id: "palette".into(),
            bindings: vec![Binding(vec![ctrl('o')])],
            origin: Origin {
                path: "example/keys.toml".into(),
                line: 3,
            },
        });
    }
    if fixture == 6 {
        changes.push(Remap {
            id: "editor".into(),
            bindings: vec![Binding(vec![
                ctrl('x'),
                stroke(KeyCode::Char('b'), KeyModifiers::NONE),
            ])],
            origin: Origin {
                path: "example/keys.toml".into(),
                line: 3,
            },
        });
    }
    if fixture == 7 {
        changes.push(Remap {
            id: "theme".into(),
            bindings: vec![Binding(vec![ctrl('t')])],
            origin: Origin {
                path: "project/keys.toml".into(),
                line: 2,
            },
        });
        changes.push(Remap {
            id: "tab".into(),
            bindings: vec![Binding(vec![ctrl('t')])],
            origin: Origin {
                path: "personal/keys.toml".into(),
                line: 7,
            },
        });
    }
    changes
}
pub(super) fn view(remapped: bool, fixture: usize) -> Commands {
    let commands = descriptors(fixture);
    let changes = overrides(remapped, fixture);
    let mut view = Commands::remapped(commands, &changes).expect("valid fixture command map");
    let scopes: Vec<_> = view
        .descriptors()
        .iter()
        .map(|c| {
            let scope = match c.id.as_str() {
                "palette" | "help" | "sessions" => "everywhere",
                "handoff" | "lens" | "actions" | "reference" | "stop" | "theme" | "tab"
                | "export" => "chat",
                "editor" => "editor",
                _ => "workspace",
            };
            (c.id.clone(), scope)
        })
        .collect();
    view.set_scopes(
        &scopes
            .iter()
            .map(|(id, scope)| (id.as_str(), *scope))
            .collect::<Vec<_>>(),
    );
    view.set_recent(&["handoff", "lens"]);
    view.set_keys(matches!(fixture, 5..=7));
    view.set_query(match fixture {
        1 => "copy",
        2 => "ctrl+x",
        3 => "stop",
        4 => "deploy",
        6 => "editor",
        7 => "ctrl+t",
        _ => "",
    });
    view
}
