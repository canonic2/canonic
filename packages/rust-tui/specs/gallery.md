# Independent gallery and verification

## Implemented dialog composition

Slice 2b has prompt, nested choice/prompt, long inspection, locked Escape,
outside dismissal, removed return target, local error, confirmation, notice,
multiline prompt and rename scenarios. Two background drafts and the dock composer
survive modal edits. Dialogs use measured centered panels. Function-key controls demonstrate push/replace/clear,
late ticket rejection and simulated outside routing. Bodies use public editor/
picker APIs and fixture-owned inspection scrolling. Dialogs use one existing
terminal owner; native mouse capture is not enabled. [Gate 2b](human-testing.md#gate-2b--dialogs)
owns human acceptance. Tests cover pure state, rendered geometry/snapshot and
an owned native macOS PTY scenario. Arrival deferral and timed removal notices
remain the separate 2b.2 gate.

## Implemented picker composition

Slice 2a exposes fourteen resettable states: default, details, disabled, danger,
filter, no-match, scrolled, multi, groups, loading, narrow, done, empty and error.
F2 cycles states; F3 inspects, F5 simulates source refresh and F6 changes stage/
dock/full-screen placement. Tab switches to an independent draft. The component
returns intents; the gallery collapses confirmed output. The host reserves one
activity row between picker and draft in every state, including `ready` after
confirmation, so the composer keeps its position. No product operation
or new terminal session runs. [Gate 2a](human-testing.md#gate-2a--rich-option-picker)
owns human acceptance. The broader expansion below remains proposed.

Status: proposed expansion of the existing feature-gated gallery. Current fixtures
and assertions are recorded in [Inventory](inventory.md). This specification
defines future acceptance, not evidence that those tests already exist.

## Development workflow

`make -C packages/rust-tui run` remains the local entry point. Catalog contains
every implemented reusable component and composed screen. Each entry identifies
its public contract, named scenarios and supported interaction modes. Direct
component/scenario entry and headless scenario listing are proposed additions
to the existing `--component` and `--list` interfaces; update reader docs when
implemented rather than advertising nonexistent options.

Open a scenario with a deterministic initial projection, explicit clock, script
of updates and simulated host acknowledgements. Reset returns exactly that state.
Pause/step/advance permits reproduction of races; live playback never prevents
typing. Inspect sanitized intents and their identity/revision in a fixture panel.
The panel is gallery instrumentation, not mandatory product UI. Switching scenario
or returning to catalog discards owned schedules, pending responses and local
state. Reopening cannot replay a stale completion.

Fixtures call the same public APIs as consumers. No parallel gallery-only widget
implementation, real agent process, account, user settings, policy grant, product
CLI, workspace Git operation or required network exists. Clipboard/external editor
and remote search use explicit simulated outcomes. Real I/O is restricted to
owned terminal acceptance fixtures with temporary artifacts.

## Required fixture inventory

Each component has default/empty/long/narrow/busy/error/cancel scenarios when
meaningful; inapplicable states are documented rather than manufactured. Themes
and sizes are orthogonal profiles, not hundreds of duplicated fixtures.

| View | Representative deterministic scenarios |
| --- | --- |
| Input/focus | Two editors, disabled/removed targets, modal return, conflicting bindings, release/repeat, timed leader |
| Editor | Combining/emoji/CJK, multiline/tabs, selection replacement, undo/redo, exact/overflow limit, CR/CRLF paste, zero area |
| Composer | Busy/disconnected, reject/accept/newer-edit race, history unsent draft, stash conflict, editor failure, stale completion/reference removal |
| Picker | Safe/no/invalid default, grouped variable rows, disabled reason, filtered reorder/removal, no match, async load/error, multi-select review |
| Conversation | Empty, long retained history, omitted older content, interleaved streaming, incomplete Markdown, old/stale revision, unread/manual anchor |
| Content | Lists/quotes/tables/links, fenced/unknown-language code, long lines/tabs, malicious controls, split/unified/binary/malformed diff |
| Activity | Parallel tools, queued/waiting/growing output, unknown tool, live connection plus tool error, cancel/completion race, retry failure |
| Review | Safe default, no selection, broad scope detail, withdrawn/expired/replaced request, response error, queued requests, draft retained |
| Questions | Required/optional, single/multi choice, disabled options, typed answer, multi-question review, stale form, cancellation |
| Dialogs | Nested/replace/clear, long scrolling body, tiny resize, removed return target, outside drag, late result after close |
| Browser/tree | Empty/loading/error, current/pinned, remote-query race, failed delete, rename conflict, removed subtree, lazy child error/cycle |
| Status/notices | Unknown context/cost, collapsed sections, narrow fallback, expired toast/actionable error, reduced motion |
| Runtime | Inline/full-screen, TTY refusal, acquisition/draw/poll/close failures, suspend/resume and restoration |

## Composed screens

1. **Inline choice/review:** ordinary preceding/following text plus shared picker
   or request; confirm/cancel/error and resize demonstrate region ownership.
2. **Conversation workspace:** transcript/composer/status, parallel activity,
   permission/question, palette/search/timeline, session browser and optional
   sidebar. Fixtures isolate each interaction then combine them with streaming.
3. **Content inspection:** file tree, code/diff body, search, selection/copy and
   nested help; change width/theme while anchored to one hunk.
4. **Disconnected/recovery:** editable retained draft, unavailable submission,
   reconnect intent, host failure then success, delayed old-session update.

These are reusable compositions, not replicas of a product's commands/backend.
Each can be instantiated without terminal I/O through public data/events/rendering.
Optional panels compose without importing the gallery or starting an event loop.

## Test boundaries

| Test layer | Meaningful evidence |
| --- | --- |
| State/action behavioral | Actual navigation/edit/submit/review traces, acknowledgement races, validation/errors, identity and focus isolation |
| Rendering / TestBackend | Readability, labeled state, cursor/hit bounds, style and reflow at zero/tiny/narrow/normal sizes; assertions accompany reviewed snapshots |
| Property | Arbitrary Unicode edits preserve UTF-8/grapheme ranges; undo round trips; navigation bounds; scroll/resize anchors; revision rejection; bounded projection/cache; tree validity |
| Real owned PTY | Encoded key/paste/resize/input delivery, raw/paste/cursor/alternate restoration, inline output region, handled cancellation/suspend and native behavior |
| Consumer boundary | Gallery-disabled library plus optional consumer compilation; later host integration tests prove actual product routing/headless invariance |

Do not snapshot every intermediate frame or test private file structure. Snapshot
representative semantic states with controlled time/IDs and accompanying assertions
for content, cursor, focus and intents. Style-aware checks complement text dumps.
Review unexpected diffs; no automatic blanket acceptance. Property models should
be independent enough to catch invalid boundaries/races, not mirror implementation.

PTY fixtures own one temporary child/session, record pre/post modes, verify emitted
control sequences and readable surrounding output, inject paste and resize, and
clean only their resources. Failure/unwind/close-error cases need targeted tests
or replaceable terminal-I/O boundary where real failure cannot be induced reliably.
SIGKILL restoration is not an acceptance requirement. Windows native harness is
a separate design/availability decision, not a Unix test enabled under emulation.

Keep one integration binary under `tests/integration/`, adding area modules only
when implemented. Unit tests cover pure internal boundaries. [Code quality](code-quality.md)
owns fmt/Clippy/nextest/doctest/docs/deny/build/coverage commands. PTY/signal tests
require host access in an agent sandbox and process-heavy suites run separately.
No additional framework or documentation-text tests are required by these specs.

## Performance measurements — proposed targets

Approve targets after first baseline; numbers below are proposed engineering
budgets, not measured behavior or promised platform support. Use release build,
record commit/toolchain, CPU/OS/terminal profile and dimensions, and distinguish
CPU layout/render from backend write and event-to-visible latency.

Fixture: 100 retained messages near the 64 KiB/message bound with mixed Unicode
and code, visible 120×40 viewport, 30 updates/second split across four parts,
plus typing; also test 40×12 reflow and rows beyond 65,535. Explicit projection
limits govern memory; total-byte/part/cache budgets must be set before acceptance.

Proposed warm CPU layout/render p95 is under 16 ms after unchanged-content caches;
input-to-next-visible-frame p95 under 50 ms on the recorded native host. Measure
at least 1,000 updates after warm-up, report p50/p95/max, cold layout and resize
cost separately, and peak retained/cache memory. Slow backend writes must not be
reported as component layout cost. An input starvation trace fails regardless
of an acceptable average. Do not broaden accepted targets from one machine.

## Acceptance criteria

- **G-01:** Every new public component/composition has catalog/direct access and
  applicable named states; it runs without product/account/network/persistent effects.
- **G-02:** Reset/pause/step/leave/reopen reproducibly controls time/updates and
  suppresses stale results; fixture action log observes public intents.
- **G-03:** New behavior has meaningful state/render/property evidence and reviewed
  snapshots; terminal contracts have real PTY/native evidence where applicable.
- **G-04:** All composed-screen scenarios demonstrate focus/draft/anchor preservation
  under streams, overlays, resize, failures and leaving; two instances remain isolated.
- **G-05:** Recorded release measurements expose cold/warm/resize/memory and input
  fairness against approved budgets, with failures/untested profiles stated.
