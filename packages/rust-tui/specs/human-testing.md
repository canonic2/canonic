# Human testing gates

Status: workflow requested on **2026-10-09**. Foundation human testing passed.
Slice 1 functional testing passed; the supplied editor/design-system redesign
reopens its human gate. The user explicitly requested a scoped commit on
2026-10-09 after visible newline/color checks. Remaining redesign gate actions
have not been reported as passed. Slice 1 redesign is committed; the user
authorized advancing on 2026-10-09. Slice 2a rich picker is human accepted and
committed; subsequent picker visual review reopened the bounded follow-up below.
Slice 2b dialogs is implemented, awaiting its human gate; slice 2c and
slices 3–13 remain unstarted. [Roadmap](roadmap.md) owns
the acceptance criteria and architecture dependency groups; this topic owns
their smaller delivery units and the human acceptance procedure.

## Delivery contract

Each numbered slice is a complete cycle:

1. Implement that slice with standalone gallery scenarios and the relevant
   composed screen. Preserve unrelated work and consumer behavior.
2. Run appropriate automated gates, inspect the diff and report actual results.
3. Provide exact runnable commands, gallery entry/scenario names, effective keys,
   numbered actions and expected visible results. The commands must work in the
   implementation being handed over. Mark any unavailable host checks explicitly.
4. Stop for the user's pass/fail confirmation. Automated tests do not substitute
   for this gate. Ask for the failing step/observed behavior if feedback is unclear.
5. If a step fails, fix the current slice and present updated verification; do
   not move to another slice or commit a knowingly failed result.
6. After explicit human acceptance, use the commit skill, rerun checks when
   changes require it, commit the reviewed package scope in `private` and `public`,
   report hashes, then begin the next slice. Nothing is pushed automatically.

One confirmation accepts only the demonstrated slice on the user's reported
terminal. New material changes after confirmation reopen the affected gate.
If engineering checks fail, the commit skill's failure handling applies; human
acceptance does not waive a failing check. Maintain the slice status here and
record commit IDs after acceptance, without claiming untested platform coverage.

Use `make -C packages/rust-tui run` from the repository root for human tests.
Catalog/scenario labels for slices 2–13 are intended deliverables. At each
handoff, replace intended labels/keys with the actual
implemented controls. Prefer one command with catalog navigation over requiring
the user to know Cargo/toolchain options. Never instruct the user to run a
nonexistent fixture flag. Automated checks and owned PTY tests remain the
implementation agent's responsibility.

## Slice sequence and human exit tests

Every gate includes resize to a narrow terminal, return to catalog, reopen to
verify reset and exit to verify ordinary terminal input/output. Where meaningful,
test dark/light presentation and preserve a draft during updates. Specific tests
below supplement those common checks. The exact keys are supplied when ready.

