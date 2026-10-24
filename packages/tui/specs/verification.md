# Verification and acceptance

This topic maps each component to the suites, gallery checks and native
terminal fixtures that exercise it, and names the evidence that is still
missing. [Code quality](code-quality.md) owns the executable package checks;
[Human testing](human-testing.md) owns runnable review steps and human
acceptance status. [Roadmap](roadmap.md) owns delivery order. Component and
feature specs own the acceptance requirements; this topic does not redefine them.
A passing run does not certify later changes.

## Independent checkpoints

Track each checkpoint for a named scope:

| Checkpoint | Evidence required | Does not establish |
| --- | --- | --- |
| Specification complete | Behavior, states, keys, implementation design, host boundary, acceptance and gaps reviewed | Implementation or accepted visuals |
| Design resolved | Layout, feedback, responsive/fallback behavior and interactions settled in the public contract | Engineering or human acceptance |
| Functionality complete | All in-scope behavior implemented; remaining host duties explicit | Passing checks or host integration |
| Engineering verified | The package checks pass, with applicable state/render/property/native checks for the scope | Human acceptance or untested platform coverage |
| Human accepted | Explicit reported pass for demonstrated steps on the reported terminal | Acceptance of later material changes or other hosts |

Use partial or pending when any in-scope requirement lacks evidence. A commit is
not human acceptance, and tests found in source are not a test run.

## How components are tested

All tests share one integration binary, [tests/integration/main.rs](../tests/integration/main.rs).
Each component is covered at four levels:

- **Behavior:** state and action tests through the public API, with proptest
  invariants where input is open-ended.
- **Rendering:** TestBackend rendering at tiny, narrow and standard sizes across
  the color, symbol and theme profiles, with reviewed insta
  [snapshots](../tests/integration/snapshots/) for representative states.
- **Gallery:** headless gallery tests open every named scenario of the component,
  exercise simulated host effects and check retained F10 navigation. Gallery host
  effects are deterministic simulations, not host integration.
- **Native terminal:** owned Unix PTY tests drive the real gallery binary for
  paste, resize, return to the draft, cooked input and exact terminal-attribute
  restoration after exit.

## Coverage by component

### Editor and Option Picker

[Editor tests](../tests/integration/editor.rs) cover Unicode-safe movement,
deletion, wrapping, selection, grouped undo/redo, paste limits and budgeted
replacement. [Picker tests](../tests/integration/picker.rs) and the
[picker PTY tests](../tests/integration/picker_terminal.rs) cover stable-identity
filtering, disabled and dangerous rows, multiple marks hidden by a filter,
paging, refresh, centered number keys, clear highlight edges, detail-width
thresholds and a stable activity row. The picker PTY test checks default
foreground, bold and no dim on selected labels in all four color modes;
ANSI 16 and no-color keep the reverse fallback.

I-06 (multiple-choice keys) is covered at the component level by
`filtered_multiple_choice_marks_with_space_and_continues_with_enter` and
`host_bounds_allow_empty_and_keep_unmarked_rows_reachable_at_maximum` in the
picker tests, and `choose_many_refuses_a_minimum_above_the_enabled_choices_before_opening`
covers an unreachable minimum. Through a PTY, the
[inline host fixture](../tests/fixtures/inline_host.py) runs `choose_many` with
one context line over a short list: the context stays visible, Enter below the
minimum shows the Space notice, and Space then Enter returns the marked
identity with the terminal restored.

### Composer

[Composer](../tests/integration/composer.rs),
[workflow](../tests/integration/composer_workflows.rs),
[host adapter](../tests/integration/composer_host.rs) and
[PTY](../tests/integration/composer_terminal.rs) tests cover delayed acceptance
and refusal, queue editing, stopping, pausing and resumption, history recall,
stash, typed paste/image/file atoms through recall, stash, storage and external
editor tokens, saved-state encoding including older snapshot schemas, failed-save
retry, editor conflicts and late results, identified stop/interrupt/background
controls, shell-output folding, private editor-file ownership and foreground
handoff to an external editor.

### Completion Menu

[Completion tests](../tests/integration/completion.rs) and the
[PTY test](../tests/integration/completion_terminal.rs) cover matching and
ranking (including aliases), disabled navigation, generation rejection,
streaming order and selection, debounce, loading, timeouts, retry with fresh
identities, partial-source failure with Alt-R retry, guesses, insertion budgets,
token boundaries, F1 help without draft loss, command removal before invocation,
Ctrl-R history routing and insert-then-invoke argument delivery.

