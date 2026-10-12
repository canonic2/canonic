# Usage

## Tree Browser

Run `cargo run --locked --features gallery -- --component tree-browser --scenario main`.
Up/Down or Ctrl-P/Ctrl-N move. Right opens a folder, then enters its first child;
Left closes it, then climbs. Space toggles in place; `*` opens the subtree and
loads missing children; `-` closes the subtree. Home/End and PageUp/PageDown move
through loaded rows. Slash filters loaded names while retaining their ancestors.
Escape clears the filter, then returns to Chat or moves to the main pane.

Enter invokes the host's primary Open/Select action, retries the selected failed
load, or loads the next page. `a` opens the supplied actions; disabled choices
retain reasons. Pending rows remain pending until a host response. F5 simulates
success, F6/F7 failure/refusal, F8 material replacement, F9 removal and F12 an old
result. These are fixtures; they never read files or execute actions.

`main`/`panel` show a 34-column tree beside retained Chat at 100+ columns and
24 columns at 60–99; below 60 the focused view fills the screen. Tab moves between
panes. `full-screen` returns to Chat with Escape; Ctrl-X F reopens the tree when
the host enables files. F1's Files command uses that same entry. Chat keeps its
draft, caret, undo and reading state. F10 opens scenarios and returns without reset.

Try `tasks`, `filter`, `loading`, `load-failed`, `paging`, `removed`, `bad-data`,
`deep`, `long-names`, `narrow`, `tiny` and the shared color/ASCII profiles.
Selected full names, disabled reasons and errors appear below the tree. Alt-PageUp
and Alt-PageDown scroll these details. The caller controls placement, focus,
snapshots and every request result; [Tree Browser](../specs/components/tree-browser.md)
defines the reusable contract. Standalone Code/Diff Inspection is separate.

## Permission Review

Open `cargo run --locked --features gallery -- --component permission-review --scenario main`.
Numbers highlight a choice; Enter confirms it. An affirmative default is invalid.
Up/Down navigate choices and the details row. Tab opens/hides safe details;
PageUp/PageDown scroll them; Ctrl-F searches loaded source and Enter finds the
next match. Paste is dropped with a notice. Escape closes search, then details,
then dismisses; dismissal differs from Deny and Skip.

Broader choices open a Dialog quoting the exact host scope, with Back selected.
Down or 2 only highlights Allow; Enter confirms. Escape returns without a response.
Destructive choices require two unmodified Enter presses within three seconds.
Movement, inspection changes, paste, expiry and replacement disarm them.
Sending locks input until an actual host acknowledgement. Failed application
requires R retry; Enter never silently resends. F5/F6/F7 simulate applied/failed/
refused results, F8 expiry, F9 replacement and F12 a stale result. No gallery
scenario grants permission, executes an agent or writes policy.

Hosts construct `permission_review::PermissionReview` from a bounded `Request`,
route events through `update`, inject elapsed time with `tick`, and drain
`Intent::Respond` immutable ID/revision/choice/scope/ticket envelopes. Only the
exact current ticket can acknowledge actual application. Call `expire`, `withdraw`
or validated `replace` for host lifetime/material changes. `render_with` draws
the panel and `render_overlay` its Dialog in a separate caller-owned viewport.
See the [complete contract](../specs/components/permission-review.md).

Use `chat.push_review(Review::Permission(Box::new(panel)))` or
`Review::Questions(Box::new(form))` for immediate saved-anchor composition.
`chat.reviews_mut()` exposes explicit host acknowledgement, reorder and withdrawal;
tick/render/update synchronizes closed receipts. Drain `take_intents` regularly.
The queue has one active review and at most sixteen waiting. Arrivals defer after
actual Composer text changes until one second of quiet or Ctrl-G. Active Permission
withdrawal stays visible until Escape; later entries retain host order.

## Question Form

Open `cargo run --locked --features gallery -- --component question-form --scenario main`.
Single-choice numbers/Enter choose and advance; Space marks multiple choices and
Enter validates them. Tab/Shift-Tab navigate freely without clearing answers.
Fields reuse Editor: Ctrl-J newline, selection, paste and undo stay in the form.
Something else opens on Enter, typing or paste on its last row. Esc keeps typed
text staged, Delete clears it and Ctrl-Z restores it. Host character limits show
validation without cutting text; resource-limit insertions reject whole.

One question submits when confirmed. Multiple questions require Enter on Review;
1–9 edit a question, and Up/Down then Enter reaches later questions. The host must
acknowledge the opaque response ticket. Failure retains answers for another
deliberate confirmation; refusal returns to the addressed question. Esc with
answers opens a Dialog defaulting to Keep answering. Send answers so far appears
only when explicitly offered by the host. Expired/replaced answers remain copyable
with Y; clipboard work is an identified host intent.

Construct `question_form::Request` with stable IDs and a material revision, then
`QuestionForm::new`. Standalone hosts call `update`, `tick`, `render_with` for the
panel and `render_overlay` last with their modal viewport. Drain `take_intents`;
acknowledge response/copy tickets separately. Use `replace` for a higher material
revision and `expire` for host lifetime changes. Components perform no I/O.

