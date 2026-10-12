# Verification and acceptance

This topic owns evidence interpretation and the current engineering summary.
[Code quality](code-quality.md) owns executable package checks;
[Human testing](human-testing.md) owns runnable review steps, human acceptance
and historical gate/commit records. [Roadmap](roadmap.md) owns delivery order.
Component/feature specs own the acceptance requirements; neither evidence nor
delivery tables redefine them.

## Independent checkpoints

Track each checkpoint for a named scope and revision:

| Checkpoint | Evidence required | Does not establish |
| --- | --- | --- |
| Specification complete | Behavior, states, keys, implementation design, host boundary, acceptance and gaps reviewed | Implementation or accepted visuals |
| Design resolved | Layout, feedback, responsive/fallback behavior and interactions settled in the public contract | Engineering or human acceptance |
| Functionality complete | All in-scope behavior implemented; remaining host duties explicit | Passing checks or consumer rollout |
| Engineering verified | Dated commands/results and applicable state/render/property/native/consumer checks | Human acceptance or untested platform coverage |
| Human accepted | Explicit reported pass for demonstrated steps on the reported terminal | Acceptance of later material changes or other hosts |

Use partial or pending when any in-scope requirement lacks evidence. A checkpoint
commit is not human acceptance. Tests found in source are not a new test run.
These records were carried forward during the specification reorganization;
Markdown changes do not rerun or refresh Rust engineering evidence.

## Tree Browser engineering record — 2026-10-12

Scope: reusable supplied file/task forests, bounded topology normalization,
stable cursor/folds, loaded ancestor filtering, lazy/root/paged discovery,
identified primary/custom actions, caller-owned focus and responsive placement.
Chat has opt-in Ctrl-X F and a shared Files palette entry. The gallery retains
the original Chat separately from its inspection fixture and supplies 66 live
scenarios; the registry now has 640 named states plus
eight profiles for each of twelve components.

Host `make -C packages/tui check` passed 327 tests and four doctests, formatting,
strict Clippy, private-item API documentation and dependency auditing.
Twenty-nine component tests cover navigation/filter/paste, refresh/removal/
reparenting, topology/limits, lazy/root/page failure/retry, cascade/collapse,
foreign/stale/duplicate/malformed/materially changed results, bounded concurrent
requests, disabled actions, focus, all sixteen color/glyph/theme combinations,
tiny rendering, extension-preserving clipping, per-row narrow status, full reasons,
authoritative error recovery, retry bookkeeping cleanup and no-color attributes/
ASCII pending chrome. Mixed physical-row paging, undiscovered capacity waiting,
delivered attempt counts, capacity recovery and manual collapse during bulk
expansion have direct regression coverage.
Two gallery tests verify all 66 named states, eight inspected snapshots, F10
retention, file/task panel focus, filter cursor ownership, delayed/no-ack behavior
and Chat caret/undo/reading/Ctrl-X F. Inactive inspection is rendered through an
in-memory backend; it never acquires a terminal or claims the active filter cursor.
The macOS native PTY test
verifies lazy loading/failure/retry, bracketed filter paste, Chat return/caret/undo,
resize, scenario navigation, cooked input and exact terminal attributes after
exit. The native check consumes restored cooked input before comparing attributes,
as the existing terminal tests do; macOS can briefly set its pending-input flag.

Gallery-disabled `cargo nextest run --locked --no-default-features` passed 265
tests. The release library/gallery build passed. Gate and Shield optional `tui`
compilation passed offline with locked disposable manifests. Drone's copied lock
needed the existing TUI Markdown dependencies; offline resolution and compilation
passed only in its disposable fixture. Original consumer manifests and lockfiles
were unchanged; this is compilation compatibility, not consumer execution or a
successful locked check against Drone's original lock.

All 480 public local links/anchors resolved. Public/private whitespace checks,
all final Rust source hashes, snapshot update state and unchanged existing staging
were verified. New source/spec/test files belong to public only. Private design
files and generated instructions were preserved.