### Command Palette

[Command tests](../tests/integration/commands.rs) and the
[PTY test](../tests/integration/commands_terminal.rs) cover ranking after exact
effective keys, captured opening height, empty scopes, unavailable and conflict
wording, remapped and pending bindings, bounded profile rendering, a boxed
palette clearing caller attributes inside its panel, the opening guard and
two-press Ctrl-C closing without quitting.

### Dialog

[Dialog tests](../tests/integration/dialog.rs),
[design tests](../tests/integration/dialog_design.rs) and the
[PTY test](../tests/integration/dialog_terminal.rs) cover FIFO ownership, timed
arrival, the key guard, two-press closure, nested layers and parent focus
restoration, withdrawn arrivals, stale tickets, removal notices, exact-name and
timed irreversible confirmation, measured geometry, fallback styles and Unicode
caret cells.

### Chat

[Chat tests](../tests/integration/chat.rs) and the
[PTY test](../tests/integration/chat_terminal.rs) cover identified interleaving
and resync, stale and foreign callbacks, loaded search, folded content,
original-source copy, full table/patch inspection, reflow and paging anchors in
both directions with failure and retry, unread state and dock geometry, safe raw
content, Unicode/ASCII, pathological cache bounds, pending acknowledgements and
refusals, and draft and caret retention across every overlay.

### Tool Call Card and Run Activity

[Tool Call Card tests](../tests/integration/tool_call_card.rs),
[gallery tests](../tests/integration/tool_card_gallery.rs) and
[Run Activity tests](../tests/integration/run_activity.rs) cover lifecycle and
revision guards, member updates, coalescing and final flush, exact omissions,
source safety, preview and group folding, capability gating, identified effects,
retry history, progress, child tasks, artifacts, single-use cancellation,
completion races, stale acknowledgements, disconnection, injected motion and
Escape selection priority.

### Question Form and Permission Review

[Question Form tests](../tests/integration/question_form.rs),
[Permission Review tests](../tests/integration/permission_review.rs),
[gallery tests](../tests/integration/permission_gallery.rs),
[review gallery tests](../tests/integration/review_gallery.rs) and the
[PTY test](../tests/integration/review_terminal.rs) cover bounded descriptors,
staged validation and cancellation, deliberate numeric/Enter and exact-scope or
destructive confirmation, retry, refusal, replacement, expiry, withdrawal,
foreign/stale/duplicate tickets, mixed queue order and capacity, ignored paste,
and preservation of the Composer document, caret, selection, undo, reading
anchors and unread output.

### Session Browser

[Session Browser tests](../tests/integration/session_browser.rs),
[gallery tests](../tests/integration/session_gallery.rs) and the
[PTY test](../tests/integration/session_terminal.rs) cover atomic validation,
stable selection and removal, title/message matching, query, discovery and
paging races, explicit retries, pending limits, failed rename drafts, action
availability, single-use/foreign/stale tickets, complete deletion wording,
destructive arming and paste disarming, and Ctrl-X O return to the draft.

### Tree Browser

[Tree Browser tests](../tests/integration/tree_browser.rs),
[gallery tests](../tests/integration/tree_gallery.rs) and the
[PTY test](../tests/integration/tree_terminal.rs) cover navigation, filtering
and paste, refresh, removal and reparenting, topology limits, lazy/root/page
failure and retry, cascade and collapse, stale or malformed results, bounded
concurrent requests, disabled actions, focus, extension-preserving clipping and
no-color/ASCII chrome. Inactive inspection renders through an in-memory backend
and never acquires a terminal or the active filter cursor.

### Code and diff inspection

[Inspection tests](../tests/integration/inspection.rs),
[gallery tests](../tests/integration/inspection_gallery.rs) and the
[PTY test](../tests/integration/inspection_terminal.rs) cover exact, foreign,
duplicate, stale and wrong-kind acknowledgements, malformed patch and whole-file
validation, numbered excerpts, folds, hidden copy and search, smart case and
Unicode mapping, bounded dense matches, page overlap, selection changes during
copy, control text, the visible one-row find field and return to the Chat
selection, draft and caret.