| Slice | Scope / roadmap group | Human test actions and expected result | Acceptance coverage |
| --- | --- | --- | --- |
| 0 | Foundation baseline | Run existing picker/chat, search/confirm/cancel, type during streaming, scroll/follow, cycle failures, resize and exit; use the checklist below | Existing behavior only; no expanded criteria accepted |
| 1 | Editor and focus / group 1 | Open Editor; insert/delete in the middle, move/select Unicode and replace text, paste multiline, undo/redo, reach limit. Open two-editor/modal fixture: typing changes only focused editor; close restores focus. Edit chat while streaming | E-01–E-03; first I-01/I-05 and A-01/A-02 evidence |
| 2a | Rich picker / group 2 | Filter grouped/disabled rows; page and confirm stable identity; mark multiple rows; refresh and preserve the independent host draft | D-02; A-03/A-04 for this view |
| 2b | Dialogs / group 2 | Open nested dialogs; Esc closes only top and restores focus; tiny resize and removed return target | D-01 |
| 2c | Commands and bindings / group 2 | Search palette/help; one selection emits one intent; test remapped/pending bindings | D-03, I-02 |
| 3 | Draft lifecycle, history and stash / group 2 | Submit with delayed acceptance/rejection, edit while pending and repeat Enter; no duplicate/lost draft. Recall history then restore unsent draft; stash/restore with overwrite/cancel/failure. Simulate external editor replacement conflict | E-04–E-05; native editor handoff deferred to slice 13 |
| 4 | Completion and references / group 2 | Accept/dismiss command/context completion; no accidental submission. Change query/cursor during delayed results; stale results never insert. Add/edit/remove references and inspect payload summaries | E-06; completion contribution to D-03/A-03 |
| 5 | Identified transcript and viewport / group 3 | Stream into multiple parts/tools while editing; scroll away, see unread, resume follow. Prepend history and resize while anchored; simulate eviction/stale update/error and inspect markers | C-01/C-03, I-03; long-content measurement contribution to C-06/G-05 |
| 6 | Rich content, selection and search / group 3 | Inspect incomplete/final Markdown, code, links/tables and unknown parts; copy logical text and simulate clipboard failure. Search/jump/timeline, reveal scope, close and restore position | C-02/C-05, I-04; complete C-06/G-05 measurements for accepted content |
| 7 | Tool activity and cancellation / group 4 | Expand parallel/growing/unknown/failed tools; distinguish tool error from disconnect. Interrupt one run, observe pending, then completion/interrupt/error races without losing draft | R-01–R-02 |
| 8 | Permissions and questions / group 4 | Review safe/no default, details and queued requests; paste/Esc never approve. Change/expire a request while open. Complete single/multi/typed questions and response-error retry; restore background draft/focus | R-03–R-05, composed P-02 |
| 9 | Sessions and resource trees / group 5 | Filter/loading/error/refresh session rows, simulate rename/delete/fork, delayed failures and stale search. Navigate lazy task/file tree, remove focused descendant and retry failed child load | D-04–D-05 |
| 10 | Status, notices and responsive panels / group 5 | Inspect unknown context/cost, errors and optional sections; shrink and reopen hidden panel. Advance notice clock; actionable error stays reachable, reduced motion stays readable | D-06; presentation contribution to T-05 |
| 11 | Diff inspection and complete composition / group 5 | Navigate files/hunks, switch unified/split, shrink/expand and copy; inspect malformed/binary/no-newline states. Combine tree/search/dialog/streaming and verify anchors/focus | C-04, full composed G-04 on implemented views |
| 12 | Inline terminal interaction and close / group 6 | Run delivered normal-screen fixture with text before/after picker/review; confirm/cancel/resize/error preserves surrounding output. Compare same view in full-screen and check exit/reacquire | T-01–T-02, P-01 |
| 13 | Native lifecycle and compatibility / group 6 | Run delivered suspend/resume/editor success/failure fixtures; verify restored modes, draft and redraw. Execute profile-specific tests on available native terminals; record unsupported/untested profiles | T-03/T-06; final compatibility evidence |

Sanitation/Unicode (**T-04**), semantic/default/no-color/ASCII/reduced-motion
presentation (**T-05**), isolation/headless boundaries (**A/P**) and gallery
coverage (**G-01–G-04**) accompany every affected slice. They are not postponed
to a final cleanup phase. Expand the per-slice checklist to include new applicable
profiles when implemented. A-01/A-02/I-01/I-05 are continuous invariants; accepting
their editor examples does not accept all future components automatically.

The sequence covers all roadmap criteria. Unresolved renderer/limit/input/native
decisions are resolved within their owning slice using concrete gallery evidence;
material choices are shown to the user before API stabilization. Deferred host
features and product rollout remain outside this implementation sequence.

## Gate 0 — runnable foundation checklist

From the Canonic repository root:

```sh
make -C packages/rust-tui run
```

1. **Catalog:** verify Option picker and Full-screen chat appear. Arrows change
   the highlight; Enter opens the chosen component.
2. **Picker:** open Option picker; its default is Continue without this server.
   Press 2 to choose Use all project servers; the fixture reports `Selected all`.
   Esc returns to catalog. Reopen: default/query reset. F2 cycles design states;
   empty choices cannot confirm. Gate 2a below covers the expanded picker.
3. **Chat submission:** open Full-screen chat. Type `hello`, Enter; your message
   appears and the simulated response streams. Type another draft while output
   arrives; typing remains responsive. A busy rejected submit restores the draft.