Human visual acceptance, native Windows/input methods, inline hosting and actual
host filesystem/Git/task adapters remain separate. The adjacent gallery view is
existing Chat with supplied inspection fixture content, not a standalone Code/Diff
Inspection implementation. Use [Tree Browser review](human-testing.md#tree-browser-review).
No commit, publication, installation or consumer rollout occurred.

## Session Browser engineering record — 2026-10-12

Scope: reusable dedicated browser, stable host-ordered IDs/groups/parents,
local and remote search/paging, independently ticketed row operations, safe
host-worded deletion and opt-in Chat entry. The gallery supplies 72 live scenarios,
bringing the named registry to 574 entries plus eight profiles per component.
Caller-owned Chat instances retain their documents, caret/selection, undo,
queues and logical reading/unread state across return and actual open results.

Final host `make -C packages/tui check` passed 295 tests, four doctests,
formatting, strict Clippy, private-item API documentation and dependency auditing.
The release library/gallery build passed. Gallery-disabled
`cargo nextest run --locked --no-default-features` passed 236 tests.

Twenty-five component tests cover atomic descriptor/material validation,
stable selection/removal, title/message matching, query/discovery/paging races,
explicit retries, concurrent/hidden rows, pending limits, failed rename drafts,
action availability, precise scope hints, single-use/foreign/stale tickets and
exact host-supplied results. Rendering covers every color/symbol/theme combination,
tiny/narrow areas, complete deletion wording, ASCII fork markers and long data.
Two gallery tests exercise all named states and retained F10 navigation; eight
snapshots were inspected. The new native macOS PTY test verifies isolated search
paste, destructive arming/paste disarming, failure, mid-draft caret/undo return,
Ctrl-X O, resize, cooked input and exact terminal attributes after exit.
Existing native terminal flows also passed in the full suite.

Gate and Shield optional `tui` compilation passed offline with locked disposable
manifests. Drone's copied lock required the existing TUI Markdown dependencies
`pulldown-cmark` and `unicase`; resolution and compilation passed offline solely
in its disposable fixture. Workspace consumer manifests and lockfiles remained
unchanged. This establishes compilation compatibility, not consumer execution
or a successful locked check against Drone's existing workspace lock.

Public local links resolved 461 references before this evidence update. Public
and private whitespace checks passed; all new source/spec/test files are excluded
from private. Existing staging and private design repairs were preserved.
No commit, publication, consumer rollout or Tree Browser implementation occurred.

Human visual acceptance and broader native/platform acceptance remain pending.
Use the [Session Browser review](human-testing.md#session-browser-review).
Actual discovery, storage, session lifecycle and execution remain host-owned.

## Question Form and Permission Review engineering record — 2026-10-11

Final scope: both reusable components, opaque single-use host acknowledgement
tickets, one host-ordered review queue, above-dock Chat composition and receipt
rendering. Question Form was completed and checked before Permission Review.
The gallery adds 52 Question Form and 56 Permission Review live scenarios,
bringing the named registry to 502 entries, plus eight profiles per component.
Gallery host effects remain deterministic simulations.

Host `make -C packages/tui check` passed 267 tests, four doctests, formatting,
strict Clippy, private-item API documentation and dependency auditing using the
package-pinned toolchain. `make -C packages/tui build` passed. Gallery-disabled
`cargo nextest run --locked --no-default-features` passed 211 tests. Gate, Shield
and Drone optional `tui` compilation passed offline using disposable manifests
and lockfiles; their workspace sources and lockfiles were preserved.

Ten Question Form and fifteen Permission Review behavioral tests cover bounded
descriptors and whole field insertion rejection, staged validation/cancellation,
deliberate numeric/Enter and exact-scope/destructive confirmation, explicit retry,
refusal, replacement, expiry, withdrawal and foreign/stale/duplicate tickets.
Mixed queue order/capacity and undrained-intent invalidation are checked. Chat
checks preserve the exact Composer document, caret, selection, mode and undo,
logical anchors/follow state and unread output. Long receipt titles cannot hide
the actual acknowledged outcome. Rendering exercises tiny/narrow areas, long
resources/choices/details/scopes and independent color/symbol/theme profiles.
Four gallery tests exercise the live states; twelve snapshots were inspected.

Two new native macOS PTY flows passed alongside the existing terminal checks.
They cover isolated multiline Question Form paste, ignored Permission Review
paste, explicit scope confirmation/Back, failed-action retry, expiry and late
acknowledgement, draft/caret restoration, resize and retained F10 navigation.
The harness drains shutdown output, verifies cooked input and compares exact
terminal attributes after exit.

Public local links resolved 443 references before this evidence update. Both
repositories passed whitespace checks; all 31 new source/spec/test files belong
only to public, and private design repairs remain untouched. No staging, commit,
publication or consumer rollout command was run. Concurrent configuration,
Shield and Workbench changes were preserved; Session Browser was not implemented.

Human visual acceptance and broader native/platform acceptance remain pending.
Use the [Question Form review](human-testing.md#question-form-review) and
[Permission Review review](human-testing.md#permission-review) for acceptance.
Actual delivery, permission authority and external effects remain host-owned.

## Question Form engineering record — 2026-10-11

Before beginning Permission Review, the complete Question Form gate passed
248 all-feature tests, four doctests, formatting, strict Clippy, API docs and
dependency auditing with host access. The release library/gallery build passed.
Nine behavioral tests cover descriptor/field bounds, Unicode validation, staged
answers, exact tickets, cancellation/partial delivery, queue timing/order and
Composer document/caret/selection/undo plus unread preservation. Two gallery
tests exercise 52 live states; five snapshots were inspected. The native macOS
PTY flow verifies isolated multiline paste, deliberate submission, acknowledgement,
mid-draft caret return, undo, resize, cooked input and terminal-mode restoration.
The PTY harness drains shutdown output before checking exit.

Gallery-disabled nextest passed 195 tests. Gate, Shield and Drone optional `tui`
compilation passed offline using disposable manifests and lockfiles; workspace
consumer lockfiles remained unchanged. Public local links resolved 410 references.
Both repositories passed whitespace checks. No staging command was run; concurrent
Shield/configuration changes outside this scope were left untouched.

Human visual acceptance and broader native/platform acceptance remain pending.
The [Question Form review](human-testing.md#question-form-review) provides steps.

## Gallery profiles and Run Activity engineering record — 2026-10-11

Scope: named Command Palette, Completion Menu and Dialog profiles apply once
on fixture opening and allow subsequent F4/Ctrl-T switching. Compact waiting
and failed cancellation preserve available action keys. Run fixtures inject
elapsed host time, keep reduced-motion decoration static, and preserve readable
Activity outside an options panel. Tool Call Card supplies 79 scenarios,
including twenty Run Activity scenarios; the complete registry has 394 entries.

Host `make -C packages/tui check` passed 236 tests, four doctests, formatting,
strict Clippy, API documentation and dependency auditing. The release build
passed; gallery-disabled nextest passed 186 tests. Behavioral rendering checks exercise component-body profile changes,
disabled options, compact hints, advancing elapsed seconds and an uncovered
Activity row. Existing native macOS PTY checks passed. Public local links and
both repositories' whitespace and ownership checks passed. These changes add
no public signatures; the recorded Composer consumer compilation remains
applicable. This record preserves
the distinction between engineering checks and pending human visual/platform
acceptance; Question Form and Permission Review implementation remain pending.

## Chat alignment engineering record — 2026-10-11

Scope: 76 interactive Chat scenarios, operation-specific pending acknowledgements
and failures, original Markdown copy, full table/patch inspection, serial paging
across both directions, durable newer-load failure/retry, quiet notes, host
suggestions and narrow/fallback search. The gallery retains paging and copy as
independent simulated host requests rather than overwriting an outstanding ticket.

Host `make -C packages/tui check` passed 233 tests, four doctests, formatting,
strict Clippy, private-item API documentation and dependency auditing. The release
library/gallery build passed; gallery-disabled nextest passed 185 tests. New
behavioral tests cover duplicate/cross-direction paging, fresh retry identities,
stale completion, pending action labels, refusal, source copy and draft retention.
All added gallery states have meaningful rendering assertions, including delayed
inspection and simultaneous earlier-loading/copy acknowledgement.

Native macOS PTY checks passed, including delayed copy/link refusal, explicit
retry and host confirmation, caret return, resize, cooked-mode input and exact
terminal-attribute restoration. Existing search, selection, picker return, tool
cards, dialogs and external-editor terminal checks also passed. Public local
links resolved 401 references across 33 Markdown files before this evidence update;
both Gate repositories passed whitespace and public/private ownership checks.
These changes preserve Chat's public signatures; the consumer compilation evidence
after Composer's expanded contracts remains applicable.

Human visual acceptance, native Linux/Windows and real consumer clipboard,
opening, inspection and storage execution remain pending. Question Form's public
contract is written; its implementation is outside this verified Chat scope.

## Design alignment engineering record — 2026-10-10

Verified scope: shared profile changes, Editor kill/yank/limit/selection behavior,
Option Picker ranking/bounds/refresh behavior, extended Tool Call Card metadata
and states, reusable artifact selection, and whole-run cancellation/connection
composition. Tool Call Card has 77 deterministic scenarios, including eighteen
Run Activity scenarios. The full host `make -C packages/tui check` passed
200 tests, four doctests, formatting, strict Clippy, private-item API documentation
and dependency auditing. Existing native macOS PTY tests passed, including card
details, draft/caret return, resize, picker navigation and terminal restoration.
The release library/gallery build passed. Gallery-disabled nextest passed
161 tests. The public documentation check resolved
382 local links and anchors; both Gate repositories passed whitespace checks.
Ownership checks place source/specs in public and design references in private.

Six Run Activity tests cover single-use cancellation, completion races, retry,
stale/foreign acknowledgement, disconnection, injected motion, draft/queue
preservation and Escape selection priority. Extended card/gallery checks cover
progress, safe escape markers, host reordering, child tasks, artifacts and all
additional run states. Gate, Shield and Drone optional `tui` consumer compilation
passed offline with disposable manifests after the expanded public APIs;
workspace consumer lockfiles were preserved. This is compilation evidence, not
consumer adapter execution.

Human visual acceptance, native Linux/Windows and broader keyboard/assistive
acceptance remain pending. Dialog timing, remaining handoff changes and the new
Question Form/Permission Review are outside this verified implementation scope.

## Dialog alignment engineering record — 2026-10-10

Verified scope: timed arrival and FIFO ownership, the 300 ms key guard, app-owned
two-press closure, attention intents, removal/replacement/clear notices, exact-name
and timed irreversible confirmation, plus artifact/options host composition.
The gallery supplies 36 named Dialog scenarios. Host `make -C packages/tui check`
passed 205 tests, four doctests, formatting, strict Clippy, private-item API docs
and dependency auditing. The release library/gallery build passed. A fresh
gallery-disabled nextest run passed 165 tests. Gate, Shield and Drone optional
TUI compilation passed offline with disposable manifests after the Dialog API
changes; workspace lockfiles were preserved.

The native macOS PTY test waits for observable guard expiry, closes one nested
layer with two Ctrl-C presses without quitting, restores parent focus and verifies
exact terminal attributes on exit. Behavioral coverage exercises queued-body
ownership on clear, withdrawn arrivals, stale tickets and confirmation intent
qualification. Gallery effects remain deterministic simulations. Human visual
acceptance, native inline hosting and Linux/Windows acceptance remain pending.
Later Command Palette and other handoff changes require their own verification.

## Command Palette alignment engineering record — 2026-10-10

Verified scope: prefix/word-start/alias ranking after exact effective keys,
captured opening height, empty scopes and complete-key-map evidence, unavailable
and conflict wording, bounded profile rendering and timed input in the gallery.
The palette preserves its dock above the draft and centers horizontally at up to
72 columns. All 25 scenarios exercise public command behavior with simulated
host effects. Host `make -C packages/tui check` passed 210 tests, four doctests,
formatting, strict Clippy, private-item API docs and dependency auditing.

Native macOS PTY coverage verifies the opening guard, first Ctrl-C arming,
second Ctrl-C closing without quitting, draft preservation and exact terminal
restoration. Rendering coverage verifies that a boxed palette clears caller
attributes inside its panel while preserving the surrounding backdrop. The
release library/gallery build passed; gallery-disabled nextest passed 169 tests.
The strengthened border assertion also passed a focused rerun. All 391 local
links and anchors resolved; both repositories passed whitespace checks, and
ownership checks preserve public source/specs and private design references.
Gate and Drone optional TUI compilation passed using disposable manifests. Shield's
current consumer tree fails independently in `src/policy/compiled.rs`: its
`matches` and `work` helpers pass `Arc<PikeVM>` where `PikeVM` is expected.
The refreshed disposable manifest includes current consumer dependencies;
workspace sources and lockfiles remain untouched by this check. Human
visual acceptance, host execution and Linux/Windows acceptance remain pending.

## Completion Menu alignment engineering record — 2026-10-11

Verified scope: alias ranking, captured opening height, distinct five-second
timeout warnings, source labels, partial-source insertion and Alt-R retry,
separate activity/footer hints, unavailable-command recovery and 30 interactive
gallery scenarios. Ctrl-R retains Composer history search. Host
`make -C packages/tui check` passed 217 tests, four doctests, formatting, strict
Clippy, private-item API documentation and dependency auditing. The release
library/gallery build passed; gallery-disabled nextest passed 175 tests.

Behavioral coverage checks fresh retry identities, retained successful sources,
stale and repeated events, F1 help without draft loss, command removal before
invocation, byte-limit refusal, disabled reasons and whole-grapheme clipping.
Four updated rendering snapshots were reviewed and accepted. Native macOS PTY
coverage verifies Alt-R retry, Ctrl-R history search, draft preservation, picker
return, resize and exact terminal restoration. Gallery host effects remain
deterministic simulations.

Gate, Shield and Drone optional TUI consumer compilation passed with refreshed
disposable manifests. Drone's copied lockfile required offline resolution in the
temporary directory before its locked check. Shield's earlier compilation blocker is resolved in its current tree;
its existing unused-import warning is outside this TUI change. Consumer checks
preserve workspace sources and lockfiles. The local documentation check resolved
390 links and anchors across 32 Markdown files before adding this record's links.
Both Gate repositories passed whitespace checks, and ownership checks preserve
public source/specs and private design references. Human visual acceptance,
native Linux/Windows and real consumer discovery/execution remain pending.

## Composer alignment engineering record — 2026-10-11

Verified scope: identified host-confirmed stop/interrupt/background controls,
completion races, acceptance-aware queue resumption, original-payload recovery
on blank queue saves, five-second failed-save retries, explicit restore failures,
guarded editor conflicts, late editor results and undoable returns. Typed
paste/image/file atoms survive recall, stashing, storage and unchanged external
editor tokens. Schema 2 decoding retains compatibility with schema 1 snapshots.
Chat keeps queue rows directly above one activity row and the fixed dock. Shell
previews fold logical output after twenty lines, retaining the real first three
and last two lines even beyond the layout budget; copying retains the full loaded
source. The gallery has 99 named Composer entries covering 97 distinct initial
states. Named presets allow live F4 color and Ctrl-T theme switching.

Host `make -C packages/tui check` passed 227 tests, four doctests, formatting,
strict Clippy, private-item API documentation and dependency auditing. All forty
added Composer frames exercise live public APIs. Reviewed snapshots reflect
the fixed queue/activity order, separate completion/draft hints and retained
narrow Escape help. Native macOS PTY checks cover cancellation failure/retry,
host confirmation, retained drafts/carets, completion retry/history routing,
picker restoration, resize and exact terminal restoration.

The release library/gallery build passed during this alignment. Gallery-disabled
nextest passed 182 tests after the public work-control and typed-atom contracts.
Gate, Shield and Drone compiled offline with optional `tui` using disposable
manifests and the pinned package toolchain; workspace source and lockfiles were
preserved. Offline dependency resolution was confined to temporary verification
directories. Documentation links, ownership and both repository whitespace diffs
passed. Human visual acceptance, native Linux/Windows, clipboard formats, IME
coverage and real consumer storage/execution remain separate pending acceptance.

## Tool Call Card engineering record — 2026-10-10

Implemented scope: public lifecycle/output/group contracts, reusable cards and
groups, Chat selection/anchors/search and identified copy/details/retry, plus
44 deterministic gallery scenarios. `make -C packages/tui check` passed formatting,
strict all-target/all-feature Clippy, 179 tests, four doctests, private-item API
documentation and dependency auditing. Seventeen card/Chat behavioral tests cover
lifecycle/revision guards, member updates, coalescing/final flush, exact omissions,
source safety, preview/group folding, capability gating, identified effects,
retry history, loaded search and draft/caret/anchor preservation. Four headless
gallery tests exercise simulated effects and navigation; five reviewed rendering
snapshots cover main, selected failure, capped expansion, narrow diff and NO_COLOR
diff. The native macOS PTY workflow covers expansion, details search/copy, return
to the draft cursor, picker return, resize, quit and exact terminal restoration.

The release library/gallery build passed with `make -C packages/tui build`.
Gallery-disabled nextest (`cargo nextest run --locked --no-default-features`)
passed 143 tests. The final card regression rerun without gallery passed all
seventeen card tests. Gate, Shield and Drone compiled offline with optional `tui`
using disposable manifests/lockfiles; workspace lockfiles were not changed.
Gate's existing timing/dead-code warnings remain outside this component's scope.
All 374 local README/docs/spec links and anchors resolved. Public/private ownership
and both repositories' whitespace checks passed; internal design work stayed private.

Human visual acceptance, consumer execution/clipboard/permission adapters,
native Linux/Windows and broader terminal/assistive acceptance remain pending.
The [Tool Call Card review](human-testing.md#tool-call-card-review--complete-local-activity)
provides runnable human steps. Dialog timing and Question Form are separate work.

## Completion Menu engineering record — 2026-10-10

`make -C packages/tui check` passed formatting, strict Clippy, all 132 tests,
two doctests, API documentation and dependency auditing. Dedicated behavioral
and property checks cover matching/ranking, disabled navigation, generation
rejection, streaming order/selection, debounce/loading/timeout, retry, guesses,
insertion budgets and editor token boundaries. The native macOS PTY trace covers
insert-then-invoke, argument delivery, source retry, resize, quit, cooked input and
exact terminal restoration. Gallery checks render all 15 states at tiny and
standard sizes and assert a fixed dock, display-only argument hints and back
navigation. Main, narrow, long-description and partial-failure layouts have
reviewed render snapshots; the semantic-color main view was inspected visually.

`make -C packages/tui build` passed the release library/gallery build.
Gallery-disabled nextest passed all 104 tests. Gate, Shield and Drone passed
offline compilation checks with their optional `tui` feature using disposable
manifests; their workspace lockfiles remained byte-for-byte unchanged.
Human visual acceptance, native Linux/Windows acceptance and real consumer source
discovery/execution remain separate from these local engineering results.

## Composer engineering record — 2026-10-10

Engineering verification on 2026-10-10: `make check` passed all 114 tests,
formatting, strict Clippy, one doctest, API documentation and dependency auditing.
The native Unix tests cover acknowledgement/recall/resize/quit, cooked foreground
handoff and private editor-file ownership. Rendered screens were reviewed for
queue edits, history, stash, editor conflicts, file insertion and paste chips.
`make build` passed the release library/gallery build. Gallery-disabled nextest
passed all 88 tests. These checks establish the reusable component and explicit
host adapters; gallery host work remains simulated.
Gate, Shield and Drone also passed offline checks with their optional `tui`
feature. Their original lockfiles were preserved byte-for-byte.

The evidence applies to the implemented reusable workflows and explicit host
adapters. Standalone Completion Menu, rich Chat targets and real consumer
execution/storage/clipboard integration are outside that verified scope.
Human acceptance remains in [Composer review](human-testing.md#gate-3a--submission-lifecycle).

## Chat engineering record — 2026-10-10

Scope: the rich reusable Chat component, fixed Composer composition and 51
interactive gallery scenarios. Tests cover identified interleaving/resync,
stale/foreign callbacks, loaded semantic search, folded content, original-source
copy, full-inspection intents, reflow/paging anchors, unread/dock geometry,
safe raw content, Unicode/ASCII and pathological cache bounds. Six snapshots
cover answer, wide-table fallback, folded search, selected code, actions-off and
the pane; Completion Menu snapshots reflect Chat's indentation/wrapping and
reserved unread row.

The macOS Unix PTY workflow covers code copy acknowledgement, loaded search,
draft-caret restoration, picker return, resize, Ctrl-C, unchanged native mouse
capture and exact terminal-attribute restoration. Physical keyboard/emulator
acceptance and native Windows coverage remain separate.

`make -C packages/tui check` passed formatting, strict all-target Clippy,
157 all-feature tests, three doctests, private-item API documentation and
dependency auditing. Gallery-disabled nextest passed 126 tests. The release
library/gallery build passed. Gate, Shield and Drone optional TUI compilation
passed offline using disposable manifests/lockfiles; workspace lockfiles stayed
unchanged. This is dependency compilation, not consumer UI rollout.

Recorded release measurement: macOS 15.7.9, x86_64 Intel Xeon W-2140B at 3.20 GHz,
Rust 1.99.0, ANSI16 TestBackend at 100×40, 100 turns containing 550,000 bytes of
Unicode prose. Cold construction plus first draw took 279.25 ms. Mean warm
draw time over 25 frames was 1.22 ms with 19,900 cached rows and no additional
parses/layouts. This includes TestBackend construction/rendering, excludes
physical terminal writes, and is not a p95/input-latency or portable guarantee.
The broader performance campaign remains in [Gallery](features/gallery.md#performance-measurements--proposed-targets).

Public Markdown links/anchors and private-design separation passed; all 36 Chat
design links resolve. Public/private TUI diffs passed whitespace checks.
Human visual acceptance remains pending;
the [Chat review](human-testing.md#chat-review--complete-local-conversation)
provides the complete component workflow. Clipboard/open/agent/history effects
in the gallery are simulated, not consumer rollout evidence.

## Foundation engineering record — 2026-10-09

Foundation engineering checks passed during the scoped package commits
`33fe70a` (private) and `2a6bc0f` (public): formatting, strict Clippy, ten
all-feature nextest tests including the Unix PTY test, seven gallery-disabled
tests, doctest, API docs, dependency audit, release build, consumer optional
feature compilation and Markdown/diff checks. Dependency auditing reported
allowed duplicate-version warnings. Foundation human acceptance passed on
2026-10-09; the user deferred visual refinement. These results do not accept
expanded capability contracts or establish visual design acceptance.

## Evidence and review locations

- [Human acceptance record](human-testing.md#acceptance-record) distinguishes the
  Editor/Picker functional gates, their visual follow-ups and pending Dialog,
  Command Palette and Composer reviews.
- [Component checkpoints](components/README.md#completeness-checkpoints) summarize
  current scopes without upgrading partial work to complete.
- [Inventory](inventory.md) links source/test evidence and identifies gaps.
- [Gallery coverage](features/gallery-designs.md) records executable scenarios
  and pending coverage; harness controls are outside product layout acceptance.

## Code and diff inspection engineering record — 2026-10-12

Scope: reusable read-only Inspector and deterministic Chat/tool/tree hosts on
macOS x86_64, Rust 1.99.0. The focused run passed 18 tests, including native
search/paste/copy failure-retry, Chat return, resize and exact terminal
restoration. Source/control/Unicode/profile/tiny/selection/fold/host-race tests,
all 51 live gallery scenarios and ten inspection snapshots were reviewed.

Visual review found full Editor chrome hiding the one-row find field. Inline
field rendering and a visible-text/cursor regression test passed before the
complete gate run. Final results:

- `make check`: formatting, strict Clippy, **343 tests**, **4 doctests**, private
  API documentation and cargo-deny advisories/bans/licenses/sources passed.
- `cargo nextest run --locked --no-default-features`: **278 tests** passed,
  including native editor handoff. Native suites used host PTY access.
- `make build`: release library/gallery passed; the release `--list` inventory
  exposes all 51 inspection scenarios and eight additional profiles.
- Gate and Shield `tui` feature compilation passed locked/offline in temporary
  manifest/lock copies. Drone passed with offline lock resolution in its copy;
  its existing working lock lacks the already-used Markdown dependencies.
  All six original consumer manifests/locks remained byte-identical.
- Source hashes remained unchanged through final validation. Both Gate
  repositories' whitespace checks and 848 local file/anchor links passed.
  The unrelated original staging remained byte-identical.

Tests cover exact/foreign/duplicate/stale/wrong-kind acknowledgements, malformed
patch/whole-file validation, numbered excerpts, folds/hidden copy/search,
smart case and Unicode mapping, bounded dense matches, physical page overlap,
selection changes during copy, control text, tiny rectangles and presentation
profiles. Ten new inspection snapshots and the changed tree snapshot were
reviewed. Native evidence covers search/paste/copy failure and retry, retained
Chat selection/draft/caret, reopen position, resize and terminal restoration.

Human visual acceptance, broader native hosts and actual product integration
remain separate. No product loading/clipboard/editor/Git work, publication or
consumer rollout is established by these library/fixture checks.

## Status Panels and Notices engineering record — 2026-10-12

Implemented the [Status Panels & Notices contract](components/status-panels-and-notices.md):
supplied section states/facts, stable identities/folds, exact action acknowledgements,
explicit notice time, severity priority, set-aside errors and retained caller focus.
The gallery exposes 27 live states plus eight shared profiles. Six snapshots were
reviewed for sidebar, narrow status, notices, failure, ASCII and review placement.

- `make check`: formatting, strict Clippy, **366 tests**, **4 doctests**, API docs
  and dependency advisories/bans/licenses/sources passed.
- `cargo nextest run --locked --no-default-features`: **297 tests** passed,
  including native editor handoff. PTY suites ran with host access.
- `make build`: release library/gallery passed. Release `--list` exposes all 27
  status scenarios and eight profiles.
- Gate and Shield optional `tui` compilation passed locked/offline with Rust 1.99
  in disposable manifests pointing at their actual library and binary sources.
  Drone passed after offline resolution of its existing missing Markdown
  dependencies in the temporary lockfile. Original consumer manifests/locks
  remained unchanged; this is compilation evidence, not product rollout.

Twenty-three additional tests cover stable navigation/folds/removal, atomic invalid
updates, exact/foreign/stale/duplicate acknowledgements, disabled and repeated
actions, pending/failure expiry, monotonic clock properties, persistent set-aside
errors, long reasons, clipping, profiles and caller caret restoration. Review/run
and Composer overlays retain attention priority. A 12,000-line reason remains
scrollable in a tiny viewport without allocating offscreen physical rows.
The native Unix PTY exercises status/notice entry, action failure/retry/success,
draft editing/undo, resize and exact terminal-mode restoration.

Final Rust source hashes remained unchanged through validation.
Both Gate repositories passed whitespace review; local Markdown
links/anchors and the renamed private design indexes were checked. Human visual
review, broader native platforms, inline hosting and real host adapters remain
separate acceptance work.

For behavior changes, run the owning package checks and record observed results
with scope, date and platform. Use real terminal evidence for terminal contracts.
For Markdown-only changes, check local links/anchors, private-design separation
and each affected Gate repository's diff. Do not add tests that assert prose.
