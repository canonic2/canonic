# Human testing gates

Status: complete components are the delivery unit, as requested on 2026-10-10.
The historical gates below retain their other component acceptance records.
Composer implements its complete local workflow; host execution, storage keys,
clipboard formats and native human visual acceptance remain consumer checkpoints.

## Tree Browser review

Human acceptance is pending. Run the gallery with `--component tree-browser
--scenario main` and review files/tasks, navigation, loading and profiles.

1. Open and enter folders with separate Right presses; close and climb with
   separate Left presses. Toggle with Space, expand with `*`, collapse with `-`.
2. Filter `schedule`; confirm ancestors remain and Escape restores saved folds.
   F8 must preserve the node by ID or cancel a materially stale action; F9 moves
   selection to a surviving ancestor. `bad-data` warns without drawing loops.
3. In `unloaded`, Right requests children; F6 fails, Right/Enter retries, F5 loads.
   Move or collapse while waiting: a late reply caches children without reopening.
   `paging` retains its page row while the host replies.
4. Enter an enabled file/task, inspect pending wording, then F5/F6. Open marks
   change only after acknowledgement. Disabled actions retain their reasons.
5. Tab between panel/main view; resize through 100/64/40 columns. Check tiny,
   deep paths, middle-cut extensions, long errors via Alt-PageDown, ASCII,
   NO_COLOR and light profiles. Unfocused trees keep an outline.
6. Escape from `full-screen` returns the original Chat draft/caret/undo. Ctrl-X F
   and the F1 Files command reopen through the same host entry; F10 retains state.

Record visual defects separately from engineering evidence. Native Windows,
input methods, inline hosting and actual filesystem/agent adapters remain separate.

## Session Browser review

Human visual acceptance remains pending. Run:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component session-browser --scenario main"
```

1. Current marker, group headings and fork indentation are distinct from the
   cursor. Enter on the current row returns to Chat. Ctrl-X O opens the dedicated
   browser again; F1 from Chat offers the `sessions` palette command.
2. Navigate, page and search. In `search`, title matches precede message matches;
   Search all sends only on Enter. F5 returns remote results. End requests their
   next page. F6 shows failure, and R (after leaving the query field) retries a
   failed page. Changing the query invalidates old results.
3. Inspect `loading`, `empty`, `load-failed`, `no-matches`, `refreshed`, `removed`
   and `long-list`. Loading/errors stay visible while scrolling; cursor identity
   survives refresh, and removal chooses the next/previous row with a reason.
4. A discovers offered actions/reasons. R selects the original title; type,
   paste, undo and cancel. `rename-limit` preserves over-limit text; save is
   disabled. `rename-refused` retains the field and reason. F5 is required before
   authoritative names, fork IDs, moved projects or pinned ordering change.
5. D starts on Keep. 2 highlights Delete; Enter arms, another Enter sends.
   Up/Down, paste, three seconds or material replacement disarm. F6 restores the
   row and reason; D starts the full check again. Inspect `last-deleted` and use
   PageDown in `long-delete` to read all host loss wording.
6. Exercise `concurrent`, `pending-return`, `no-ack`, `delayed`, `material-changed`,
   `action-removed` and the stale/foreign/duplicate fixtures. Other rows remain
   usable while waiting; no view action creates a real session or grant.
7. In `before-chat`, note the caret between the space and "the PR", then open
   with Ctrl-X O, search and return. Insert and undo at the same caret. Check
   `switched`, `switch-back`, `queued-drafts` and `streaming`; each Chat retains
   its draft, queue, reading position and own unread output.
8. Compare 40, 64 and 100 columns, tiny/short areas, long titles/previews and
   destinations. Review all eight shared profiles, NO_COLOR pending/error text
   and reduced motion. Escape and current actions stay reachable. F10 returns
   through retained searchable scenario navigation without resetting an edit.

Report the terminal, size and profiles reviewed. Engineering and automated native
PTY checks do not establish human visual or wider platform acceptance.

## Permission Review

Human visual acceptance remains pending. Run:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component permission-review --scenario main"
```

1. Enter initially does nothing. 1 highlights Allow once; Enter sends. F5
   acknowledges before an allowed receipt appears. Composer's mid-draft caret
   returns exactly. F10 opens the retained searchable scenario picker.
2. In `safe-default`, only Deny starts selected. In `disabled`, the reason is
   readable and Enter sends nothing. Paste into `main` or `search`; the whole
   paste is dropped and neither choice nor Composer changes.
3. Tab opens details; PageDown reaches the remainder. Ctrl-F types a literal
   search; Enter visits matches without responding. Escape ends search, hides
   details, then dismisses. Check `many-resources`, `long-details`, `command`
   and `network`; all safe host resources/source remain inspectable.