4. **Paste:** paste two lines into the composer. They remain a draft; paste does
   not send. Enter sends when simulation is idle. Alt-Enter inserts a newline if
   supported by the terminal. Backspace after a joined emoji removes the grapheme.
5. **Streaming and interruption:** F2 cycles ready → empty → streaming → error
   → disconnected → long text → ready. In streaming, type a draft, then Ctrl-X;
   activity stops and Interrupted fixture appears; draft remains.
6. **Disconnected:** switch to disconnected, enter a message and press Enter;
   no simulated response runs, draft returns and status says message not executed.
7. **Scrolling:** switch to long text; PageUp/PageDown scroll, End follows latest
   output. Current paging is five rows; stable reflow anchors are a later slice.
8. **Presentation and exit:** Ctrl-T toggles dark/light. Resize narrow then wider;
   view stays usable within current clipping limits. Esc returns to catalog,
   then Esc exits. Rerun and exit with Ctrl-C. Shell input, echo and cursor should
   work afterward.

Report **pass** or **fail**, with a step number and observed behavior for failures.
If possible include terminal application/OS; no screenshot or private data is
required. Baseline failures are fixed before starting slice 1. The plan changes
can be committed after this gate passes; the existing foundation commits remain
unchanged.

## Gate 1 — editor and focus

From the repository root run:

```sh
make -C packages/rust-tui run
```

1. Open **Multiline editor**. Its text starts at column 0 between title/footer
   rules, with no side borders. Tab changes focus: inactive text remains readable,
   its rules/title dim and cursor/footer disappear. Return and confirm draft,
   caret and selection survive. F3 modal traps input; Esc clears its selection
   before closing and restores the previous target.
2. Type words; Home/End and Ctrl-A/Ctrl-E move to the visible wrapped row ends.
   Ctrl-G selects all (Ctrl-Shift-A also works where encoded distinctly).
   Shift-arrows extends selection. Ctrl-W deletes a word; Ctrl-U/Ctrl-K deletes
   to logical line start/end. Ctrl-J inserts newline; standalone Enter does too.
3. Paste two lines; the footer confirms paste and text is tinted until the next
   key or three seconds. Ctrl-Z undoes the complete paste; Ctrl-Y restores it.
   Ctrl-minus/Ctrl-period are alternatives. Type a word quickly: one undo removes
   the group. Pause over a second, type more and verify a separate undo step.
4. F2 cycles focused → blurred → empty → wrap → selection → Unicode → scrolled
   → pasted → undo → near-limit → limit → paste-cut → focused. Empty shows a dim
   placeholder. Prose wraps by word, long tokens break at the edge. Unicode
   movements/deletions keep accents/CJK/joined emoji whole. Scrolled shows more
   counts on the rules; PageUp/PageDown moves by editor height. Resize narrow
   then wide, preserving text and keeping a usable cursor where space permits.
5. Near-limit shows a byte counter; limit refuses text that cannot fit. Paste-cut
   retains the fitting grapheme prefix and reports discarded bytes. Undo recovers
   the full original draft. In any fixture, Ctrl-G then paste a longer value:
   replacement stays within the budget and one undo restores draft and selection.
6. F4 cycles color profiles; test RGB only on a supporting terminal. RGB supports
   Ctrl-T dark/light roles. The selection
   is tinted in RGB/indexed and reversed in ANSI 16/no-color. Body colors follow
   the terminal. Exit then run the explicit no-color/ASCII command below; rules
   and metadata use ASCII while document Unicode remains intact.
7. Return to catalog/reopen: fixture resets. Open **Full-screen chat**, F2 twice
   for streaming; edit during output. Ctrl-J adds a newline, Enter submits when
   idle. Alt-PageUp/PageDown and Alt-End navigate transcript; plain End/Page keys
   navigate the editor. Resize and exit with Ctrl-C; ordinary shell echo/cursor
   still work.

Explicit profiles from the repository root:

```sh
make -C packages/rust-tui run CARGO_FLAGS="-- --component editor --colors ansi256"
make -C packages/rust-tui run CARGO_FLAGS="-- --component editor --colors truecolor"
make -C packages/rust-tui run CARGO_FLAGS="-- --component editor --colors truecolor --light"
make -C packages/rust-tui run CARGO_FLAGS="-- --component editor --colors none --ascii"
```

