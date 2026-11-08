# TUI Kit

Build terminal interfaces and guided command-line flows in Rust.

`canonic-tui-kit` provides editors, pickers, chat views, dialogs and review forms,
with shared keyboard behavior, themes and terminal ownership. A local gallery
lets you try the components before integrating them into an application.

Components return actions to their host. Your application owns execution,
storage, agent connections and authorization; opening the gallery performs
none of those product operations.

## Try the gallery

TUI Kit is unpublished and built from source. Consumers require Rust 1.97 or
newer, and the gallery needs an interactive terminal. This checkout pins its
own development toolchain.

From the repository root:

```sh
make -C packages/tui-kit run
```

Choose a component with the arrow keys and Enter, then select a scenario.
The gallery uses local fixture data and needs no AI account. Esc returns through
the pickers; Ctrl-T switches themes and F10 opens the active component's
scenarios. Some components handle Esc and Ctrl-C locally; see
[Gallery controls](docs/usage.md#gallery) for their exit and cancellation keys.

The gallery also supports listing scenarios without an interactive terminal.
See [Usage](docs/usage.md) for component and scenario arguments.

## Use the library

From a sibling Rust package, add a versioned local dependency:

```toml
[dependencies]
canonic-tui-kit = { version = "0.5.0", path = "../tui-kit" }
```

Adjust the path to your checkout's `packages/tui-kit` directory. Rust code imports
the crate as `canonic_tui_kit`.

For ordinary CLI questions, use the [shared prompt host](docs/cli-prompts.md).
For a custom application, follow [Local consumers](docs/usage.md#local-consumers)
to acquire a terminal lease, render components and dispatch input. Importing the
library does not enter terminal modes or start an event loop. Plain and
structured noninteractive output stay with the host.

## Choose a component

| Task | Components and guide |
| --- | --- |
| Ask for choices or review a change | [CLI prompts](docs/cli-prompts.md), [Question Form](docs/usage.md#question-form), [Permission Review](docs/usage.md#permission-review) |
| Write and edit text | [Multiline editor](docs/usage.md#multiline-editing-and-focus), [Composer](docs/usage.md#composer-workflows), [Completion Menu](docs/usage.md#completion-menu) |
| Present conversations and tool work | [Chat and gallery](docs/usage.md#gallery), [Tool Call Card](docs/usage.md#tool-call-card), [Session Browser](docs/usage.md#session-browser) |
| Navigate and inspect content | [Tree Browser](docs/usage.md#tree-browser), [Code and diff inspection](docs/usage.md#code-and-diff-inspection) |
| Compose an application shell | [Dialogs](docs/usage.md#dialogs), [Commands and keyboard help](docs/usage.md#commands-and-keyboard-help), [Status panels](docs/usage.md#status-panels-and-notices), [Inline composition](docs/usage.md#inline-composition) |

The [usage guide](docs/usage.md) owns detailed controls, lifecycle behavior and
consumer responsibilities.

## Development

[Development](docs/development.md) covers formatting, lint, tests, snapshots and
terminal verification. Select checks individually; gallery use does not require
running coverage or installing a separate command.

## License

TUI Kit is subject to the [Canonic Proprietary License](LICENSE).
Third-party components retain their own licenses and notices.