4. 2 then Enter opens the scope Dialog on Back. Enter backs out. Reopen, choose
   2 and Enter: sending includes exact scope. F5 records it word for word.
   Check `scope-back`, `scope-sending`, `allowed-session` and `long-scope`; PageDown
   reveals every scope word. No grant file is written.
5. `destructive` needs Enter twice within three seconds. `armed`, `disarmed`
   and `arm-expired` expose timing/movement. `scope-destructive` also requires
   the destructive second Enter inside the exact-scope check.
6. In `failed`, Enter cannot resend; R sends the same material with a fresh
   attempt. F6 fails, F7 refuses/disables the attempted choice, F8 expires and
   F9 replaces/clears it. F12 submits a stale acknowledgement. Inspect
   `delayed-ack`, `foreign-ack`, `changed-scope` and `expired-sending`.
7. Compare `denied`, `skipped`, `dismissed`, `expired` and `withdrawn` receipts.
   Withdrawal holds the active explanation until Escape opens the next review.
   `mixed-queue`, `next-request`, `queue-reordered` and `queue-withdrawn` follow
   host order with one active review. `waiting-open` waits one second or Ctrl-G.
8. `streaming`, `draft-restored`, `anchor-restored`, `linked-card` and
   `card-resumed` preserve draft/viewport and show host-linked card meaning.
   Inspect `narrow-details`, `narrow-scope`, `short`, `tiny`, all eight profiles
   and `reduced-motion`. F4/Ctrl-T remain live after opening any profile.

## Question Form review


Human visual acceptance remains pending. Run:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component question-form --scenario main"
```

1. Choose Audience with a number; mark Channels with Space, then Enter. Type
   Notes with Ctrl-J newline or paste; Tab opens Review. Inspect each answer,
   use Up/Down then Enter to edit, return, and Enter to send. F5 acknowledges;
   no receipt appears before it. The mid-draft Composer caret returns unchanged.
2. F10 opens the retained scenario picker. Exercise `single`, `multiple`,
   `required-missing`, `limit-reached`, `too-long`, `something-else`,
   `other-cleared` and `other-undo`. Disabled rows explain why they cannot answer.
3. In `cancel-with-answers`, Keep answering is selected. Escape keeps every field.
   `partial-offered` requires a deliberate offered partial submission; F5 accepts.
   `partial-submitting` locks keys. `partial-accepted` names the partial receipt.
4. Inspect `submitting`, `no-ack`, `delayed-ack`, `failed`, `refused`,
   `foreign-ack` and `stale-ack`. F6 fails, F7 refuses, F12 supplies a stale result.
   Enter retries deliberately with a fresh ticket; repeated pending Enter is inert.
5. F8 expires; Y copies retained answers through a simulated acknowledgement.
   F9 replaces the material revision; old answers are copyable, never answers to
   the replacement. Inspect `copy-failed`, `expired-copy` and `resource-limit`.
6. `waiting-open` opens after one second of quiet or Ctrl-G. `queue`,
   `queue-reordered`, `queue-withdrawn` and `streaming` preserve draft and active
   focus. Output accumulated during review remains unread when the panel closes.
7. Inspect `long-choices`, `long-prompt`, `narrow-review`, `narrow-field`, `tiny`
   and all named profiles. F4 changes color capabilities; Ctrl-T toggles light.
   Confirm Enter/Escape remain usable and selection does not depend on hue.


## Delivery contract

Each complete component or explicitly bounded feature is a delivery unit;
numbered gates below are its review checklists. The cycle is:

1. Implement that component or feature with standalone gallery scenarios and the relevant
   composed screen. Preserve unrelated work and consumer behavior.
2. Run appropriate automated gates, inspect the diff and report actual results.
3. Provide exact runnable commands, gallery entry/scenario names, effective keys,
   numbered actions and expected visible results. The commands must work in the
   implementation being handed over. Mark any unavailable host checks explicitly.
4. Stop for the user's pass/fail confirmation. Automated tests do not substitute
   for the applicable human gate. Ask for the failing step/observed behavior if feedback is unclear.
5. If a step fails, fix the current delivery unit and present updated verification; do
   not move to another delivery unit or commit a knowingly failed result.
6. Commit only when explicitly requested or authorized; use the commit skill,
   rerun checks when changes require it, and report hashes for the reviewed scope
   in the owning repositories. Commit authorization does not imply human acceptance.
   Nothing is pushed automatically.

One confirmation accepts only the demonstrated scope on the user's reported
terminal. New material changes after confirmation reopen the affected gate.
If engineering checks fail, the commit skill's failure handling applies; human
acceptance does not waive a failing check. Maintain human review status here and
record commit IDs after acceptance, without claiming untested platform coverage.

Use `make -C packages/tui run` from the repository root for human tests.
Catalog/scenario labels for slices 2–13 are intended deliverables. At each
handoff, replace intended labels/keys with the actual
implemented controls. Prefer one command with catalog navigation over requiring
the user to know Cargo/toolchain options. Never instruct the user to run a
nonexistent fixture flag. Automated checks and owned PTY tests remain the
implementation agent's responsibility.

## Gate 3a — submission lifecycle

Run from the repository root:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component composer --scenario main"
```

