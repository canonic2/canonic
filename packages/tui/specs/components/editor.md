# Editor

## Purpose

Provide reusable Unicode multiline editing with text, cursor, selection, undo
and viewport state owned by one editor instance.

## Current status

Status: implemented reusable multiline editor, with human acceptance tracked in
[Human testing](../human-testing.md). Hosts own focus, area, clocks and submission.
The editor owns Unicode text, cursor, selection, undo and viewport state.

## Behavior and state transitions

### Editor workflow

Text is UTF-8 with LF newlines. Cursor movement and deletion cross whole
graphemes, including combining marks, CJK and joined emoji. Up/down use visual
wrapped rows and retain a preferred cell column. Selection, word movement,
Home/End, Ctrl-A/E, Ctrl-W/U/K, grouped undo/redo and resize follow the shared
[keyboard contract](../features/interaction.md#keyboard-contract-and-terminal-quirks).
Paste is one transaction; adjacent typing groups for one second until whitespace
or navigation. Undo is bounded to 64 transactions. Host replacement resets undo;
Composer submission starts an independent editor and cannot be undone back into sent text.

The generic editor defaults to 4 KiB. Composer and Chat default to 64 KiB of
expanded UTF-8 payload, configurable with `Composer::new(limit)`. Ordinary text
insertion retains the complete-grapheme prefix that fits and reports truncation.
An oversized chip is refused as a whole. No-fit insertion preserves selection
and draft. Up to 256 inline payloads are supported in a draft. Rendering is safe
at zero area and does not move the cursor outside the supplied rectangle.

`set_reject_overflow(true)` makes typing, paste and yank reject an oversized
insertion whole, preserving text, selection, caret and undo. Question Form uses
this policy at 64 KiB; its host character limit remains validation only.

## Implementation design

### View contract

`EditorView` supplies title, placeholder, footer and presentation; changing props
does not change text. The full-width view uses a title rule, text rows and footer
rule without side borders. `chrome=false` renders text only for picker queries.
Hosts choose fixed, filling or growing height. The editor starts no terminal loop.
Shared styles and primitives belong to [Presentation](../presentation.md).

### Implementation details

The editor contract follows the supplied redesign: word wrapping, visual-row
Home/End and Ctrl-A/Ctrl-E, Ctrl-Shift-A select-all, previous-space Ctrl-W, logical
line Ctrl-U/Ctrl-K, and viewport PageUp/PageDown. Ctrl-G is a select-all fallback
because traditional terminals cannot distinguish Ctrl-Shift-A from Ctrl-A.
Chat transcript navigation uses Alt-PageUp/PageDown and Alt-End; host wiring
preserves editor bindings. No keyboard enhancement protocol is automatically
enabled; modified-key availability is checked through the human gate.

Ctrl-W removes trailing whitespace and the preceding non-whitespace span.
Alt-D kills through the next Unicode word end. Alt-Backspace or reported
Ctrl-Backspace kills a preceding
word part, recognizing separators, case transitions, acronym boundaries and
letter/digit transitions without splitting graphemes. Ctrl-U/Ctrl-K kill to the
logical line edge; pressing again at that edge removes the adjacent newline.
These kills populate an instance-local ring of the last ten complete payloads.
Consecutive kills in the same direction join in reading order until another
action, a selection kill or a one-second pause starts a new entry.
Ctrl-Y yanks the newest; Alt-Y replaces that yank with the next ring entry until
another edit or movement. Yank preserves inline chips and opaque attachment IDs,
is one undo transaction and retains the whole-grapheme prefix that fits the
expanded-content byte budget. It never cuts a chip payload. Dropped bytes are
reported; a no-fit yank preserves the draft and selection. Failed cycling
preserves the displayed yank and permits cycling onward.
Host reset clears the ring; it is not saved or shared between editor instances.
Ctrl-Z undoes; Ctrl-Shift-Z or Alt-Z redoes. Plain Enter is a host submission
action; Ctrl-J inserts a newline. Reported Shift/Alt-Enter may also insert one.

Input exceeding the byte budget retains its whole-grapheme prefix and reports
dropped bytes. No-fit insertion preserves draft/selection. Paste is one undo
transaction even when cut.
`insert_typed` accepts a host-reported text/IME commit with typing grouping;
`insert` accepts paste with one independent undo transaction. Neither submits.
Undo keeps 64 snapshots, including viewport offset; typing/deletion coalesces
until an injected one-second pause, movement, selection
replacement or paste. Whitespace ends typing groups. Host clock advancement
expires edit notices/paste tint after three seconds; the next key also clears
them. Limit state persists until an edit makes room; counter appears at 90%.

CRLF/CR normalizes to LF; tabs remain stored and expand at four-cell stops.
Other control characters remain stored and render as dim printable pictures or
hex labels. Rendering never emits their raw escape/control bytes. Submission
returns the original draft to the host, which owns execution and validation.
This differs from transcript sanitation, whose fragmented-stream contract is
still pending. Selection metadata counts graphemes and logical LF lines.

The view shows scroll counts on the rules and keeps one row of caret context
with at least four text rows. With three or fewer, it scrolls only at the edge.
Selection metadata includes selected graphemes and LF line count; narrow footers
omit caret position before clipping that count. Focus changes preserve state; inactive editors request no cursor
and omit their ordinary footer. Very short areas prioritize rules and hide text
and cursor if no text row remains. One/two-row chrome reports the hidden logical
line count on the title. Blurred editors retain scroll counts, selection counts
and near/full-limit bytes; the full limit uses error styling without bold.

### Presentation ownership

The editor separates [state](../../src/editor.rs), [transactions](../../src/editor/history.rs),
[input](../../src/editor/input.rs), [source-to-row layout](../../src/editor/layout.rs)
and [presentation](../../src/editor/view.rs). `EditorView` supplies title,
placeholder, footer and `Presentation`. Props never change the draft. `render`
remains a convenience wrapper; `render_with` accepts explicit props. Chat and
picker have equivalent profile-aware render entry points. A full-width editor
contains title rule, text rows and footer rule. Hosts can supply a three-row
single-text-row area, a fixed/fill area, or vary the area to implement growing
height; no host-height policy lives inside the editor.

## Host boundary

Hosts choose focus, area, height policy, limits and explicit elapsed time.
They decide how Enter is routed into submission and validate or execute returned
text. The editor performs no terminal acquisition, clipboard reads or transport.
[Composer](composer.md) owns sent messages, queue, recall, stash and handoff.

## Acceptance criteria

- **E-01:** Unicode movement, selection, replacement and resize retain valid boundaries.
- **E-02:** Paste and undo are atomic, including a paste cut by the byte limit.
- **E-03:** Limits preserve whole graphemes, report loss and keep tiny views bounded.

[Editor tests](../../tests/integration/editor.rs) and
[presentation tests](../../tests/integration/presentation.rs) cover the reusable
editor. Composer owns submission, recall and attachment lifecycle; see its
[contract](composer.md).

## Remaining gaps

Functional editor/focus acceptance is recorded; remaining redesign actions and
terminal modifier-key encodings need the [human gate](../human-testing.md#gate-1--editor-and-focus).
Native Linux/Windows and IME coverage are not established by macOS PTY checks.