Chat owns `reviews_mut()` and `reviews()` for a shared `ReviewQueue`. Push
`Review::Questions(Box::new(form))`, reorder a complete set of identities, or
withdraw by ID. Active input and focus survive later arrivals/reordering. Actual
typing defers opening for a one-second pause or Ctrl-G; navigation alone does not.
Chat consumes form input above the fixed dock and restores Composer state and the
saved logical viewport on closure. Concurrent output remains unread. Host execution,
request lifetime, order, transport and durable storage remain external.

Gallery F5/F6/F7 simulate acceptance/failure/refusal. F8 expires, F9 replaces, F12
tries a stale reply, F2 cycles fixtures and F10 returns to the retained scenario
picker. No fixture executes an agent or changes real permissions.

## Commands and keyboard help

Open **Commands and bindings** in the catalog, or use
`make -C packages/tui run CARGO_FLAGS="-- --component commands --scenario main"` from the
repository root. Ctrl-P opens a docked palette centered horizontally above the activity row
and message box. F1 opens the full-screen keys view. Closing either preserves the
message draft. F3 switches between the two projections.

The palette shows recent commands first, then groups. Its plain filter searches
labels, descriptions, aliases such as `/branch`, and effective keys. Exact keys
rank first, followed by prefixes, word starts, aliases and fuzzy matches, with
recent commands breaking ties. Filtering hides group headings; digits enter the filter. Enter
returns one enabled command ID; the gallery records the intent without executing
an operation. Esc clears the filter first, then closes. Optional descriptions
are omitted on narrow screens, with the selected description below its row at
40 columns. Unavailable reasons replace the right-aligned key. Opening height
stays fixed during filtering and clamps above the message box.

The keys view groups by host-supplied scope, displays effective shortcuts on the
left, and shows selected custom bindings' defaults, source locations and conflicts.
Tab cycles scopes. `o` on a custom/conflicting row returns a source inspection
intent; the gallery prints it, and does not open a file. Help never runs commands.
F5 remaps Ctrl-P to Ctrl-O using fixture data; the active map and help agree.

Close the view and press Ctrl-X. The activity immediately shows the prefix; after
0.4 seconds a continuation strip appears above it. The sequence expires after
3 seconds. Follow with `h` to record a handoff intent. Esc quietly cancels;
an unmatched continuation shows feedback for 3 seconds. Neither mismatches nor
paste reach the message draft. F6 changes focus and cancels pending input.
F2 cycles palette, text filter, key filter, unavailable, no-match, keys, custom
keys, conflict, pending, delayed hints, mismatch and empty fixtures. F4 cycles
color profiles. Additional fixtures cover empty scopes, host refusal, alias,
short terminals and profiles. While the palette/help is open, Ctrl-C arms then
closes it without quitting; a subsequent Ctrl-C at the host exits.

