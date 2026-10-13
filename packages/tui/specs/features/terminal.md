# Terminal ownership, compatibility and presentation

Status: full-screen and bounded inline leases, explicit cleanup errors, foreground
release/resume and an ordered print ledger are implemented. Wider native
compatibility remains unverified. [Inventory](../inventory.md) records the scope.

## One terminal owner

Host explicitly acquires one lease and chooses inline or full-screen. Acquisition
checks input/output interactivity and required capabilities before changing modes.
Import and component construction perform no I/O. Noninteractive refusal emits
no escape bytes; host supplies plain fallback. Components receive frames/events
but cannot acquire raw mode or poll/read terminals. Nested views share the lease.

Lease tracks successfully acquired raw/paste/screen/cursor/mouse/enhanced-input
state. On partial startup failure, restore each acquired mode and release the
guard. Normal explicit close returns cleanup errors while attempting all steps;
drop/unwind is a best-effort backstop. Repeated close/drop is idempotent. Preserve
original operational error alongside cleanup error. Reacquisition is possible
after close/failure. Restore previous mode settings where APIs support capture;
otherwise document the exact owned mode transition rather than promising an
unknown terminal baseline.

No global panic hook may silently replace a consumer hook. A host can arrange
unwind/handled-signal cleanup; a reusable library cannot restore after SIGKILL,
power loss or process abort. Process-local guard is not cross-process arbitration.
PTY mode assertions, panic paths and injected I/O failure prove distinct boundaries.
Never signal or reset unrelated user terminals/processes in verification.

## Full-screen and inline coexistence