### Status Panels and Notices

[Status tests](../tests/integration/status_panels.rs),
[gallery tests](../tests/integration/status_gallery.rs) and the
[PTY test](../tests/integration/status_terminal.rs) cover stable navigation,
folds and removal, atomic invalid updates, exact/foreign/stale/duplicate
acknowledgements, disabled and repeated actions, pending and failure expiry,
monotonic clock properties, persistent set-aside errors, long reasons that stay
scrollable in a tiny viewport, review and Composer overlay priority, and caller
caret restoration.

## Terminal runtime

### Inline composition

[Inline composition tests](../tests/integration/inline_composition.rs),
[inline gallery tests](../tests/integration/inline_gallery.rs) and the
[inline PTY tests](../tests/integration/inline_terminal.rs) cover cursor
anchoring and the missing-report fallback, contiguous settling, revisions during
pending output, exact receipts, ambiguous-write stop, surrounding output before
and after, growth and shrink of the preferred height (shrink keeps discarded
rows as scrollback), zero-height rejection, deferred fitting while a borrowed
screen is active, foreground release and resume, alternate-screen borrowing and
return, and confirmation blocked at tiny sizes. A small VT harness records that
no normal-screen whole-page clear occurs.

### Job control

The gallery's [job control](../src/gallery/job_control.rs) handles Ctrl-Alt-Z
and external SIGTSTP; reusable runtime APIs install no job-control handlers. Owned
controlling-PTY cases verify actual stops, canonical shell input and exact
termios while stopped, background SIGCONT without mode acquisition or drawing,
shell output preserved through resize, retained inline and full-screen drafts,
repeated suspension, too-small resume and a missing controlling terminal. Each
case owns its session and process group; no user terminal or unrelated process
is signalled.

## Native terminal findings

- On macOS a controlling tty can briefly mark restored input `PENDIN`. Native
  fixtures consume a cooked shell line before comparing the original termios
  rather than masking the flag or changing production restoration.
- iTerm2 profiles can apply a custom Bold color darker than Foreground;
  shell-only output reproduces it. This is an accepted terminal-profile
  limitation, not an application defect. See
  [presentation limits](presentation.md#tokens-and-fallback-behavior).
- PTY input produces one newline each for raw and encoded Ctrl-J and
  Shift/Alt-Enter.

## Chat rendering measurement

A release build on macOS 15.7.9 (x86_64, Intel Xeon W-2140B at 3.20 GHz, Rust
1.99.0) rendered 100 turns of 550,000 bytes of Unicode prose through an ANSI 16
TestBackend at 100×40. Cold construction plus first draw took 279.25 ms; warm
draws averaged 1.22 ms over 25 frames with 19,900 cached rows and no extra parses
or layouts. This excludes physical terminal writes and is not a p95,
input-latency or portable guarantee. The broader campaign is in
[Gallery](features/gallery.md#performance-measurements--proposed-targets).

## Gaps

- Human visual acceptance is pending for most components; see
  [Human testing](human-testing.md#acceptance-record).
- Native suites run on macOS only. Linux and Windows, multiplexers, real emulator
  reflow, IME, physical keyboard modifier encodings and assistive technology are
  not verified.
- Real host clipboard formats, opening, storage, discovery, permission authority
  and execution are host-owned and not covered by gallery simulations.
- Uncatchable SIGSTOP cannot run cleanup; termination signals and inherited
  custom handlers need separate policies.
- No PTY case types a filter into `choose_many`, and `choose`,
  `choose_choices` and `select` have no PTY case; I-06's filtered keys are
  verified at the component level only.
- Automatic content-height policy for inline hosting remains qualified in the
  [terminal contract](features/terminal.md#inline-runtime-api).

## Evidence and review locations

- [Human acceptance record](human-testing.md#acceptance-record) holds human
  review status per component.
- [Component checkpoints](components/README.md#completeness-checkpoints) summarize
  current scopes without upgrading partial work to complete.
- [Inventory](inventory.md) links source/test evidence and identifies gaps.
- [Gallery coverage](features/gallery-designs.md) records executable scenarios
  and pending coverage; harness controls are outside product layout acceptance.

When a change alters what a suite covers, update the coverage above. Use real
terminal evidence for terminal contracts. Do not add tests that assert prose.