1. Send text: the box clears immediately, a fixed `you · 20:10 · sending` turn
   appears with dim delivery, and ordinary sending has no yellow activity row.
   Edit the new draft; F5 accepts without changing it. F6 refusal restores an
   untouched draft or stashes the refused copy beside newer text. F8 is ignored.
2. Enter while busy: messages queue. F5 first accepts, then ends a successful
   turn and sends one eligible item. Open `edit-queued`: Enter saves that slot,
   Esc keeps its original, Ctrl-D stashes the edited copy. `queue-stopped` and
   `queue-paused` wait for Ctrl-X Enter. F12 restores a paused queue.
3. Ctrl-X s stashes; Ctrl-X u restores/swaps, then swaps back. `/stash` opens
   the list; d drops and u undoes until closing. Check `stash-empty`, `stash-drop`
   and `stash-swap`. Fifty-first insertion reports oldest-entry eviction.
4. Inspect `paste-chip`, `paste-selected`, `image-chip` and `hidden-characters`.
   Chips move/delete/undo atomically. First Enter removes hidden format controls
   without sending; joined emoji remain intact. Pasted !, / and @ never execute.
5. Inspect `shell`, `shell-output`, `file-reference`, `file-inserted`, `slash`
   and `email`. F3 expands shell output. File selection inserts an atomic host reference;
   command selection emits its ID. F1 shows composer bindings.
6. In `editor-open`, F11 returns text. In `editor-conflict`, choose 1 or 2,
   Enter confirms; Esc keeps the box. The other version goes to stash. Empty/
   failed returns preserve text. Late callbacks cannot replace a newer lifetime.
7. Esc closes the nearest overlay/recall or cancels editor waiting. During work,
   Esc and Ctrl-C request one stop and pause the queue; the draft remains. In
   `cancelling`, a repeated Esc emits nothing; F6 reports failure and Esc retries
   with a fresh ticket. F5 confirms the stop. `shell-background-requested` also
   waits for F5 before releasing the foreground. Idle Esc twice clears text or recalls a rewind copy;
   rewind emits only when sending. Ctrl-C clears idle text, then twice on empty
   requests quit. Resize does not cancel an armed confirmation.
8. F10 opens scenarios; F10 returns with the current draft intact. Esc through
   pickers retains their selection/search/scroll. Cycle F4 and resize to 40
   columns; dashed recall must remain readable in NO_COLOR and ASCII.
9. Review `queue-full`, `queue-saved-blank`, `queue-resume-waits`,
   `queue-held-at-turn-end`, `restore-failed`, `save-failed`, `stash-full`,
   `editor-too-large`, `editor-read-failed`, `editor-stopped-waiting` and
   `editor-late-return`. Recovery preserves payload identities and the newer draft.
   Failed autosave retries after five seconds; an empty queue save stashes the original.
   In `queue-forty-columns`, queue rows sit directly above one activity row and
   the dock; Escape stays visible. Check `leader-waiting` quietly expires after three seconds.

## Completion Menu review

