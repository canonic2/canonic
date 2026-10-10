# Experience inventory and gaps

## Evidence and status

Baseline inspected on **2026-10-09**: [manifest](../Cargo.toml),
[exports](../src/lib.rs), all source modules, [usage](../docs/usage.md),
[development](../docs/development.md) and integration test modules. Structure
and acceptance conventions follow [Gate](../../gate/specs/README.md) and
[Shield](../../../private/shield/specs/README.md). This pass inspected source and
assertions; it did not rerun Rust gates or establish fresh platform acceptance.

Inventory research included direct source/documentation inspection of terminal
routes, prompt editing/completions, modal routing, permission/question flows,
session browsers, rich text/tool renderers, themes, keymaps, selection, diff
navigation, status panels and runtime cleanup. The target is independently stated.
Full application operations appear below to establish host ownership.

## Verified local foundation

| Evidence | Observed behavior and qualification |
| --- | --- |
| [Picker](../src/picker.rs) | Single choice, original-index action, explicit default, case-insensitive substring search, arrows/Enter/Esc/Ctrl-C/paste; 128-byte title, 4 KiB labels/query; backspace removes a scalar rather than grapheme |
| [Rich picker](../src/picker/choice.rs) | Slice 2a accepted/committed: stable IDs, disabled/danger/current rows, groups/details, editor-backed fuzzy filter, multiple marks, source refresh and compact view; compatibility picker stays intact |
| [Dialogs](../src/dialog.rs) | Slice 2b awaits human gate: generic caller bodies, bounded stack, top-only dispatch, return focus repair, unique tickets, replacement/clear records, close policy and clamped shared-rule view |
| [Chat](../src/chat.rs) | Plain role/text, append only to last message, status string; 100 messages, 64 KiB each, 4 KiB editor draft; submit immediately clears and host may restore |
| [Editor](../src/editor.rs) / [Focus](../src/focus.rs) | Slice 1 functional gate passed; redesign awaits renewed gate: word-wrapped source rows, selection, one-transaction paste with whole-grapheme cutting, grouped undo; independent focus orders and modal return scopes |
| Chat viewport | Grapheme/cell wrapping, follow/manual, five-row paging; `u16` row offset clamped during render; no stable anchors, IDs, structured parts or virtualization |
| [Text](../src/text.rs) | Removes controls except allowed newlines, bounds UTF-8 by scalars, grapheme wrapping, narrow replacement; strips ESC rather than parsing complete escape sequences |
| [Theme](../src/theme.rs) | Semantic ANSI/indexed/RGB/no-color profiles and ASCII symbols; terminal-default body colors; tokens for dim/accent/success/warning/error/info/surface/selection; no rich syntax/diff rendering |
| [Runtime](../src/runtime.rs) | TTY checks, process-local lease, raw/alternate/cursor/paste, startup guard/drop restoration attempts, synchronous draw/poll; no inline, explicit close result, suspend/resume or forced-signal handling |
| [Gallery](../src/gallery.rs) | Selector/picker/chat/editor; two independent editors and modal, resettable Unicode/long/limit scenarios; deterministic stream queue; busy/disconnected restores draft; tool/error fixtures are plain text |
| [Editor tests](../tests/integration/editor.rs) | Editing, bounds, selection, undo, focus removal/return, cursor/style, reviewed multiline snapshot and arbitrary Unicode properties |
| [Rich picker tests](../tests/integration/picker.rs) | Identity/default/disabled refresh, hidden marks, query undo, status isolation, group paging, compact snapshot, Unicode/narrow styles and supplied-ID properties |
| [Dialog tests](../tests/integration/dialog.rs) | Nested routing, selection-first Esc, return repair, stale/foreign tickets, atomic validation, outside pointer isolation, tiny geometry, snapshot and arbitrary stack changes |
| [Component tests](../tests/integration/components.rs) | Choice/default/cancel/search identity, paste/graphemes/bounds/safe control display, tiny rendering/focus style; property covers choice-index bounds |
| [Gallery tests](../tests/integration/gallery.rs) | Navigation/simulation, one selector snapshot, headless paths; Unix Python PTY assertions for navigation, submission, resize, Ctrl-C, raw and alternate-screen restoration |
| Consumers | Optional dependencies in [Gate](../../gate/Cargo.toml), [Shield](../../../private/shield/Cargo.toml), [Drone](../../../private/drone/Cargo.toml); no library calls found in consumer source; this is dependency readiness |