The user confirmed cyan chrome and actual fixture line breaks on 2026-10-09,
and supplied an iTerm2 screenshot showing colored focused chrome. These checks
cover the visible fixes; the other Gate 1 actions remain pending.

Report **pass** or **fail**, with the step and observed behavior for a failure.
The redesigned editor is a material change after functional acceptance, so its
human gate is reopened by the delivery contract. The user's explicit scoped
commit request authorizes recording this implementation; it does not establish
acceptance of unreported test actions. The user authorized slice 2a next. No native
Linux/Windows or modified-key encoding is inferred from pure tests.

## Gate 2a — rich option picker

From the repository root, in iTerm2 for RGB:

```sh
make -C packages/rust-tui run CARGO_FLAGS="-- --component picker --colors truecolor"
```

1. **Default:** three choices occupy five rows, without side borders or a filter.
   Continue without this server is highlighted. Enter reports `Selected skip`
   and collapses the result. Reopen and press 1: `Selected once` appears.
   These are local results; no setting changes.
2. **Rows:** F2 cycles default → details → disabled → danger → filter → no-match
   → scrolled → multi → groups → loading → narrow → done → empty → error.
   Details has context/source labels. Disabled skips row 2; pressing 2 is inert.
   Danger starts on a safe row and requires explicit navigation to choose its
   destructive row. F3 inspects without choosing. Loading/error/empty Enter is
   inert. F2 continues cycling after confirmation.
3. **Filter:** reach filter (four F2 presses from default). Ctrl-G then paste
   `example29`: one result has detail `updated 29 hours ago`; Enter reports
   `Selected branch-29`. Repeat with Ctrl-Z/Ctrl-Y undo/redo. Digits edit the query.
   Esc clears it first while retaining picker height; Esc again returns to catalog.
   No-match displays its own message and cannot select in single mode.
4. **Multiple:** reach multi (seven F2 presses from default). One row is marked.
   Space toggles the highlighted mark; Alt-A toggles matching enabled rows. Type
   a filter: hidden marks remain included in the marked count. Enter reports
   `Confirmed [...]` in source order, including hidden marks. An empty marked set
   is valid explicit confirmation for this nonempty source.
5. **Paging/refresh:** scrolled starts at the last of 31 rows. Home/End/Page keys
   move without wrapping. Groups have dim headings that count in paging. Resize:
   highlight stays visible; details disappear before labels truncate. F5 removes
   the highlighted ID and chooses a surviving safe highlight without confirming.
6. **Host separation:** Tab focuses the independent draft; type/paste and Ctrl-J
   for a newline, then Tab back. Query edits affect only the filter. F2 preserves
   the host draft. F6 cycles stage/dock/full-screen placement preserving picker
   state. These are gallery controls within one terminal session, not an inline
   terminal adapter or picker-owned focus behavior.
7. **Presentation/exit:** F4 cycles profiles; Ctrl-T changes RGB polarity. Selected
   keys reverse/bold, disabled rows dim, body uses terminal defaults. Test the
   no-color/ASCII command below. Return/reopen resets fixtures; Ctrl-C exits and
   ordinary shell input/echo/cursor work.

```sh
make -C packages/rust-tui run CARGO_FLAGS="-- --component picker --colors none --ascii"
```

Report **pass** or **fail**, with the step and observed behavior for failures.
No commit or dialog implementation follows until this gate passes.

Slice 2a engineering: formatting, strict Clippy, 34 all-feature tests (including
native macOS PTY), one doctest, API docs and dependency audit passed. The
gallery-disabled library passed 29 tests; release build passed. Gate, Shield and
Drone optional `tui` features compiled offline, with lockfiles restored byte-for-
byte after temporary resolution. Reviewed the compact snapshot and six actual
TestBackend style/geometry previews. Duplicate dependency warnings remain allowed
by auditing. Native Linux/Windows and human modifier-key encoding are unverified.

## Picker visual follow-up — 2026-10-09