Engineering and human acceptance are separate. Start with:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component completion-menu --scenario main"
```

1. Type `rev`, move with Up/Down and insert with Enter or Tab. The menu closes
   with `/review ` in the box, without a sent turn or invocation. Type a number
   and press Enter again: the local host reports the command and argument.
2. Open `files`, insert a path and keep editing. The next Enter sends the message.
   Check `no-match-command` Enter sends literal text; `no-match-file` Tab takes
   the explicit closest guess and Enter sends the original token instead.
3. Check `loading` and `still-loading`: the dock stays put while results arrive.
   Open `error` and Tab to retry; the draft survives. `partly-failed` keeps good
   results with a nonselectable warning. Tab still inserts; Alt-R retries the failed
   source and Ctrl-R opens history. `timeout` is a warning and rejects the old reply
   after retry. Disabled rows show reasons and are skipped.
4. Check `command-inserted` faded argument help is not editable payload. Move
   within a token, then outside it; Escape preserves text, and ordinary subsequent
   typing does not reopen the same token. Paste stays data. Open `doesnt-open`.
5. Check `long-description`, `mixed-sources`, `narrow`, `under-thirty-columns`,
   `wide-name-clipped`, `menu-no-color`, `menu-ascii` and `menu-light`;
   resize below 30 columns. One selected band remains readable, descriptions
   expand within the same body height and fallback chrome retains meaning.
6. Check `shell-path` and its inserted state: plain path and trailing space with
   no @ or chip. Check dismissed typing, new trigger, leaving the word and pasted
   slash. `replacement-too-long` keeps the menu/draft with an activity notice;
   `removed-before-enter` keeps the draft and emits no command or submission.
   Menu keys appear in activity and the footer keeps Composer's Enter meaning.
7. Escape/F10 back through both pickers; searches, selections and scroll positions
   survive. Quit with Ctrl-C and verify normal shell input is restored.

Status: expanded implementation complete; [engineering verification passed](verification.md#completion-menu-alignment-engineering-record--2026-10-11)
and human visual review pending. No consumer rollout or commit is implied by this review.

## Gate 3b.1 — box modes and protected draft

```sh
make -C packages/tui run CARGO_FLAGS="-- --component composer --scenario box-history"
```

The rule says `history 3 of 12`. Up shows entry 4; Down shows entry 3. Down past
newest or Esc restores the exact unsent draft and cursor. Up in draft edits lines,
then reaches byte zero; the next Up recalls queue/history. Typing in a history
copy adopts it and stashes the set-aside draft. Enter sends a copy; acceptance
moves it to the top without duplication. `history-search` recalls without
sending; `history-earlier-session` displays host-supplied origin labels.

Engineering tests cover the full lifecycle, saved-state encoding/validation,
process temp-file permissions/cleanup and native Unix terminal release/reacquire.
TestBackend renders all registered scenarios and presentation profiles at bounded
sizes. Native human visual acceptance is not inferred from these checks. Gallery
storage, shell execution and clipboard responses are deterministic simulations;
consumer integrations must verify their real host callbacks and persistence.

The dated engineering summary is recorded in
[Verification](verification.md#composer-alignment-engineering-record--2026-10-11).
These steps request human acceptance of the demonstrated component scope.

## Gate 2c — commands and bindings

Implemented; human acceptance pending. This gate does not accept Dialog policies
or inline chat screen transitions, which have their own scopes.

```sh
make -C packages/tui run CARGO_FLAGS="-- --component commands --scenario main --colors truecolor"
```

1. The palette sits directly above the live yellow activity row and message box.
   Search `/branch`, then Enter. The activity reports `intent: rename`; the draft
   survives and no operation runs. Ctrl-P reopens commands with recent rows first.
2. Filter by `copy`, then `ctrl+x`. Groups disappear; keys align on the right.
   Digits enter the query. Search `/push`: unavailable publish cannot confirm.
   Esc clears the query first, then closes. No-match has no Enter hint.
3. F1 opens full-screen keys, hiding the dock. Search a key or label; Tab changes
   scope. Esc clears then closes, restoring the message box and its draft.
   F3 switches between docked palette and full-screen keys. Empty help scopes
   remain reachable; searching `ctrl+q` reports its availability only for the
   complete simulated host map.
4. F5 remaps Ctrl-P to Ctrl-O; both views show the effective key. Close and use
   Ctrl-O to reopen. F2 cycles the custom/conflict fixtures: the selected row shows
   default keys and source/conflict details. `o` prints a source inspection intent.
5. Close the view. Ctrl-X shows a pending activity immediately; wait 0.4 seconds
   for the continuation strip. `h` records one handoff intent. Repeat Ctrl-X and
   wait 3 seconds: the strip disappears. Esc quietly cancels. Ctrl-X then `z`
   shows mismatch feedback for 3 seconds. Paste while pending never edits the draft.
6. F6 changes focus and F5 replaces the map; both cancel pending input. Ctrl-C
   exits at the host even while pending. With the palette/help open, the first
   Ctrl-C arms closure and the second closes without quitting. F2 cycles 25 states, including empty,
   unavailable and no-match. Long lists page/scroll. Resize to 40 columns and short
   heights; optional details disappear before labels/keys. F4 checks all color
   fallbacks. Modified Enter never confirms a search result.
7. Try `alias`, `slash`, `empty-scope`, `keys-empty-scope`, `keys-no-match`,
   `unavailable-after-selection`, `short-terminal`, `forty-columns`,
   `palette-no-color`, `palette-ascii`, `palette-light`, `keys-no-color` and
   `hints-light` through F10. Filtering preserves opening height; host refusal
   keeps the draft and shows its reason for three seconds. In NO_COLOR disabled
   reasons include `[-]`, conflicts show `! wins`/`! overridden`, and selection
   uses reverse video. Native visual acceptance remains pending.

Report pass/fail by step before committing. The
[command contract](components/command-palette.md#commands-completion-and-help) defines behavior. Ctrl-P docking and F1 full-screen
placement follow the user's explicit 2026-10-10 decision. Source loading/opening
remain host boundaries; Composer slash/file routing follows the
[Completion Menu contract](components/completion-menu.md). The native visual gate
is pending even when rendered-buffer and PTY checks pass.

Engineering on 2026-10-10: `make check` passed formatting, strict Clippy,
63 all-feature tests (including native macOS PTY restoration), one doctest,
API docs and dependency audit. Gallery-disabled nextest passed 47 tests, and
the release build passed. The final recent-shortcut and stable-filter-height
refinements passed their rendered regressions, strict Clippy and release rebuild.
Shield's locked optional TUI compilation passed; Drone's workspace lockfile is
out of date, so its optional TUI compilation used a disposable manifest/lockfile
copy and passed without editing workspace files. Palette/keys snapshots
were inspected; Markdown targets and scoped diff
checks passed. Native visual acceptance and Linux/Windows remain unverified.



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
| 3 | Draft lifecycle, history and stash / group 2 | Submit with delayed acceptance/rejection, edit while pending and repeat Enter; no duplicate/lost draft. Recall history then restore unsent draft; stash/restore with overwrite/cancel/failure. Simulate external editor replacement conflict | E-04–E-06; explicit foreground handoff implemented |
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
make -C packages/tui run
```