No per-chat-state reviewed snapshots, rich-content properties, inline PTY
suite or native Windows acceptance are present. Complete escape removal, stable
scroll positions, large-history behavior and cleanup-error reporting remain gaps.
Editor controls remain stored and render as safe pictures/hex; fragmented
transcript sanitation remains pending. See [Gate 1](human-testing.md#gate-1--editor-and-focus)
for the editor and [Gate 2a](human-testing.md#gate-2a--rich-option-picker) for the
rich picker. [Gate 2b](human-testing.md#gate-2b--dialogs) owns dialog acceptance.
Its seven scenarios preserve two host drafts; a dedicated PTY test verifies
nested paste, parent preservation, resize and restored terminal modes. The
picker's fourteen fixtures preserve an independent host draft while
cycling stage/dock/full-screen placement. Engineering and human evidence remain
separate.

## Complete experience inventory

“Partial” identifies related foundations, not accepted proposed behavior.
“Host” identifies operations exposed by views/intents but executed elsewhere.

| Experience | Reusable capability | Host responsibility | Baseline / spec owner |
| --- | --- | --- | --- |
| Welcome, empty screen, contextual hints | Empty/status panels and composition | Startup/configuration/session selection | Partial; [Product](product.md), [Dialogs](dialogs.md) |
| Pane/tab navigation, shortcut hints | Focus scopes, binding dispatcher/help | Available actions and chosen bindings | Missing; [Interaction](interaction.md) |
| Roles, timestamps, turn separators | Transcript blocks/metadata | Author/event normalization | Partial; [Conversation](conversation.md) |
| Markdown, links, lists, tables, code | Safe rich renderer/inspection | Opening links/loading artifacts | Missing; [Conversation](conversation.md) |
| Reasoning, summary, compaction markers | Supplied foldable blocks | Exposure and actual summarization | Missing; [Conversation](conversation.md) |
| Streaming/concurrent parts/retry | Revision updates, follow/unread/status | Transport ordering/recovery/retry policy | Partial last append; [Conversation](conversation.md), [Activity](activity.md) |
| Multiline/cursor/word/selection/undo | Unicode editor | Draft/history persistence | Partial; [Composer](composer.md) |
| Paste summaries/file/image/context chips | Atomic paste/reference summaries | Clipboard media/file reads/payload storage/model support | Partial plain paste; [Composer](composer.md) |
| Draft recall/stash/external editor | History/stash/replace-draft intents | Storage/editor process/conflicts | Missing; [Composer](composer.md), [Terminal](terminal.md) |
| Commands/aliases/skills | Completion/palette/help | Discovery/parsing/execution | Missing; [Dialogs](dialogs.md) |
| Mentions/paths/directories/ranges | Query/result and token contracts | Filesystem/IDE search/context inclusion | Missing; [Composer](composer.md) |
| Shell entry mode | Host-defined mode label/submit | Execution/authorization | Host mode; [Composer](composer.md) |
| Tool calls/output/errors/diagnostics | Cards/expandable output/progress | Invocation/taxonomy/result meaning | Plain text; [Activity](activity.md) |
| File changes/diffs/hunks | Code/diff viewer/resource tree | Diff/Git/snapshot/revert/apply | Missing; [Conversation](conversation.md), [Dialogs](dialogs.md) |
| Permission/trust/confirmation | Versioned review and safe choices | Identity/scope/expiry/grants/enforcement | Picker only; [Activity](activity.md) |
| Single/multi/typed questions | Form and review | Delivery/schema/validation | Missing; [Activity](activity.md) |
| Cancel/retry | Pending/acknowledged states/intents | Process interrupts/deadlines/reconnect | Partial; [Activity](activity.md) |
| Transcript search/timeline | Loaded search/jump browser | Unloaded search/paging | Missing; [Conversation](conversation.md) |
| Selection/copy/export | Logical ranges/payload intents | Clipboard/files/destination/privacy | Missing; [Interaction](interaction.md) |
| Session recent/pinned/rename/delete/resume | Stable row browser/confirmation | Persistence/destructive operations | Missing; [Dialogs](dialogs.md) |
| Parent/child/fork/move | Tree/navigation intents | Orchestration/workspace/session operations | Missing; [Dialogs](dialogs.md) |
| Model/provider/agent/mode/variant | Grouped browser/selection label | Discovery/auth/backend capabilities | Missing; [Dialogs](dialogs.md) |
| Context/cost/todos/files/connections | Optional summary sections | Counting/pricing/execution/MCP/LSP probes | Status string; [Dialogs](dialogs.md) |
| Custom theme/default/no-color | Semantic palette/preview | Settings/discovery/persistence | Partial; [Terminal](terminal.md) |
| Resize/Unicode/narrow/mouse | Source mapping/optional pointer routing | Terminal profile/capture selection | Partial; [Interaction](interaction.md), [Terminal](terminal.md) |
| Notices/errors/loading/attention | Clocked notices/display | OS notifications/audio/telemetry | Status string; [Dialogs](dialogs.md) |
| Debug/status/help | Inspection/help/cleanup errors | Logs/collection/redaction | Basic hints; [Dialogs](dialogs.md) |
| Suspend/editor/title | Lease transitions/capability flags | Processes/signals/title policy | Missing; [Terminal](terminal.md) |
| Inline menus/review | Shared view/normal-screen adapter | CLI fallback/results | Missing; [Terminal](terminal.md) |
| Plugins/auth/sharing/setup | Generic forms/lists/loading only | Installation/OAuth/keys/publication/configuration | Host operations; excluded from library |

## Priorities and limitations

Close editing/composition gaps before application-shaped menus. Stable IDs,
focus, viewport anchors and explicit updates precede rich conversation and async
dialogs. Tool/permission text fixtures do not establish component acceptance.

This pass measured no compatibility matrix or performance budget. No claim of
screen-reader, IME, native images, forced-kill cleanup or application rollout
follows from source inspection or buffer snapshots. [Roadmap](roadmap.md) owns
delivery and [Gallery](gallery.md) owns future evidence.