Hosts supply `commands::Command` descriptors to `Commands` and
`commands::bindings::Dispatcher`. `Commands::remapped` resolves ordered host-loaded
overrides: later entries win, conflict losers remain visible without the losing
key, and reserved-key overrides are ignored with evidence. Hosts load files,
provide source paths/lines and execute inspection or invocation intents.
Construct replacements before swapping the active map, advance dispatch with the
host clock, reset on scope changes, and recheck availability before execution.
Modified Enter, repeats, releases and paste never invoke commands. Slash insertion
belongs to [Completion Menu](#completion-menu); a later Composer Enter invokes it.

## Gallery

Open the gallery with `make -C packages/tui run`. It starts at the component
picker. Select a component with Enter, then select a scenario with Enter.
Esc returns from the scenario to its scenario picker, then to the component
picker; Esc at the component picker exits. Component-specific dismissal (such
as closing a dialog or clearing a selection) takes precedence. Ctrl-C arms then
closes an open dialog; during a run it requests cancellation. When the active
component yields Ctrl-C to the gallery, it exits. Composer has its own quit arming.
Ctrl-T changes RGB
dark/light tokens; F4 cycles ANSI 16, ANSI 256, RGB and no-color profiles.
Body text uses the terminal's foreground/background in every profile.

The scenario picker is searchable, with `main` first. Enter opens a fresh fixture.
Both picker levels retain their search text, selection and scroll position when
you return. Each component has its own remembered scenario picker. Resizing
adjusts the viewport as needed to keep the selected item visible. This state
lasts for the current gallery run.
F10 opens the active component's scenario picker; pressing F10 again returns to
the current scenario with its draft and cursor intact. `--component <name>` opens
that component's scenario picker, and adding `--scenario <name>` opens a fixture
directly. Simulation pauses while the picker is open. F2 cycles the working
fixtures. Each component also offers `narrow-40`, `ansi16`, `no-color`, `ascii`,
`light`, `truecolor-dark`, `ansi256` and `no-color-ascii`
profiles of its main view; F4 changes colors and Ctrl-T changes the theme.
The narrow profile caps the component area at 40 columns inside the gallery.

Entries marked `design pending` open a description of the intended behavior and
its source board. They do not simulate a feature that is still awaiting
implementation. Some working fixtures describe a remaining design gap beneath
the component. The [design map](../specs/features/gallery-designs.md) records coverage.

From the package directory, list or open named scenarios without cycling:

```sh
cargo run --locked --features gallery -- --list
cargo run --locked --features gallery -- --component composer --scenario sending
cargo run --locked --features gallery -- --component editor --scenario selection
cargo run --locked --features gallery -- --component composer --scenario history
```

Unknown scenario names fail before entering terminal mode. `--scenario` requires
`--component`. The design names `default`, `focused`, `prompt`, `palette` and
`nest` are accepted aliases for their corresponding main or nested fixtures.

The rich picker returns stable choice IDs. The default three-row fixture has no
filter: arrows/Home/End/Page keys navigate and 1–3 immediately choose a row.
Plain Enter confirms the highlighted row; modified, repeated or released Enter
never confirms. Disabled rows cannot be highlighted or chosen.
Danger rows require explicit navigation and are never implicit defaults or
pre-marks. F3 returns an inspection intent when the host enables a detail view.
Confirmation updates a fixture
result label and does not change files. Esc cancels; with a nonempty filter it
clears that filter first. Cancellation remains visible until another Esc returns
to navigation. Paste never confirms.

F2 cycles default → details → disabled → danger → filter → no-match → scrolled
→ multi → groups → loading → narrow → done → empty → error → maximum → minimum.
It resets the picker
while preserving the independent host draft. F5 simulates a source refresh by
removing the highlighted ID. F6 changes stage/dock/full-screen placement within
the gallery's existing terminal session. Default placement docks the measured
picker above a persistent activity row and message editor; F6 cycles dock → full
screen → stage. The activity row shows pending input or source status, then
`ready` after confirmation. It remains reserved across fixture states.
Tab/Shift-Tab switches picker/draft
focus; Ctrl-J inserts a draft newline. Return to catalog/reopen resets both.

Filtering defaults on above seven choices; hosts can force it on/off. Type or
paste into the filter; digits are query text when filtering is enabled. Ctrl-G
selects the query; Ctrl-Z/Alt-Z undo/redo its editor transactions. Matching is a
case-insensitive subsequence of the label or detail, ignoring query whitespace.
Label matches rank before detail-only matches, then prefix, word start and fuzzy;
source order breaks ties. The
matched graphemes are bold/underlined. A surviving enabled highlighted ID remains
highlighted; otherwise the first safe ranked result is used. Multiple mode uses
Space to toggle a mark and Alt-A to toggle all matching enabled rows. Hidden
marks survive filtering; Enter returns the marked IDs in supplied order, including
hidden marks. An empty marked set may be confirmed when the source is nonempty.
Hosts use `set_selection_limits` for a minimum/maximum; unmarked rows remain
reachable at the maximum. `notice()` reports why a bound prevented a change.
Danger rows are never pre-marked but explicit Alt-A includes them.
Loading/error/empty sources cannot confirm. The compatibility `Picker`/`Action`
API retains its original-index semantics and remains the catalog's implementation.

Focused enabled labels use bold terminal-default text without inherited dim
styling. The row band leaves both outer cells at terminal-default background;
unselected keys stay muted. Number keys are centered in three cells, followed by
one space outside the number block before aligned labels. Details start two cells after the longest visible
label and disappear if the full gap/minimum detail width cannot fit. Contiguous
shortcut sets with two or more keys use a range; disabled gaps remain spelled out.
The host activity row uses warning color for `! needs you` and muted shortcut hints.
`ChoicePicker::height(width, view)` lets hosts place the compact view accurately.

The chat fixture has a conversation, status, composer and keyboard hints. Type or
paste; Enter submits a snapshot to the simulator, which acknowledges immediately.
Shift-Enter and Alt-Enter insert a newline where the
terminal reports the modifier; Ctrl-J is the traditional-key fallback.
Pasted newlines also work. Backspace/Delete remove whole graphemes; arrows edit
the draft. Alt-PageUp/Alt-PageDown move a visible transcript page; Alt-Up/Down
move three rows. Alt-Home goes to the oldest loaded row; press again to load
simulated earlier history. Alt-End follows latest output. Manual reading keeps
a source anchor through resize and shows unread above the fixed dock.
Ctrl-F searches loaded content: Up/Down move matches, Esc restores the old view,
Enter keeps the match. Ctrl-X V selects blocks: Up/Down move, Shift extends,
M selects the message, Y copies Markdown, C copies code, O opens/inspects and
Enter folds. Copy/open are simulated and acknowledged locally. Ctrl-X T toggles
timestamps; Ctrl-X R requests retry on a failed reply. Plain editor keys still
belong to Composer. F2 cycles 76 fixtures; F5–F9 simulate host events. Delayed
copy/open/inspection and paging fixtures use F5 to acknowledge success and F6
to refuse the outstanding request. Retrying emits a fresh identified request;
selection, the draft and its caret survive host failure. No agent runs.

```sh
make -C packages/tui run CARGO_FLAGS="-- --component chat --scenario answer"
make -C packages/tui run CARGO_FLAGS="-- --component chat --scenario search-folded"
make -C packages/tui run CARGO_FLAGS="-- --component chat --scenario select-code --colors none --ascii"
```

For headless inventory use `cargo run --locked --features gallery -- --list`
from the package. The interactive gallery refuses piped input/output before
changing terminal modes.

## Composer workflows

Open **Composer**, or run from the repository root:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component composer --scenario main"
```

Enter fixes the sent turn and clears the box immediately. Its dim `sending`
suffix disappears on acceptance. Fresh edits survive acknowledgements; refusal
returns an untouched draft or stashes the refused copy beside newer text.
One draft belongs to the session. The dock stays five rows high while lists and
queue previews grow upward.

Up edits visual lines first, then reaches the first line's start. The next Up
recalls queued messages, then history. Dashed rules identify recalled content.
Esc or Down past newest restores your exact draft and cursor. Editing a history
copy stashes the set-aside draft. Ctrl-R searches history without sending.

Enter while busy queues a message. A successful turn end sends one eligible
entry. Enter in a queued edit saves its slot; Esc keeps the original; Ctrl-D
removes the edited copy into the stash. Errors and stopping pause delivery;
Ctrl-X Enter resumes it. Restart restores queues paused.

Ctrl-X s stashes a draft; Ctrl-X u restores or swaps with the newest stash.
Type `/stash`, Enter to insert, then Enter to open the list: Enter restores, d drops, u undoes until
closing. The stash holds 50 entries and reports eviction of the oldest.
Ctrl-X e requests an external editor. F11 simulates its return in this gallery;
a changed box opens a two-version conflict dialog and stashes the unused version.
F1 opens key help; Ctrl-V requests a host clipboard image.

Long/multiline paste becomes an atomic chip with its full payload preserved.
Pasted prefixes never execute. Typed ! in an empty box enters shell mode;
typed / opens commands and @ at a word boundary opens files. Tab and Enter insert
the selected suggestion; a later Enter invokes the command or sends the message.
File selection inserts an atomic `@path` reference followed by a space. One
Backspace removes the reference; undo restores its full path and host ID. Shell
paths stay editable text. Saved documents distinguish paste, image and file kinds.
The host supplies candidates and performs execution, clipboard and file work.

Escape follows the local cancellation ladder: close overlay, restore recall,
cancel editor waiting, stop work and pause queue, or arm draft clearing/rewind.
Repeat within 1.5 seconds to confirm. Ctrl-C clears idle text; on an empty idle
box, Ctrl-C or Ctrl-D twice saves and requests quit. **F10** opens scenarios;
F10 again returns with the current fixture intact. Picker search, selection and
scroll position survive back navigation. F2 cycles all 97 distinct fixture
states; aliases also appear in `--list`. F4 changes colors; Ctrl-T changes theme.

Gallery host controls: F5 accepts a submission, confirms a pending work control
or successfully ends a turn; F6 refuses or fails it. Cancellation and background
requests remain pending until that simulated host response. F7 replaces the draft lifetime; F8 tests a stale response;
F9 changes availability; F11 returns editor text; F12 restores a saved snapshot;
F3 expands/collapses fixture shell output. Gallery storage and execution are
simulated. The public [composer contract](../specs/components/composer.md) records the
layout, state transitions, limits and host adapters.

`Composer::update` returns an optional immutable `Submission`. Hosts project it
with `Chat::push_submission`, then call `acknowledge_submission` with its exact
ticket. Call `turn_ended` once per completed turn and project any returned queued
submission. Host acceptance and turn completion are distinct callbacks.
Drain `take_intents` for Save, Editor, identified Work, Command, ClipboardImage and other
host work. Advance `tick` with elapsed time for 500 ms autosave and confirmations.
`WorkRequest` identifies Stop, InterruptAfterStep or Background. Keep foreground
work active until `acknowledge_work` consumes that exact ticket; project its
optional queued submission once. Cancellation failure preserves the draft and
permits a new attempt. Authoritative turn completion invalidates pending controls.
Without Run Activity, `Chat::update` exposes a stop through the compatibility
`Action::Cancel`; obtain its retained identified request from
`composer().work_request()` and acknowledge it after the host responds. A standalone
Composer emits `Intent::Work` directly. With Run Activity installed, Chat emits its
identified run-cancellation intent instead.
After background acknowledgement, `finish_background` consumes the same identified
command's eventual completion. Hosts supply elapsed work time with
`set_working_elapsed`; `set_reduced_motion` freezes dots without freezing seconds.
`EditorCancelled` means stop waiting for that return; the host still owns its editor.
Save acknowledgements and editor/completion results require their current ticket;
late results leave content unchanged.

`SavedComposer::write_to/read_from` encode complete state on caller-owned streams;
`restore_saved` validates before applying and never sends automatically. Schema 2
retains typed file references, and decoding accepts schema 1 paste/image snapshots.
Report read/decode failures with `report_restore_failure`; failed writes retry
after five seconds while preserving memory. Hosts
choose storage keys and finish the current Save before honoring Quit.
`editor_command` resolves supplied VISUAL/EDITOR values; `run_editor` is an
explicit foreground adapter using a private temporary file. Wrap foreground work
in `TerminalSession::handoff` and handle operation and reacquisition errors.
GUI scheduling, process cancellation and real agent integration remain host duties.

## Multiline editing and focus

Open **Multiline editor** in the catalog, or run
`cargo run --locked --features gallery -- --component editor --scenario main` from the package.
Top and bottom editors have independent drafts; Tab/Shift-Tab switches focus.
F3 opens a temporary editor modal. Only it receives typing/paste;
Esc clears selection first, then closes it and restores the prior editor.
Esc clears selection before returning to the scenario picker too. Ctrl-C exits the gallery.

| Input | Behavior |
| --- | --- |
| Left/Right, Backspace/Delete | Move/remove a grapheme |
| Ctrl-Left/Right or Alt-Left/Right | Move to Unicode word boundaries |
| Ctrl-Backspace/Delete or Alt-Delete | Delete to a word boundary |
| Up/Down | Move by visual wrapped line, retaining preferred column |
| Home/End, Ctrl-A/Ctrl-E | Visual wrapped row start/end |
| Ctrl-Home/End | Draft start/end |
| PageUp/PageDown | Move one editor viewport |
| Shift plus movement | Extend selection; typing/paste replaces it |
| Ctrl-Shift-A or Ctrl-G | Select all; Ctrl-G works with traditional terminal encodings |
| Ctrl-W | Kill back through the preceding non-whitespace span |
| Alt-Backspace / Alt-D | Kill previous word part / next word |
| Ctrl-U/Ctrl-K | Kill to logical line start/end; repeat at the edge joins lines |
| Ctrl-Z or Ctrl-minus | Undo one transaction; paste is one transaction |
| Ctrl-Shift-Z or Alt-Z | Redo; a new edit clears redo |
| Ctrl-Y / Alt-Y | Yank newest kill / cycle the last ten kills |
| Enter | Host submission; the gallery records a local result |
| Ctrl-J | Newline in every multiline field |
| Alt-Enter or Shift-Enter | Newline when terminal encoding reports the modifier |

Host owns submission/focus; `Editor::update` leaves plain Enter
unhandled. Gallery records submission in its standalone fixture. Neither editor
nor focus state acquires modes. Clipboard I/O/persistent history remain host work.

F2 cycles focused, blurred, empty, wrap, selection, Unicode, scrolled, pasted,
undo, near-limit, limit, paste-cut, kill-ring, word-parts, line-kills and typed-refused.
Additional fixtures cover blurred-scrolled, blurred-limit, tiny-3/2/1,
paste-refused-whole, chip-cap, control-characters and typed-cut (an IME commit).
The limit fixtures use 120 bytes; ordinary
drafts use 4 KiB. The editor uses its full width with title/footer rules and no
side borders. Prose wraps by word; longer words break at grapheme boundaries.
Up/Down follows those rendered rows; logical line/column metadata counts graphemes.

Oversized input inserts the complete grapheme prefix that fits, in one undo
transaction, and reports discarded bytes. If no grapheme fits, draft/selection
remain intact. The byte counter appears at 90%; a reached limit persists until
an edit makes room. `set_text` truncates host replacement at a grapheme boundary
and resets selection/undo. Undo keeps 64 transactions. Typing/deletion groups
until a one-second pause, movement, selection replacement or paste; trailing
whitespace ends a typing group. Paste is always one separate transaction.

Paste normalizes CRLF/CR to LF and retains tabs at four-cell display stops.
Other stored controls render as dim control pictures or ASCII hex labels; they
never emit escape sequences. The host receives the original controls on submission
and owns validation/execution. Resize preserves text/caret; release events do not
edit. Focus changes preserve the draft, selection and viewport. The terminal's
cursor retains its own shape/blink; the library does not install a custom cursor.

`EditorView` supplies title, placeholder, footer and `Presentation`; `chrome=false`
renders only its text area, as used by the picker's single-row filter. Edit notices
temporarily override the footer, and pasted text is tinted until the next key or
three seconds. Hosts call `Editor::tick(elapsed)` to advance time; the editor owns
no timer. Empty footer props use line/column and selection metadata.

## Dialogs

Open **Dialogs** from the catalog, or run from the repository root:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component dialog --scenario main"
```

F2 cycles prompt → nested → long → locked → outside → removed-return → error →
confirm → notice → prompt-long → rename, then the arrival, closure, confirmation
and profile fixtures. F10 lists all 36 named scenarios.
Prompt plain Enter returns local text. Shift-Enter, Alt-Enter and Ctrl-J add a
newline; paste never submits. Unix terminal sessions request enhanced keyboard
reporting and restore it on exit. Modified Enter requires the terminal to report
the modifier distinctly; Ctrl-J remains available with traditional key reporting.
Escape clears an editor selection before closing one layer. Tab/Shift-Tab stays
inside the current modal scope. Nested starts with a picker above a parent draft;
choosing returns only that picker's ID. Long inspection uses arrows/page/Home/End;
Enter acknowledges. Locked offers a safe Keep/No choice; Escape shows “answer to continue”.
Error stays visible on Enter; F6 retries by replacing it with a prompt.

F3 opens a child prompt. F5 removes background draft one's return target; closing
returns to draft two. F6 replaces the active dialog, preserving its parent. F7
checks the fixture's saved ticket; after replacement/close it reports a rejected
stale result without changing a draft. F8 clears all layers and restores base
focus. F9 simulates an outside left press; only the outside fixture opts into
dismissal. The gallery does not enable mouse capture. These function keys are
host test controls, not library dialog bindings.

Two background drafts and the dock's independent message draft are read-only
and faded while a modal is open. After closing/clearing, Tab switches between
their surviving focus targets. F2 preserves drafts; return to catalog and reopen
resets the fixture. The panel centers over the visible screen, at most 72 columns,
and screen width minus four, with one column of side padding and a lighter surface.
The dock keeps its position.
Prompt fields grow to six rows before scrolling.
Nested titles form a trail; only the top body is visible. Confirm defaults to
keeping the draft; notice returns a local acknowledgement; rename demonstrates
scope, message and named action footer. No real operation executes.

Indexed/RGB profiles fill the surface around a terminal-background field.
ANSI 16/no-color use a full box and the field's own rules. Narrow layouts wrap
messages, drop scope and shorten the footer. Boxed prompt scroll hints appear
only on the field's line. ANSI 16 hints use the default foreground with faint;
no-color reverses only the focused field title, with plain background content.
Below three rows the body uses all
space without chrome. The visible activity row stays live while the rest of the
dock fades; it remains in place after closure. In an open dialog Ctrl-C/Ctrl-D
first arms closure, then closes only the top layer. Neither press quits.

`waiting` keeps typed text in the dock until a one-second pause or Ctrl-G.
`key-guard` ignores Enter/digits for the first 300 ms while other letters type.
`away` simulates an attention request; it sends no real notification.
`child-removed` restores its parent and shows a three-second reason.
`danger-armed` requires a second deliberate Enter within three seconds;
`danger-expired` requires arming again. Numbers only move to a danger choice.
`type-name` requires the exact displayed name; paste never confirms.
`inside-error` retains the editable field and corrective footer.

For hosts, `dialog::Stack<B,T>` owns scopes and caller-supplied bodies. `dispatch`
calls only the top handler, consumes Tab/release and outside pointer events, and
offers Escape to the body before applying `ClosePolicy`. Unhandled modal input
never reaches the background. The caller decides when a body's explicit result
closes the current ticket with `Reason::Submitted`. `replace` and `clear` return
dismissal records/bodies; cancel host work from those records. `body_mut(ticket)`
accepts only the active opening, rejecting foreign, obscured, closed or replaced
tickets. Recheck availability/authority in the host before executing any result.
Render the background unfocused before the stack; the stack fades its visible
cells while open. `DialogView` supplies scope, message and action
footers. `Stack::render_with` receives a body measurement and renderer, rendering
only the top body with its focus target. The renderer may return a scroll hint
for the title rule. Each body owns its scrolling and render/input contract.

Use `Stack::new_timed` for the supplied host policies. Report actual background
typing with `typed`, elapsed time with `tick`, and terminal focus with
`set_terminal_focused`. `arrive` opens or queues at most sixteen roots in host order;
`push` nests explicitly. `activity` supplies the live row, and `take_attention`
returns identified notification requests for the host to consume.
`remove`/`clear_with_reason` provide timed notices. `update_view` updates active
failure labels without replacing the body. `DialogView::message_error` selects
the error role. `Confirmation` qualifies irreversible or exact-name actions using
injected time; the host supplies stable revision-specific action IDs, clears
arming when selection/details change, and owns the resulting external effect.
`Editor::visual_rows` measures wrapped field content; `ChoicePicker::render_list`
renders choices inside host-owned dialog chrome.

## Presentation profiles

The gallery resolves environment hints once before entering the terminal:
nonempty `NO_COLOR`, then `COLORTERM=truecolor`/`24bit`, then `TERM` containing
`256color`, otherwise ANSI 16. Explicit command options override that choice;
an explicit color profile emits colors even when `NO_COLOR` is set. Auto mode
still respects `NO_COLOR`. RGB output requires a terminal that supports it;
use `--colors ansi256` for the indexed palette when RGB is unavailable.
An explicit `truecolor` option requests RGB output without verifying support.

```sh
make -C packages/tui run CARGO_FLAGS="-- --component editor --scenario main --colors truecolor"
make -C packages/tui run CARGO_FLAGS="-- --component editor --scenario main --colors truecolor --light"
make -C packages/tui run CARGO_FLAGS="-- --component editor --scenario main --colors none --ascii"
```

Tokens cover body/dim/accent/success/warning/error/info, surface and selection;
spacing is measured in cells. ANSI 16 uses the user's palette and reverse
selection. Extended modes use selection tint; no-color uses bold/reverse without
color overrides. ASCII affects decorations and control pictures, preserving
Unicode document text. Components receive explicit profiles and never inspect
environment variables or query the terminal. Automatic background detection
through OSC 11 is not implemented; select light tokens explicitly.

## Local consumers

Drone, Shield and Gate declare `canonic-tui` as an optional, versioned local
path dependency. Enable their `tui` feature; no package registry is involved for
this library. Upstream dependencies are resolved through their Cargo lockfiles.

```rust
use canonic_tui::{picker::{Action, Picker}, event::{Event, KeyCode, KeyEvent, KeyModifiers}};

let mut picker = Picker::new("Review", vec!["Allow once".into(), "Cancel".into()], Some(1));
let result = picker.update(&Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
assert_eq!(result, Action::Selected(1));
```

The host dispatches events and renders components in its Ratatui frame. Components
return actions; the host decides how to execute or reject them. `Chat::push`,
`append`, `set_status` and `set_input` support plain hosts. Rich hosts use
`push_record`, revision-addressed `apply`, `resynchronize`, `finish_turn` and
`take_intents`; acknowledge copy/open and complete opaque paging requests.
Drain Composer intents separately for saving, commands and external editor work. `chat::Action::Submit`
carries a `Submission`; acknowledge its ticket through `chat.composer_mut()`.
Submission clears the box immediately; acknowledgement preserves fresh edits.
`TerminalSession` is an optional host helper
for one full-screen event loop; inline integrations can own their own runtime.
Never acquire another terminal session for a nested picker.

## Completion Menu

```sh
make -C packages/tui run CARGO_FLAGS="-- --component completion-menu --scenario main"
```

The scenario picker offers all 30 menu states, plus `narrow-40`, `ansi16`,
`no-color` and `ascii`. Up/Down or Ctrl-P/N select enabled rows; page keys move a
menu height without wrapping. Tab and Enter insert with a trailing space. Nothing
runs or sends until a later Composer Enter. `command-inserted` demonstrates an
argument hint: the faded `<pr>` is display-only. `file-inserted` remains editable.

Typing filters the current token. Left/Right/Home/End move the box cursor; leaving
the token closes the menu. Escape keeps the draft and dismisses that token.
Paste never opens suggestions. A new typed trigger can open another menu.
`loading` and `still-loading` finish locally after three seconds; `error` retries
on Tab without changing the draft. With no selectable match, Enter sends literal
text. `no-match-file` offers an explicit closest guess that only Tab inserts.
Partial-source errors keep good results selectable. Alt-R retries only failed
or timed-out sources, preserving good results and the draft. Ctrl-R opens
Composer history search. Menu insertion keys appear above the draft; the footer
keeps Composer's Enter behavior. `timeout` shows a warning distinct from source
errors. `shell-path`, `replacement-too-long`, `removed-before-enter`,
`wide-name-clipped` and the menu profiles exercise recovery and rendering.
After `removed-before-enter` refuses invocation, the next explicit Enter sends
the unchanged draft as literal text. F1 closes completion and opens Composer help.
F2 cycles states, F10 opens
scenarios and Esc returns after local dismissal. Ctrl-C quits this component.

For standalone use, construct `completion::Menu` with named sources and validated
`Candidate` values. Send editor-owned query changes through `set_query`, tick the
host clock, and poll `take_request` after the 60 ms debounce. Apply each source's
accumulated `Batch` using that exact opaque request. Loading appears after 150 ms;
five seconds without results yields a retryable warning reported by
`timed_out_sources`; `failures` reports source errors separately. Finished sources reject
duplicate batches. Close only after successfully applying `Action::Insert` to the
editor; a failed budget check should preserve the menu. `Action::PassEnter` lets
the host handle literal submission. Rendering keeps focus and the host cursor.

Composer provides `set_completions` for cached command/file rows and
`completion_request`/`complete` for a single host source. Named asynchronous
sources use `set_completion_sources`, `completion_due`, `complete_source`,
`set_completion_guess` and `retry_completion_sources`. Hosts handle
`Intent::RetryCompletion` with discovery I/O. A later Enter emits `Command(id)` or
`CommandWithArguments { id, arguments }`; hosts validate availability and execute.
Completion insertion never performs execution, filesystem or clipboard access.
`completion_hint_for` supplies the activity row; `footer_hint_for` supplies
Composer help below the dock. Candidate aliases are bounded and rank after name
prefix/word matches; tags are host-provided `kind · source` labels.

## Tool Call Card

```sh
make -C packages/tui run CARGO_FLAGS="-- --component tool-call-card --scenario main"
make -C packages/tui run CARGO_FLAGS="-- --component tool-call-card --scenario many-at-once --colors none --ascii"
```

The picker offers 79 scenarios and eight standard main-view profiles. Cards
present proposed, queued, running, waiting, cancelling, completed, failed,
interrupted and cancelled work using host
summaries. Failed cards show three initial error lines; text previews show the
last three. Enter expansion shows the first ten and last ten, with hidden counts.
Concurrent groups preserve host order and fold completed successes; failures stay
visible. Narrow cards retain filenames, wrap error/diff lines and drop numbers.

Typing stays in Composer. Ctrl-X V enters Chat selection; arrows choose a card
or group. Enter expands/collapses. `o` requests details, `r` requests a retry for
enabled failed/cancelled cards, `y` copies call and retained output, and `c` copies
output only. Hints show available actions. Escape closes details/selection first;
in Compose it stops the whole working turn. F10 returns to scenarios without
resetting the draft, caret or card selection.

The deterministic gallery acknowledges simulated effects. Details opens a full
retained-output view: arrows/page/Home/End scroll, `/` searches literally, `n`
finds next and `y` simulates copy-all. Escape returns to the same card. F5 steps
queued/waiting/running work, F6 fails active calls, F8 grows output and F7 resolves
held effects. The copy-failed/details-failed/retry-refused scenarios report host
refusal; no-ack keeps requests pending, and stale-ack rejects an obsolete ticket.
No clipboard, agent, process execution or permission grant occurs.

Host snapshots may include measured `Progress`, sixteen identified `Artifact`
links and a `ChildTask` with ten latest-call lines. Only a supplied total produces
a progress bar. Child details emit `Effect::OpenTask`; deliberate artifact commands
emit `Effect::OpenArtifact`. `Chat::open_tool_artifact` routes a supplied artifact
ID through the same capability and acknowledgement checks. The gallery's selected
artifact card uses `a` to choose a link and opens deterministic local inspection.
Groups accept host reordering while expansion and selection follow call IDs.

Attach whole-run state with `Chat::begin_run(run_activity::Snapshot)` and supply
newer revisions through `update_run`. Drain `Intent::Run` for cancel/options
requests. Escape and Ctrl-C send one identified cancel while the draft stays
editable and Enter keeps queueing. Report cancellation with a final snapshot;
`fail_run_cancel` supplies a host failure and allows another deliberate request.
Completion clears a pending cancel without relabelling it cancelled. Connection
comes through `set_run_connection` and never implies a run outcome. The run gallery
holds cancellation until F7 confirms, F5 completes first or F6 reports failure;
F8 toggles connection. `set_run_reduced_motion` keeps working dots static while
the host supplies elapsed seconds.

For library use, construct `tool_call_card::Call` and `ToolCallCard`, dispatch
`Command` only from the selected focus scope, render into the caller's frame and
tick elapsed time. `replace` accepts newer revisions of the same call and keeps
local expansion. Effects carry call ID, revision and opaque `Ticket`; call
`acknowledge` only after the host effect finishes. Retry acceptance requires a new
call ID, preserving the original. `ToolGroupCard` composes ordered calls.

For Chat, put `Content::ToolCard(Box::new(call))` or `Content::ToolGroup(group)` in
identified parts and apply versioned snapshots. Drain `Intent::Tool`; resolve
copy/details/refusal with `acknowledge_tool`. `complete_tool_retry` consumes the
matching pending ticket and atomically inserts the accepted successor below its
parent. The host still validates execution/authority and owns full output.
Chat retains its 64 KiB per-turn source limit; omitted bytes are disclosed and
Details lets a real host fetch the complete source. See the
[public contract](../specs/components/tool-call-card.md).

## Session Browser

Open the dedicated-screen fixture:

```sh
make -C packages/tui run CARGO_FLAGS="-- --component session-browser --scenario main"
```

Up/Down, PageUp/PageDown and Home/End navigate host-ordered sessions. Enter opens
an offered session after acknowledgement; Enter on the originating session
returns to its Chat. Slash edits a literal loaded search. Titles appear before
message-only matches, preserving host order within each set. Move to the final
Search all row and press Enter to request remote results. Loaded matches stay
usable while waiting. New queries invalidate older search and remote-page
tickets. Escape stops a remote request, leaves the search field, clears the
filter, then returns to Chat, one step at a time.

A opens offered row actions, including readable disabled reasons. R renames in
an Editor with the old title selected; Enter saves, Escape cancels. Over-limit
text stays editable, and resource overflow or multiline paste is refused whole.
F requests a fork, M chooses a host destination, and P requests pin/unpin.
Pending rows are dimmed with a verb; other rows stay usable. Failure shows the
host reason. A refused rename retains its typed draft for explicit save/retry.

D opens full host deletion wording with Keep selected. Numbers highlight only.
Choose Delete, then press Enter twice within three injected seconds. Movement,
paste, expiry or changed material disarms it. PageUp/PageDown inspect long loss
wording. Only a successful authoritative result removes the row.

At 100 columns the browser shows activity and a read-only supplied preview;
50–99 columns keep metadata columns, and narrower views use two lines per row.
Escape remains visible, and the shared color/ASCII/light profiles apply.

For a consumer, construct `session_browser::SessionBrowser` from a validated
`Snapshot` and originating ID. Offer operations explicitly with `Offered`; a
new `Session` offers none. Feed caller events/time/frames, drain `take_intents()`,
and return exact requests to `complete()` or `complete_discovery()`. Successful
row changes supply a valid newer whole snapshot; fork supplies its actual new
ID, while Open returns `Intent::Opened` for the host to select a retained Chat.
Malformed results keep the operation pending; stale/foreign/duplicate results
are rejected. Material action/title/destination changes require higher row
revisions, while presentation facts may refresh without cancelling work.

`Chat::set_sessions_available(true)` enables Ctrl-X O. The host's `sessions`
palette command calls `Chat::request_sessions()` and handles the same
`chat::Intent::Sessions`. Keep each Chat instance, including its Composer and
queue, instead of resetting or serializing it when changing screens. Returning
or opening another session never acquires a terminal or creates storage.

Gallery F5 succeeds, F6/F7 fail/refuse, F8 refreshes material, F9 removes the
selected row and F12 delivers an old result. `before-chat` exposes Ctrl-X O;
F1 opens the simulated palette. `queued-drafts`, `switch-back` and `streaming`
demonstrate retained per-session state. All operations are fixture simulations.
The [public contract](../specs/components/session-browser.md) owns limits and
lifetime rules; [human review](../specs/human-testing.md#session-browser-review)
remains a separate acceptance checkpoint.