The picker uses default foreground, bold and no dim attribute for its selected
enabled label. Number keys are centered in three cells with one plain band cell
before each label; both highlight edges
leave one clear cell, and details have a two-cell gap. The gallery reserves a
persistent activity row between picker and composer in all fixture states.
The user explicitly authorized a scoped checkpoint commit on 2026-10-09.
Remaining visual checks are separate from that commit authorization.
Package checks passed: formatting, strict Clippy, 47 tests (including three native
PTY scenarios), one doctest, generated API documentation and dependency checks.
The tests cover centered numbers, clear highlight edges, ANSI 16 heading weight,
detail-width thresholds and activity-row/composer stability across all fourteen
fixtures and confirmation.

Diagnosis on the macOS host: an owned PTY capture shows SGR 39 (default
foreground), SGR 1 (bold), and SGR 27 (reverse off) before the selected label;
the title also receives SGR 1. An additional owned PTY test verifies default
foreground, bold and no dim on selected labels and bold on titles in all four
color modes, including selection transitions. ANSI 16/no-color retain the
documented reverse fallback; indexed/RGB label text is not reversed.
Saved iTerm2 profiles enable a custom Bold color
that is darker than Foreground. The user's shell-only comparison on 2026-10-09
reproduced the gray bold text outside the app, confirming this terminal behavior.
The user explicitly accepts it as a terminal-profile limitation and does not want
iTerm2 settings changed. The selected-label color concern is closed with this
accepted limitation; other visual checks remain separate.
iTerm2's [Colors documentation](https://iterm2.com/documentation-preferences-profiles-colors.html)
describes this override. Terminal preferences have not been modified.

```sh
make -C packages/rust-tui run CARGO_FLAGS="-- --component picker --colors truecolor"
```

The confirmed shell-only reproduction was:

```sh
printf '\033[0;39mnormal text  \033[1mbold text\033[0m\n'
```

The bold sample turns gray under the user's iTerm2 profile. Accept that rendering
for this gate without a terminal-settings change or application workaround.
Navigate to Use this server and confirm bold emphasis. Detail remains muted;
both outer cells stay outside the
band; each number is centered without moving between rows and has one plain
band cell between its block and label. The longest label has two blank cells before
detail; shrinking drops details rather than squeezing that gap. F4 to ANSI 16:
focused title stays bold. Between the picker footer and message title, confirm
`! needs you` in warning yellow and `· 1–3` in muted color; the picker footer also
uses `1–3`. Enter confirms: this row becomes `ready`, and the message
title stays at the same height. F2 cycles all states with that row reserved.
F6 changes placement while preserving state. Report
pass/fail for this follow-up separately from the dialog gate. Native emulator
edge padding/contrast behavior remains subject to this visual check.

## Gate 2b — dialogs