1. **Catalog:** verify Option picker and Full-screen chat appear. Arrows change
   the highlight; Enter opens the chosen component.
2. **Picker:** open Option picker; its default is Continue without this server.
   Press 2 to choose Use all project servers; the fixture reports `Selected all`.
   Esc returns to scenarios; Esc again returns to catalog. Reopen: default/query reset. F2 cycles design states;
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
   view stays usable within current clipping limits. Esc returns to scenarios; Esc again returns to catalog,
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
make -C packages/tui run
```

1. Open **Multiline editor**. Its text starts at column 0 between title/footer
   rules, with no side borders. Tab changes focus: inactive text remains readable,
   its rules/title dim and cursor/footer disappear. Return and confirm draft,
   caret and selection survive. F3 modal traps input; Esc clears its selection
   before closing and restores the previous target.
2. Type words; Home/End and Ctrl-A/Ctrl-E move to the visible wrapped row ends.
   Ctrl-G selects all (Ctrl-Shift-A also works where encoded distinctly).
   Shift-arrows extends selection. Ctrl-W kills back to the previous space; Ctrl-U/Ctrl-K kills
   to logical line start/end. Ctrl-J inserts newline; standalone Enter does too.
3. Paste two lines; the footer confirms paste and text is tinted until the next
   key or three seconds. Ctrl-Z undoes the complete paste; Alt-Z restores it.
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
make -C packages/tui run CARGO_FLAGS="-- --component editor --scenario main --colors ansi256"
make -C packages/tui run CARGO_FLAGS="-- --component editor --scenario main --colors truecolor"
make -C packages/tui run CARGO_FLAGS="-- --component editor --scenario main --colors truecolor --light"
make -C packages/tui run CARGO_FLAGS="-- --component editor --scenario main --colors none --ascii"
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
make -C packages/tui run CARGO_FLAGS="-- --component picker --scenario main --colors truecolor"
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
   `Selected branch-29`. Repeat with Ctrl-Z/Alt-Z undo/redo. Digits edit the query.
   Esc clears it first while retaining picker height; Esc again returns to scenarios, then Esc returns to catalog.
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
make -C packages/tui run CARGO_FLAGS="-- --component picker --scenario main --colors none --ascii"
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
make -C packages/tui run CARGO_FLAGS="-- --component picker --scenario main --colors truecolor"
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

### Gate 2b.1 — visual shell and composition

The [dialog panel contract](components/dialog.md#official-dialog-design) is implemented for
centered panels, content-driven height, fill/inset fields, faded background, action
footers, title trails and boxed ANSI 16/no-color fallback. Human acceptance is
pending. Typing-aware arrival and timed removal notices belong to the separate
2b.2 gate; passing this gate does not accept its timing and caution behavior.

The user rejected the first visual handoff on 2026-10-09 and supplied a revised
contract: one centered panel in every view. The corrected panel is awaiting
review. Inline chat alternate-screen entry/exit is pending the inline adapter;
the gallery uses its existing alternate-screen owner throughout.

On 2026-10-10 the reviewer accepted title casing, live activity styling and emoji
cursor placement. Fade remained open; multiline growth, nesting, parent restoration
and fallback borders were not yet visually reviewed. Both timing and inline chat
screen transitions remain required before the complete dialog ticket closes.

The subsequent review passed growth, title trails and Escape parent restoration.
Fallback styling required correction: readable ANSI 16 faint hints, field-only
scroll hints, no-color field-only reverse focus, plain no-color background and
box corner spacing. Those corrections await human review. Saved iTerm2 profiles
brighten bold cyan to a grey-green bright-cyan slot; strict ANSI 16 cannot override
that hue while honoring the profile. See [presentation limits](presentation.md#tokens-and-fallback-behavior).
PTY input verifies one newline each for raw/encoded Ctrl-J and Shift/Alt-Enter;
duplicate physical key reporting was not reproduced on a native iTerm2 surface.

From the repository root, in iTerm2 for RGB:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component dialog --scenario main --colors truecolor"
```

