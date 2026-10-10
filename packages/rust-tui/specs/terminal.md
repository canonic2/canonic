# Terminal ownership, compatibility and presentation

Status: foundation lease exists; inline mode, explicit error reporting and wider
compatibility are proposed. [Inventory](inventory.md) qualifies existing cleanup.

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

Chat dialogs follow the [dialog panel contract](dialogs.md#official-dialog-design):
temporarily enter an alternate screen, redraw the visible conversation tail there,
and return on close. This transition remains pending the inline adapter.

Inline adapter owns cursor-region bookkeeping and redraw, not picker decisions.
Same picker/review renders into a bounded area and returns same intent in both
modes. One inline interaction at a time; no hidden interaction on hook/protocol
paths. Gate and Shield choose interactive invocation eligibility themselves.
Initial inline acceptance uses short menus/review; arbitrary rich full-screen
layouts in scrollback are outside that first slice.

## Suspension and asynchronous lifecycle

Runtime offers explicit suspend/resume transitions for an external editor or
host-requested job control. Suspend flushes draw state, releases owned modes and
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
it never replays escape effects. See [Presentation](presentation.md).

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