Design acceptance is pending alignment with the official
[Dialog design](../design/dialog/dialog.html). Its sheets/panels, content-driven
height, fill/inset treatment, faded background, footer actions, title trails,
waiting/removal activity states and boxed ANSI 16/no-color fallback supersede the
current gallery presentation. The steps below check existing stack mechanics;
passing them alone does not accept the official design. See the
[design contract](dialogs.md#official-dialog-design) for the remaining scope.

From the repository root, in iTerm2 for RGB:

```sh
make -C packages/rust-tui run CARGO_FLAGS="-- --component dialog --colors truecolor"
```

1. **Prompt:** the modal sits over two background drafts. Ctrl-G then paste two
   lines: only its draft changes and nothing submits. Ctrl-Z/Ctrl-Y undo/redo;
   Ctrl-J adds a newline. Tab/Shift-Tab stays in the modal. Ctrl-G then Esc clears
   selection without closing; a second Esc closes and returns focus to background
   one. Type there, Tab to background two and verify independent editing.
2. **Nested:** F3 opens a prompt; edit it, F3 opens its child, edit the child.
   Esc closes only the child; parent text/caret survive. Enter returns the parent
   text as a local result. Background drafts remain untouched while obscured.
   F2's nested fixture opens a picker over a prompt: default Cancel is safe;
   choosing returns the choice ID and leaves its parent available.
3. **Fixtures:** F2 cycles prompt → nested → long → locked → outside → removed-
   return → error. Long inspection scrolls by arrows/PageUp/PageDown/Home/End;
   End exposes row 59. Enter acknowledges only its layer. Locked ignores Escape;
   Enter returns local text. Error ignores Enter, and F6 replaces it with a usable
   retry prompt; no real operation runs.
4. **Return repair:** F5 removes background one's saved target. Close all layers:
   focus returns to background two; typing changes only that draft. The removed-
   return fixture demonstrates the same fallback. F2 preserves draft contents
   while resetting each fixture's scope.
5. **Lifecycle:** F3 nests; F6 replaces only the top. F7 reports `Rejected stale
   result; draft unchanged` for its old ticket. F8 clears all layers and reports
   the count; background drafts remain. F9 simulates an outside left press:
   ordinary fixtures stay open; the outside fixture dismisses only the top.
   This tests routing policy; the gallery does not enable native mouse capture.
6. **Geometry/profiles:** resize narrow, tiny and back. Preferred 72 × 14 cells
   clamp to the viewport; at fewer than three available modal rows, the body
   takes all rows without chrome. Keyboard dismissal/result remains available.
   F4/Ctrl-T and the command below exercise no-color/ASCII, with terminal-default
   body colors and shared heading/footer rules. Native key encodings depend on
   the terminal; unsupported function/modifier keys should be reported.
7. **Reset/exit:** close layers, Esc returns to catalog, reopen resets fixtures.
   Ctrl-C exits even with nested layers; ordinary shell echo/input/cursor work.

```sh
make -C packages/rust-tui run CARGO_FLAGS="-- --component dialog --colors none --ascii"
```

Report **pass** or **fail**, naming the step and observed behavior. The user
explicitly authorized a scoped checkpoint commit on 2026-10-09; dialog design
acceptance remains pending. Commands/bindings (2c) wait for this gate.

Slice 2b engineering: formatting, strict Clippy, 43 all-feature tests (including
two native macOS PTY scenarios), one doctest, API docs and dependency auditing
passed. Gallery-disabled testing passed 36 tests; release build passed. Gate,
Shield and Drone optional TUI features compiled offline with existing lockfiles
restored byte-for-byte. Reviewed dialog geometry and selector snapshots. Both
repositories' scoped diff checks and local Markdown links passed. Duplicate
dependency warnings remain allowed by auditing; native Linux/Windows, human
modifier/function-key encodings and native mouse capture are unverified.

## Acceptance record

| Slice | Engineering | Human | Commit / next action |
| --- | --- | --- | --- |
| 0 | Passed during foundation commit; recorded in roadmap | Passed on 2026-10-09; user reported it works, with visual design deferred | Plan committed: private `d623657`, public `e32517d` |
| 1 | Redesign checks recorded below | Functional editor/focus and visible newline/color fixes passed on 2026-10-09; remaining redesign actions pending | Explicitly authorized commit: private `41ee25b`, public `735103b`; user authorized next slice |
| 2a | Checks recorded after Gate 2a below | Accepted on 2026-10-09; user requested next slice | Committed: private `f3e7579`, public `17e53d5` |
| 2b | Engineering recorded below | Pending official design alignment | Scoped checkpoint commit explicitly authorized on 2026-10-09; keep design gate pending |
| 2c, 3–13 | Not started | Not presented | Implement in sequence after previous accepted commit |

Slice 1 engineering: `make -C packages/rust-tui check` passed formatting, strict
Clippy, 25 all-feature tests including native macOS PTY, one doctest, API docs
and dependency auditing. `cargo nextest run --locked --no-default-features`
passed 21 library tests; `make -C packages/rust-tui build` passed release build.
Gate/Shield/Drone `cargo check --offline --features tui` passed with temporary
lock resolution; their existing lockfiles were restored byte-for-byte to keep
this slice scoped to Rust TUI. Consumers need lock resolution when adopting the
polling backend dependency; this is compilation evidence, not a product rollout.
Auditing allows duplicate hashbrown/syn/thiserror versions. Markdown link and
both repositories' diff checks passed. Native Linux/Windows and human modifier
key encodings remain unverified.
