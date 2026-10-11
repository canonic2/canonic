# Current package architecture

This is the implemented package/module map. [System design](system-design.md)
owns architectural rules and proposed capability boundaries; component and
feature specs own detailed behavior and limits. [Inventory](inventory.md)
records implementation gaps; [Verification](verification.md) records evidence.

## Package and consumers

`canonic-tui` is an unpublished Rust library with an optional `gallery`
feature and `tui-gallery` binary. Drone, Gate and Shield declare optional local
path dependencies behind their `tui` features. There is no root Cargo workspace
or registry publication pipeline. Dependency compilation is distinct from a
consumer UI rollout.

## Implemented module map

| Source | Current responsibility | Owning contract |
| --- | --- | --- |
| [editor](../src/editor.rs) | Unicode text, input, transactions, atomic payload markers, layout and view | [Editor](components/editor.md) |
| [picker](../src/picker.rs) | Compatibility index picker and stable-ID ChoicePicker | [Option Picker](components/option-picker.md) |
| [dialog](../src/dialog.rs) | Validated stack, scoped tickets, timed arrival/guards/notices, caution qualification and measured chrome | [Dialog](components/dialog.md) |
| [commands](../src/commands.rs) | Descriptors, search/help views, bindings and remaps | [Command Palette](components/command-palette.md) |
| [completion](../src/completion.rs) | Ranked source snapshots, opaque queries, selection, progress/retry and fixed-height view | [Completion Menu](components/completion-menu.md) |
| [composer](../src/composer.rs) | Draft/submission, queue, recall/stash, payloads, completion, saved state and editor conflicts | [Composer](components/composer.md), [Completion Menu](components/completion-menu.md) |
| [chat](../src/chat.rs) | Identified rich transcript, revision/resync contracts, cached layout, anchors, loaded search/selection and fixed Composer | [Chat](components/chat.md) |
| [tool_call_card](../src/tool_call_card.rs) | Pure tool lifecycle/output model, local expansion, coalesced previews, ordered groups and opaque effect acknowledgements | [Tool Call Card](components/tool-call-card.md) |
| [run_activity](../src/run_activity.rs) | Host run snapshots, identified cancellation/options and independent transport indication | [Run Activity](components/run-activity.md) |
| [focus](../src/focus.rs) | Instance-local target orders, modal scopes and return focus | [Input/navigation](features/interaction.md) |
| [text](../src/text.rs), [theme](../src/theme.rs), [ui](../src/ui.rs) | Private sanitized text/wrapping, semantic presentation and shared rules | [Presentation](presentation.md) |
| [runtime](../src/runtime.rs) | Interactive terminal lease, polling/drawing, explicit close and foreground handoff | [Terminal hosting](features/terminal.md) |
| [gallery](../src/gallery.rs) | Deterministic component/scenario composition and host simulation | [Gallery](features/gallery.md) |
| [lib.rs](../src/lib.rs), [main.rs](../src/main.rs) | Public exports/event-rendering types and gallery argument parsing/error reporting | [System design](system-design.md) |

## Composition and side effects

A consumer or gallery owns one terminal runtime and supplies events, frames,
areas, focus, presentation and elapsed time to components. Chat composes Composer
and Tool Call Card. `chat/tools.rs` adapts semantic card rows into anchored blocks;
group members keep lifetime-scoped source slots across growth and retry insertion.
Composer uses Editor, Option Picker, Completion Menu and Dialog. Commands share descriptor/binding
values with host dispatch. Detailed limits belong to the component specs.

Pure construction/update/render does not connect agents, execute commands, read
clipboard contents or write product storage. Composer's explicit foreground
editor adapter performs host-requested process/temp-file work; runtime owns the
terminal handoff. Both boundaries are opt-in and independently report failures.

## Existing limitations

Broader run activity and review, resource browsers, native inline hosting and broad platform acceptance
remain separately tracked work. No current module map implies those targets exist.
The terminal guard is process-local; it does not establish cross-process ownership
or forced-signal recovery. See the [component checkpoints](components/README.md)
and [feature index](features/README.md).