1. **Panel:** the prompt centers over the screen, uses its content height and has
   one column of padding on each side. Its title names the task; keys appear in the footer.
   Background drafts and composer use a darker faded grey while their contents
   survive. The activity row stays live: yellow dialog status and normal grey
   Escape hints. A tall panel may cover the activity row.
2. **Field growth:** Ctrl-G then paste several lines, including accented text,
   CJK and a joined emoji. Only the prompt changes; paste never submits. Ctrl-J
   adds a newline; Shift-Enter and Alt-Enter do too when distinctly encoded by the
   terminal. Plain Enter submits. The panel grows and recenters; the field stops growing at six rows
   and scrolls with its caret visible. Left/right cross the entire emoji in one
   step without stopping on its second cell. Ctrl-Z/Alt-Z undo/redo the paste.
   Ctrl-G then Esc clears selection before a second Esc closes the layer.
3. **Nested/lifecycle:** reopen with F3, edit its prompt, then F3 again. Only the
   child body displays; its title trail includes the parent. Esc restores the
   parent's draft/caret. F6 replaces the top without adding a trail entry. F7
   rejects the stale ticket. F8 clears all layers; the dock message position stays
   fixed and activity becomes ready. Tab visits surviving background/composer
   targets. F5 removes background one's target; closing falls back to draft two.
4. **Fixtures:** F2 cycles prompt → nested → long → locked → outside →
   removed-return → error → confirm → notice → prompt-long → rename.
   Confirm defaults to Keep this draft. Notice acknowledges locally; rename shows
   scope, message and a named action. Long scrolls with arrows/Page/Home/End.
   Locked ignores Esc; Enter completes it. Error ignores Enter; F6 replaces it
   with a retry prompt. F9 simulates outside dismissal only in the outside fixture.
   These are local test actions; native mouse capture is not enabled.
5. **Panel/resize:** the panel centers over the screen, at most 72 columns.
   Resize to about 40 columns:
   scope disappears, message wraps, footer shortens and body remains usable.
   Shrink the height, then restore it: the active caret/content stay reachable.
6. **Profiles:** F4 cycles all four modes. RGB/indexed use filled surfaces with
   terminal-background fields; ANSI 16/no-color use full boxes and field rules.
   Ctrl-T changes RGB dark/light roles. Repeat with no-color/ASCII below.
   The previously demonstrated iTerm2 custom Bold color remains an accepted
   terminal-profile limitation; no settings change or app workaround is required.
7. **Exit/reset:** close layers and Esc returns to scenarios; Esc again returns to catalog. Reopen resets fixtures.
   Ctrl-C exits even while nested; ordinary shell echo/input/cursor work.

```sh
make -C packages/tui run CARGO_FLAGS="-- --component dialog --scenario main --colors none --ascii"
```

Report **pass** or **fail**, naming the step and observed behavior. Commit only
the reviewed Rust TUI changes after confirmation, then proceed to 2b.2.

Engineering: formatting, strict Clippy, 52 all-feature tests including macOS PTY
scenarios, 40 gallery-disabled tests, one doctest, API docs and dependency audit
passed. Measured geometry, fallback styles, Unicode caret cells, fixed dock and
snapshots are covered. Native Linux/Windows and human key encodings are unverified.
The release build and Gate/Shield/Drone optional TUI feature compilation passed;
consumer lockfiles were restored byte-for-byte. Scoped diff and local Markdown
link checks passed in both owning repositories. Dependency auditing reports
allowed duplicate-version warnings.

### Gate 2b.2 — arrival and removal timing

Implemented reusable policies; human acceptance pending. Open Dialog `waiting`:
type continuously, then pause for one second; the panel must preserve the draft.
Reopen and press Ctrl-G to open immediately. In `key-guard`, Enter/digits cannot
answer for 300 ms; letters still type. In `away`, inspect the simulated attention
request and waiting duration. In `child-removed`, verify parent focus and the
three-second reason. Ctrl-C/Ctrl-D must arm, then close one layer without quitting.
In `danger-armed`, confirm only with a second Enter within three seconds; in
`danger-expired`, Enter must arm again. In `type-name`, verify exact matching and
that paste does not submit. Check `inside-error`, `tiny-content`, `tall-panel`,
ASCII/light and ANSI 16/NO_COLOR profiles. Engineering results and native inline
hosting remain independent of this human review.

