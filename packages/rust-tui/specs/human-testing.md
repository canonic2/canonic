# Human testing gates

Status: workflow requested on **2026-10-09**. Foundation human testing passed.
Visual refinement is deferred at the user's request; functional acceptance does
not establish design acceptance. Implementation slices remain unstarted. [Roadmap](roadmap.md) owns
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
Future catalog/scenario labels below are intended deliverables, not available
commands today. At each handoff, replace intended labels/keys with the actual
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
| 2 | Rich picker, dialogs and commands / group 2 | Filter grouped/disabled rows; page and confirm original identity. Open nested dialogs; Esc closes only top and restores focus. Search palette/help; one selection emits one intent. Test remapped/pending bindings | D-01–D-03, I-02; A-03/A-04 for these views |
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
   Type `all`; only Use all project servers should match. Enter confirms and the
   fixture label reports original choice 2. Esc returns to catalog. Reopen:
   default/query reset. F2 shows empty choices; Enter must not confirm anything.
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

## Acceptance record

| Slice | Engineering | Human | Commit / next action |
| --- | --- | --- | --- |
| 0 | Passed during foundation commit; recorded in roadmap | Passed on 2026-10-09; user reported it works, with visual design deferred | Commit gated plan, then implement slice 1 |
| 1–13 | Not started | Not presented | Implement in sequence after previous accepted commit |