Full-screen acquires alternate screen, raw input and bracketed paste. Unix
sessions push disambiguated keyboard reporting and pop it on exit so supported
terminals distinguish modified Enter from submission, following the shared
[keyboard contract](interaction.md#keyboard-contract-and-terminal-quirks).
It draws only caller-owned frame contents and returns to the previous screen on release.
Inline retains ordinary scrollback and draws a bounded interaction region after
the host prompt. It acquires only required raw/paste/cursor modes, never alternate
screen by default. Exit leaves a readable confirmed/cancelled result or clears
temporary rows according to an explicit host policy, then returns the cursor to
where ordinary output resumes. Window shrink and scrolling must not overwrite
unrelated preceding output; if safe anchoring is unavailable, use host plain fallback.

Chat dialogs follow the [dialog panel contract](../components/dialog.md#official-dialog-design):
temporarily enter an alternate screen, redraw the visible conversation tail there,
and return on close. The inline gallery borrows that screen with the existing lease.

Inline adapter owns cursor-region bookkeeping and redraw, not picker decisions.
Same picker/review renders into a bounded area and returns same intent in both
modes. One inline interaction at a time; no hidden interaction on hook/protocol
paths. Gate and Shield choose interactive invocation eligibility themselves.
Initial inline acceptance uses short menus/review; arbitrary rich full-screen
layouts in scrollback are outside that first slice.

## Inline runtime API

`runtime::InlineSession` acquires modes only after host-resolved cursor capability,
TTY and initial size checks. A real cursor report establishes its first anchor;
unknown/missing reports return an error for plain fallback. `InlineOptions::height`
is the preferred content height, capped at window height minus two rows. A fixed Ratatui
viewport occupies freshly reserved rows. It never uses the inline autoresize path
that can clear the normal screen when width shrinks. Size changes abandon stale
coordinates and reserve fresh rows at the bottom; existing terminal reflow artifacts
are not erased. Resize events invalidate the anchor even while the alternate
screen is borrowed and even if the size returns to its original dimensions.
Hosts using another event source call `invalidate_anchor` for any resize;
the provided `poll` records these events. `redraw` invalidates only the
current owned live buffer.

Hosts supply component measurements to `set_height`, without reacquiring terminal
modes or replacing component state. Growth clears the old owned rows and reserves
only the additional rows below them, scrolling preceding output into normal
history if needed. Shrink clears the previous owned region and keeps its top
anchor; abandoned rows are blank. Equal effective heights perform no redraw I/O.
Zero is rejected without changing the accepted height or stopping the lease.
Terminal failures stop further drawing. During alternate borrowing, only the
preferred height changes; normal-screen fitting occurs on return. Window shrink
clamps the visible region while retaining the preference for later growth and
foreground resume. Known resize invalidation always takes precedence over old rows.

The inline gallery uses `ChoicePicker::height`, `QuestionForm::height` and
`PermissionReview::height`. Chat exposes pure `inline_height` for its fixed dock,
above-dock content and active review allowance; hosts add the wrapped live records.
It excludes printed history and measures with the current width/presentation.
The gallery preview and native host use the same measurements. Notices retain an
explicit row, and insufficient room for the component still blocks confirmation.

Hosts gate affirmative input with `confirmation_available`; undersized regions
still offer cancellation. `print_before` accepts at most 64 KiB per record, strips
control effects, wraps in terminal cells, prints above the region and reanchors it.
All writers must participate in this host route while a lease is active. Direct
external writes are not arbitrated. On `enter_alternate`, the same lease borrows
the full screen; `leave_alternate` returns to the normal region and drains deferred
output in order. The queue is capped at 128 records and 1 MiB. Overflow is explicit.
Host state remains alive across transitions. A write failure stops further drawing;
finish attempts mode restoration and reports errors without retrying partial writes.

`pause` clears only owned rows and returns a released `PausedInline` token.
`resume` checks the actual terminal and obtains a fresh anchor below intervening
foreground output. Unix tokens preserve the initial termios baseline across
handoffs. These APIs do not send job-control signals. `finish(result)` restores
modes and emits plain result text; `finish("")` is the explicit clear policy.
Drop restores the lease as a backstop but supplies no completion result.

`inline_composition::PrintLedger` owns write-once output independently of rendering
and terminal I/O. A per-turn snapshot has at most 256 identities, 1 MiB of supplied
text/labels/IDs, 60 KiB per text, 128-byte IDs and 256-byte labels. Retained accepted
parts remain bounded by 256 identities. Settled records wait for unsettled
predecessors. `ready` freezes one exact batch; updates during a pending write do not
change that batch. Exact acknowledgement records its printed versions. Duplicate,
foreign and altered receipts are inert. New revisions of printed records append
`↳ label → text`. Atomic replacement rejects duplicate IDs and stale/materially
inconsistent revisions, including outstanding batches. `abort` stops replay after
an ambiguous write. No notice, status, permission authority or execution belongs
to this ledger. Hosts insert explicit retry/rewind markers as fresh ordered records.

`Chat::render_inline_live` reuses Composer/activity/reviews and excludes the loaded
transcript. The host renders live records above its dock. Transcript search,
selection and reading use a retained full-screen Chat view. The gallery additionally
borrows views for Composer overlays, sessions, tree, inspection and status/notices.
It has no sidebars beside normal scrollback. The native CLI is opt-in; list/help
and existing consumers acquire no inline lease implicitly. Plain picker/review
fixtures have typed-number input without defaults; empty/EOF cancels or retries.
Plain Chat reports its retained draft and ends without submission.

Remaining scope: general external log-writer coordination and actual product
adapters are host integration work. Windows ConPTY, emulator reflow, multiplexers, IME and
assistive testing are separate acceptance. The handoff's unresolved remove-key,
cancel/skip glyph, filter-clear hint and Tab wording choices do not change current
component bindings in this increment.

## Suspension and asynchronous lifecycle

`TerminalSession::close` reports restoration failures. `handoff` consumes the
lease, releases modes before a foreground host callback and reacquires afterward;
operation and reacquisition results remain separate. Release failure skips the
callback. Unix restoration preserves the original termios settings. The owned
PTY test verifies a cooked editor callback, raw reacquisition and final restored
shell input. Both gallery hosts bind Ctrl-Alt-Z to an explicit Unix host
adapter; Ctrl-Z retains editor undo. It verifies controlling-terminal foreground
ownership, pauses the lease, then sends SIGSTOP only to its own process. SIGCONT
from a background job stops again before mode acquisition or drawing. Foreground
return resumes below intervening output, measures the window, and restores any
borrowed alternate view with retained host state. Full-screen resume uses the
runtime's existing handoff and repaints with current geometry. Acquisition failure
returns an error with modes released. Unsupported hosts retain the active lease
and show an unavailable notice. Native Linux/Windows acceptance remains separate.

`gallery/job_control.rs` owns a SIGTSTP flag registration for an interactive Unix
foreground gallery. Signal delivery only sets an atomic flag. The owning event
loop handles it before drawing and after polling, retains any polled event,
releases the lease, and stops through the same foreground-return path. Component
state, focus, pending input bursts and host data remain in the loop. Active fixture
clocks restart on return without advancing through suspended time. Registration
is absent from plain/list/help paths and gallery-disabled library imports.
The guard unregisters its flag on exit; signal-hook retains its process-wide
dispatch shim rather than restoring the previous OS disposition. This adapter is
for the gallery CLI lifetime, not an embeddable arbitrary signal-policy manager.
Uncatchable SIGSTOP bypasses cleanup; external termination signals and custom
pre-existing signal handlers have separate policies/acceptance.

Suspend flushes draw state, releases owned modes and
marks lease unavailable for rendering. Host owns launch/signals/child wait.
Resume checks terminal/capabilities, reacquires modes, measures size and forces
redraw. Resume failure returns an error and leaves a clean noninteractive state;
host retains draft/data. POSIX suspend and Windows external-editor handoff have
different host mechanics and separate acceptance.

Event loop must service input, resize and host updates fairly. Components do not
sleep, await network requests or spin animation loops. Host bounds update batches,
coalesces redraws without dropping semantic events and observes worker completions.
Clipboard/editor/file/search operations have request generations and cancellation
owned by host. Screen close invalidates late updates and owned callbacks.
Logs go through host facilities; direct writes during a lease cannot corrupt frames.

## Text and Unicode

For transcript content, parse/remove complete ANSI/OSC/DCS/control sequences at the display boundary,
including fragmented input across a host-supplied streaming normalization context.
Stripping ESC alone can leave misleading remnants. Do not replay hyperlinks,
clipboard, title changes, bells or cursor movement from content. Stateful sanitation
is scoped per stream/part and reset on replacement, never shared globally.
Host keeps original content separately if it needs archival fidelity.

Editor drafts retain controls for editing and host validation. Their renderer
uses printable control pictures or ASCII hex labels rather than raw controls;
it never replays escape effects. See [Presentation](../presentation.md).

Text bounds maintain valid UTF-8 and grapheme boundaries. Layout measures terminal
cells, handles combining marks/CJK/emoji/zero-width sequences, and uses replacement
for a glyph wider than its area. Ambiguous-width policy is explicit. Newline/tab
normalization distinguishes draft content from display mapping. Invisible/bidi
control handling needs a documented display policy, without claiming complete
Unicode spoof protection. Source-to-row maps drive cursor, selection and anchors.
Display truncation is labeled and bounded by bytes/rows/cache limits.

## Themes and accessibility

Semantic roles include base/panel/muted/focus/selection, enabled/disabled,
success/warning/error/info, role labels, syntax and diff added/removed/context.
Pass immutable values per render; no global mutable palette or file watcher.
Supply dark/light, terminal-default and no-color profiles. Unknown color support
uses conservative ANSI/default styles; truecolor/256-color are explicit capabilities.
Custom palette validation and missing-role fallback are pure; discovery/persistence
remain host-owned. Preview cancellation restores caller's original palette.

Never communicate permission, error, focus or diff meaning solely by color.
Use labels/signs/markers. Keyboard hints are generated from effective bindings.
ASCII border/symbol fallback and reduced-motion profiles keep status readable.
Only active editor shows cursor. Text/plain export and host headless paths remain
available; terminal screen-reader behavior requires specific native testing.
Do not claim assistive-technology compliance from snapshots. IME, terminal width
tables, multiplexer passthrough and enhanced keys are compatibility questions.

## Native compatibility matrix

| Surface | Required evidence before support claim |
| --- | --- |
| macOS terminal | Raw/paste/resize/restoration, Unicode, inline region, suspend/editor and capability fallbacks |
| Linux terminal | Same native evidence, POSIX handled-signal/suspend behavior and available locales |
| Windows Terminal / ConPTY | Native key/paste including CR-only newlines, resize, cursor/mode restoration and editor handoff; no POSIX job-control claim |
| Traditional keys / limited color | Newline fallback, no-color/ASCII, unsupported enhanced keys and readable focus |
| SSH / tmux / screen | Explicit capability profile, paste/resize/selection/clipboard behavior; no automatic passthrough assumption |
| Noninteractive | List/help/plain host paths without modes/escapes; refuse interactive acquisition |

Record emulator/version, OS, locale, TERM/profile and capture settings. The current
Unix PTY test provides narrow automatic assertions, not this complete matrix.
Cross-compiling and TestBackend rendering do not establish native acceptance.

## Acceptance criteria

- **T-01:** Single lease, startup failure, explicit close/drop/unwind/reacquisition
  restore owned modes; cleanup errors are observable; piped acquisition emits no escapes.
- **T-02:** Inline picker/review preserves surrounding output and resumes ordinary
  printing on confirm/cancel/resize/error; full-screen returns to previous screen.
- **T-03:** Suspend/resume/editor success/failure restore modes/size/redraw and
  retain data; unsupported capabilities return an explicit fallback outcome.
- **T-04:** Complete and fragmented control sequences cannot cause terminal effects;
  Unicode/cell mapping, truncation and pathological narrow layouts remain valid.
- **T-05:** Themes/no-color/default/ASCII/reduced-motion retain labeled meaning,
  valid focus/cursor and per-instance isolation without settings I/O.
- **T-06:** Native matrix results and limitations are recorded per profile; untested
  Windows, multiplexer, IME and assistive behavior remain unclaimed.