## Chat review — complete local conversation

Run `make -C packages/tui run CARGO_FLAGS="-- --component chat --scenario answer"`.
Review the product content without counting gallery title/F-key rows.

1. Compare answer, streaming, actions-off, welcome and pane scenarios. Check
   header metadata, body indentation, content/folds and the fixed dock.
2. Type a draft, move the caret, open search with Ctrl-F, search timezone,
   navigate matches and Esc. Type again: caret and draft must be restored.
3. Ctrl-X V selects blocks; navigate to code and press C. Check the simulated
   copy notice. Y copies Markdown; O requests supplied content inspection.
4. In long, Alt-Home twice loads earlier history. F9 shows a retryable failure;
   Alt-End follows again. Resize while reading and compare the source anchor.
5. In streaming, Enter queues a message; Esc stops and pauses the queue.
   Ctrl-X Enter resumes explicitly. New output while reading shows unread
   without moving Composer. F6 preserves failed partial output; Ctrl-X R retries.
6. Check narrow-40, ansi16, no-color and ascii, including Unicode source,
   copied code, visible signs and native terminal selection.
7. Open copy-pending and link-pending. F6 refuses the effect; selection and
   draft remain. Y/O requests again and F5 confirms. Table-inspection-pending
   opens full content only after F5; Esc restores the draft. Earlier-loading and
   newer-loading block duplicate/cross-direction requests; F6 retains a retry
   hint and Alt-Home/Alt-End creates a fresh request. Inspect quiet-note,
   timestamps-hidden, attachment-unknown and host-suggestions. Search-ansi16,
   search-no-color and search-narrow-ascii retain match navigation and Esc.

Engineering evidence does not record human visual acceptance. The real host
owns clipboard, opening, history storage and agent execution; this review uses
the gallery's local acknowledgements.

## Tool Call Card review — complete local activity

Run `make -C packages/tui run CARGO_FLAGS="-- --component tool-call-card --scenario main"`.

1. Compare queued/running/waiting/completed/failed/cancelled, text/code/diff,
   unknown and not-shown. Check words, duration, indentation and the fixed dock.
2. Type a draft and move the caret. Ctrl-X V selects cards; arrows move blocks,
   Enter expands/collapses, Y/C copy clean retained call/output and Escape returns.
   Type again and confirm that the original caret and draft survive.
3. In failed-selected, R adds a new retry below the dim folded original. Review
   retry-refused, copy-failed and details-failed: failures do not invent success.
4. In large-output, select and O for details. `/` searches `(700)`, N finds next,
   Y simulates copy-all and Escape returns to the same card. Expanded-capped shows
   first ten/last ten and an explicit 1,184-line omission marker.
5. Review three-at-once, out-of-order, all-done, all-succeeded and many-at-once.
   Each call stays in start order; failures remain visible and group Enter reveals
   hidden members. Streaming-large and F8 grow output without growing its tail.
6. Review narrow/narrow-diff/narrow-failed, palette-16, without-color,
   without-color-diff and ascii. Check complete narrow durations, visible signs,
   reversed no-color head, wrapping and capability-gated hints. Resize while selected.
7. F10 and Escape return through pickers; confirm remembered query/selection/scroll.
   No-ack holds effects; delayed-ack F7 succeeds; stale-ack F7 rejects the old result.

This review is pending human acceptance. Engineering snapshots and macOS PTY tests
provide separate evidence. Clipboard, execution and permissions remain host-owned;
gallery effects are only local simulations. Dialog timing and broader platform
acceptance remain independent work.

## Acceptance record

