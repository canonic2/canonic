# Experience inventory and gaps

## Evidence and status

Baseline inspected on **2026-10-09**: [manifest](../Cargo.toml),
[exports](../src/lib.rs), all source modules, [usage](../docs/usage.md),
[development](../docs/development.md) and integration test modules. Structure
and acceptance conventions follow [Gate](../../gate/specs/README.md) and
[Shield](../../../private/shield/specs/README.md). This pass inspected source and
assertions; it did not rerun Rust gates or establish fresh platform acceptance.
Chat implementation and test coverage were updated on 2026-10-11; executed checks
are recorded separately in [Verification](verification.md#chat-alignment-engineering-record--2026-10-11).

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
| [Dialogs](../src/dialog.rs) | Caller bodies, bounded stack, top-only routing, return focus/tickets, timed arrival/guard/notices, attention intents, caution policies and 36 named scenarios; human acceptance pending |
| [Chat](../src/chat.rs) | Identified rich parts/revisions/resync, immutable submission delivery, semantic content/search/copy; 100 turns and 64 KiB per turn |
| [Tool Call Card](../src/tool_call_card.rs) | Nine lifecycle states, text/code/diff/unavailable bodies, host progress/tasks/artifacts, coalesced previews, reorder by ID and acknowledged effects; 79 deterministic gallery scenarios |
| [Run Activity](../src/run_activity.rs) | Whole-run state, single outstanding cancel, completion/failure races, independent offline status, 140 ms injected animation and Chat composition |
| [Question Form](../src/question_form.rs) / [Review Queue](../src/review_queue.rs) | Bounded structured descriptors, isolated Editor fields, staged validation/review, opaque acknowledgement tickets, explicit partial cancellation, expiry/copy and host-ordered Chat focus |
| [Session Browser](../src/session_browser.rs) | Stable-ID host rows, local/remote search and paging, identified row actions, full safe delete confirmation, material races and retained Chat gallery instances |
| [Tree Browser](../src/tree_browser.rs) | Normalized host forests, stable cursor/folds, loaded filtering, lazy/paged children and identified actions; caller-owned focus/placement |
| [Code/Diff Inspection](../src/inspection.rs) | Bounded code/unified patches, logical navigation/folds/find/selection, whole-file mapping and exact host effects |
| [Permission Review](../src/permission_review.rs) | Safe defaults, numeric highlighting, explicit Enter/scope/destructive confirmation, loaded source search, exact response tickets, R retry, refusal and expiry/withdrawal/material races; mixed queue and Chat receipts |
| [Composer](../src/composer.rs) | Submission and work-control tickets with host acknowledgements; FIFO held edits/pause, history/search, stash/drop undo, typed paste/image/file atoms, shell/completion, backward-compatible autosave codec, editor conflicts and 99 gallery scenarios; host executes/persists |
| [Completion Menu](../src/completion.rs) | Prefix/word/alias/fuzzy ranking, source tickets/progress/streaming, distinct timeout warnings and Alt-R retry; Composer token routing, separate menu/draft help and 30 interactive gallery states; host discovers/executes |
| [Editor](../src/editor.rs) / [Focus](../src/focus.rs) | Slice 1 functional gate passed; redesign awaits renewed gate: word-wrapped source rows, selection, one-transaction paste with whole-grapheme cutting, grouped undo; independent focus orders and modal return scopes |
| Chat viewport | Logical message/part/source anchors, usize navigation, follow/unread, opaque paging, cached visible row projection |
| [Text](../src/text.rs) | Removes controls except allowed newlines, bounds UTF-8 by scalars, grapheme wrapping, narrow replacement; strips ESC rather than parsing complete escape sequences |
| [Theme](../src/theme.rs) | Semantic ANSI/indexed/RGB/no-color profiles and ASCII decoration; terminal-default body colors and syntax/diff/selection styles resolved by Chat |
| [Runtime](../src/runtime.rs) | TTY/capability checks, one process-local lease, raw/cursor/paste/enhanced keys, full-screen and fixed inline rows, cursor reports, alternate borrowing, output/deferred queue, explicit close and foreground release/resume; exact Unix termios restoration; host owns signals |
| [Inline composition](../src/inline_composition.rs) | Bounded ordered settled prefix, frozen exact write batches, write-once receipts, appended corrections and ambiguous-write stop; host owns terminal I/O and live rendering |
| [Gallery](../src/gallery.rs) | Selector/picker/chat/editor; independent drafts, resettable Unicode/long/limit scenarios; deterministic stream queue; busy/disconnected protects draft; 76 Chat scenarios exercise rich content, delayed identified effects and serial paging recovery |
| [Editor tests](../tests/integration/editor.rs) | Editing, bounds, selection, undo, focus removal/return, cursor/style, reviewed multiline snapshot and arbitrary Unicode properties |
| [Rich picker tests](../tests/integration/picker.rs) | Identity/default/disabled refresh, hidden marks, query undo, status isolation, group paging, compact snapshot, Unicode/narrow styles and supplied-ID properties |
| [Dialog tests](../tests/integration/dialog.rs) | Nested routing, selection-first Esc, return repair, stale/foreign tickets, atomic validation, outside pointer isolation, tiny geometry, snapshot and arbitrary stack changes |
| [Component tests](../tests/integration/components.rs) | Choice/default/cancel/search identity, paste/graphemes/bounds/safe control display, tiny rendering/focus style; property covers choice-index bounds |
| [Gallery tests](../tests/integration/gallery.rs) | Navigation/simulation, one selector snapshot, headless paths; Unix Python PTY assertions for navigation, submission, resize, Ctrl-C, raw and alternate-screen restoration |
| Consumers | Optional dependencies in [Gate](../../gate/Cargo.toml), [Shield](../../../private/shield/Cargo.toml), [Drone](../../../private/drone/Cargo.toml); no library calls found in consumer source; this is dependency readiness |

Chat snapshots, Unicode properties and native Unix search/copy/resize workflows
are present. Native Windows acceptance and full host adapters
remain separate work.
Editor controls remain stored and render as safe pictures/hex; transcript controls
are sanitized before rendering. See [Gate 1](human-testing.md#gate-1--editor-and-focus)
for the editor and [Gate 2a](human-testing.md#gate-2a--rich-option-picker) for the
rich picker. [Gate 2b](human-testing.md#gate-2b--dialogs) owns dialog acceptance.
Its 36 scenarios preserve host drafts; a dedicated PTY test verifies
nested paste, parent preservation, resize and restored terminal modes. The
picker's sixteen local choice fixtures preserve an independent host draft while
cycling stage/dock/full-screen placement. Engineering and human evidence remain
separate.

## Complete experience inventory

“Partial” identifies related foundations, not accepted proposed behavior.
“Host” identifies operations exposed by views/intents but executed elsewhere.

| Experience | Reusable capability | Host responsibility | Baseline / spec owner |
| --- | --- | --- | --- |
| Welcome, empty screen, contextual hints | Empty/status panels and composition | Startup/configuration/session selection | Partial; [Product](product.md), [Browsing](features/browsing.md) |
| Pane/tab navigation, shortcut hints | Focus scopes, binding dispatcher/help | Available actions and chosen bindings | Missing; [Interaction](features/interaction.md) |
| Roles, timestamps, turn separators | Transcript blocks/metadata | Author/event normalization | Implemented; [Conversation](components/chat.md) |
| Markdown, links, lists, tables, code | Safe rich renderer/inspection | Opening links/loading artifacts | Implemented local projection; [Conversation](components/chat.md) |
| Reasoning, summary, compaction markers | Supplied foldable blocks | Exposure and actual summarization | Implemented local projection; [Conversation](components/chat.md) |
| Streaming/concurrent parts/retry | Revision updates, follow/unread/status | Transport ordering/recovery/retry policy | Implemented identified updates and retry intents; [Conversation](components/chat.md), [Activity](features/activity.md) |
| Multiline/cursor/word/selection/undo | Unicode editor | Draft/history persistence | Partial; [Editor](components/editor.md) |
| Paste summaries/file/image/context chips | Atomic paste/reference summaries | Clipboard media/file reads/payload storage/model support | Atomic text/image chips implemented; host owns payload storage; [Composer](components/composer.md) |
| Draft recall/stash/external editor | History/stash/replace-draft intents | Storage/editor process/conflicts | Implemented with versioned saved-state codec and foreground adapter; [Composer](components/composer.md), [Terminal](features/terminal.md) |
| Commands/aliases/skills | Completion/palette/help | Discovery/parsing/execution | Palette/help and Composer slash routing implemented; host supplies skills/discovery; [Command Palette](components/command-palette.md), [Completion Menu](components/completion-menu.md) |
| Mentions/paths/directories/ranges | Query/result and token contracts | Filesystem/IDE search/context inclusion | File completion implemented; host supplies discovery; [Composer](components/composer.md) |
| Shell entry mode | Host-defined mode label/submit | Execution/authorization | Implemented shell mode and separate recall; host executes; [Composer](components/composer.md) |
| Tool calls/output/errors/diagnostics | Cards/expandable output/progress | Invocation/taxonomy/result meaning | Reusable cards/groups, supplied progress/tasks/artifacts, Chat selection, identified effects and bounded streaming; [Tool Call Card](components/tool-call-card.md), [Run Activity](components/run-activity.md), [Activity](features/activity.md) |
| File changes/diffs/hunks | Code/diff viewer/resource tree | Diff/Git/snapshot/revert/apply | Implemented local projection; [Conversation](components/chat.md), [Browsing](features/browsing.md) |
| Permission/trust/confirmation | Versioned review and safe choices | Identity/scope/expiry/grants/enforcement | Implemented local review; [Permission Review](components/permission-review.md) |
| Single/multi/typed questions | Form and review | Delivery/schema/validation | Implemented; [Question Form](components/question-form.md) |
| Cancel/retry | Pending/acknowledged states/intents | Process interrupts/deadlines/reconnect | Partial; [Activity](features/activity.md) |
| Transcript search/timeline | Loaded search/jump browser | Unloaded search/paging | Chat loaded search/anchors/paging implemented; timeline browser separate; [Conversation](components/chat.md) |
| Selection/copy/export | Logical ranges/payload intents | Clipboard/files/destination/privacy | Chat block/Markdown/code copy implemented; full export host-owned; [Interaction](features/interaction.md) |
| Session recent/pinned/rename/delete/resume | Stable row browser/confirmation | Persistence/destructive operations | Implemented reusable view; [Session Browser](components/session-browser.md) |
| Parent/child/fork/move | Tree/navigation intents | Orchestration/workspace/session operations | Session fork/move and reusable lazy trees implemented; [Session Browser](components/session-browser.md), [Tree Browser](components/tree-browser.md) |
| Model/provider/agent/mode/variant | Grouped browser/selection label | Discovery/auth/backend capabilities | Missing; [Browsing](features/browsing.md) |
| Context/cost/todos/files/connections | Optional summary sections | Counting/pricing/execution/MCP/LSP probes | Implemented reusable view; [Status Panels & Notices](components/status-panels-and-notices.md) |
| Custom theme/default/no-color | Semantic palette/preview | Settings/discovery/persistence | Partial; [Terminal](features/terminal.md) |
| Resize/Unicode/narrow/mouse | Source mapping/optional pointer routing | Terminal profile/capture selection | Partial; [Interaction](features/interaction.md), [Terminal](features/terminal.md) |
| Notices/errors/loading/attention | Clocked notices/display | OS notifications/audio/telemetry | Implemented reusable view; [Status Panels & Notices](components/status-panels-and-notices.md) |
| Debug/status/help | Inspection/help/cleanup errors | Logs/collection/redaction | Basic hints; [Browsing](features/browsing.md) |
| Suspend/editor/title | Lease transitions/capability flags | Processes/signals/title policy | Foreground handoff implemented; job-control/title remain host work; [Terminal](features/terminal.md) |
| Inline menus/review | Shared view/normal-screen adapter | CLI fallback/results | Missing; [Terminal](features/terminal.md) |
| Plugins/auth/sharing/setup | Generic forms/lists/loading only | Installation/OAuth/keys/publication/configuration | Host operations; excluded from library |

## Priorities and limitations

Close editing/composition gaps before application-shaped menus. Stable IDs,
focus, viewport anchors and explicit updates precede rich conversation and async
dialogs. Tool/permission text fixtures do not establish component acceptance.

This pass measured no compatibility matrix or performance budget. No claim of
screen-reader, IME, native images, forced-kill cleanup or application rollout
follows from source inspection or buffer snapshots. [Roadmap](roadmap.md) owns
delivery and [Gallery](features/gallery.md) owns future evidence.