| Slice | Engineering | Human | Commit / next action |
| --- | --- | --- | --- |
| 0 | Passed during foundation commit; recorded in verification | Passed on 2026-10-09; user reported it works, with visual design deferred | Plan committed: private `d623657`, public `e32517d` |
| 1 | Redesign checks recorded below | Functional editor/focus and visible newline/color fixes passed on 2026-10-09; remaining redesign actions pending | Explicitly authorized commit: private `41ee25b`, public `735103b`; user authorized next slice |
| 2a | Checks recorded after Gate 2a below | Accepted on 2026-10-09; user requested next slice | Committed: private `f3e7579`, public `17e53d5` |
| 2b | Foundation engineering recorded; timed policies and 36 scenarios implemented | Expanded visual review pending | Foundation checkpoint: private `4c6260d`, public `05857ee`; new work uncommitted; native inline transition separate |
| 2c | Implemented; engineering results recorded at Gate 2c | Pending | Explicit checkpoint commit requested on 2026-10-10; visual acceptance remains pending |
| Composer | Complete local workflow and native host adapters implemented; current engineering checks recorded above | Native visual review remains separate | Review complete component; commit only when requested |
| Completion Menu | Reusable component, Composer integration and 30 interactive states; expanded engineering checks passed | Complete visual review pending | Review complete component; commit only when requested |
| Chat | Rich component and 51 interactive states; [engineering record](verification.md#chat-engineering-record--2026-10-10) | Complete visual review pending | Review complete component; commit only when requested |
| Tool Call Card | Complete local cards/groups, Chat integration and 44 interactive states; [engineering record](verification.md#tool-call-card-engineering-record--2026-10-10) | Complete visual review pending | Review complete component; commit only when requested |
| 5–13 | Other capabilities retain their documented gaps | Not presented | Complete components in dependency order |

Slice 1 engineering: `make -C packages/tui check` passed formatting, strict
Clippy, 25 all-feature tests including native macOS PTY, one doctest, API docs
and dependency auditing. `cargo nextest run --locked --no-default-features`
passed 21 library tests; `make -C packages/tui build` passed release build.
Gate/Shield/Drone `cargo check --offline --features tui` passed with temporary
lock resolution; their existing lockfiles were restored byte-for-byte to keep
this slice scoped to Rust TUI. Consumers need lock resolution when adopting the
polling backend dependency; this is compilation evidence, not a product rollout.
Auditing allows duplicate hashbrown/syn/thiserror versions. Markdown link and
both repositories' diff checks passed. Native Linux/Windows and human modifier
key encodings remain unverified.

## Code and diff inspection review

Engineering evidence is recorded separately in
[verification](verification.md#code-and-diff-inspection-engineering-record--2026-10-12).
Human visual acceptance is pending. Open:

```sh
cargo run --locked --features gallery -- --component code-diff-inspection --scenario main
```

1. Compare `main`, `code`, `deleted`, `renamed`, `folded` and `unfolded`.
   Check numbers/signs, function headers, cursor bands, context counts, Enter
   unfolding and first/last hunk feedback. Tab in `main` keeps the source line
   when switching to the supplied whole file and back.
2. In `long`/`wrapped`, move by line/page/end and sideways. Confirm two-row
   overlap, long wrapped-line access and stable source position after resizing.
   Use `:` with an existing line, past-end number and invalid input.
3. In `find`, verify visible query and cursor, counts, case sensitivity with
   capitals, Enter/n/N, no-match stability and hidden-fold reveal. Paste a query;
   multiline paste and commands outside the field must not execute.
4. In `selection`, extend through context and changes. `y` requests patch/text;
   `c` omits removed rows. F6 failure retains selection/reason; retry and F5
   success clears the original selection. Check editor pending/failure and
   stale/foreign/duplicate results, with no actual external effects.
5. Check loading/error/retry, binary/generated/too-large/partial states.
   Loading permits return; only explicitly supplied partial content can be
   shown. Its partial marker remains visible after subsequent operations.
6. In `from-chat`, inspect and return to the same selected block/draft/caret.
   Close selection, insert and undo a character, reopen the inspector and check
   retained position. In `from-tree`, Tab/Escape returns focus to the same node.
   F10 and F10 again preserve the active scenario state.
7. Compare narrow code/diff, tiny, ANSI16/256, RGB dark/light, NO_COLOR, ASCII,
   Unicode and controls. Escape must remain reachable/visible, line signs must
   remain meaningful without color and source controls must remain text.

No filesystem, clipboard, editor or Git operation is performed. Record visual
findings against the component contract; tests alone do not establish native
Windows/Linux, assistive input or actual consumer acceptance.

## Status Panels & Notices review

Open `--component status-panels-and-notices --scenario main` at 130 columns.
Review the 32-column sidebar, then Ctrl-X I, arrows/folds and Esc. Edit in the
middle of the retained draft and verify caret/undo survive entry and return.
Resize to 64 and 40 columns; inspect status and notices without obscuring the draft.

Use Ctrl-X N, Enter, F6, retry and F5 to review action failure/recovery. D sets an
error aside while its badge and list entry remain. F7 expires info/success; errors
remain. Use `long-reason` and Alt-PageUp/PageDown, five state fixtures and ASCII,
ANSI16, NO_COLOR, light and reduced-motion profiles. In `review`, notice arrivals
must leave review Enter and Ctrl-G behavior intact. These steps await human visual
acceptance; automated evidence is recorded in [verification](verification.md).
