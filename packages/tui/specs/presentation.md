# Presentation architecture

Status: design-system/editor redesign requested on **2026-10-09**. Implemented
and awaiting the renewed human gate. Broader session/activity/diff mockups remain
in their owning delivery slices. Slice 2a applies the supplied option-picker
design, awaiting its own human gate. The implementation contains no copied
session/project data.

## Ownership and composition

This topic owns semantic tokens, drawing primitives and shared presentation
fallbacks. Component specs own exact geometry and interactions; Architecture owns
the current module map and System design owns dependency/public-contract rules.

Use frontend-style ownership with terminal-native units. Semantic tokens own
colors, symbols and cell dimensions. Primitives own reusable drawing contracts.
Component views combine those primitives with read-only component state and
explicit host props. Editing/history/input logic owns behavior independently of
styles. Hosts own area, focus, clocks, submission and product operations. Gallery
composition owns fixtures, scenario keys, stacking and the temporary modal.

The [theme](../src/theme.rs) resolves ANSI 16, indexed, RGB and no-color styles
from [tokens](../src/theme/tokens.rs). Components contain no palette literals.
The [rule](../src/ui.rs) draws one horizontal row with independently styled
left/right labels; metadata takes priority at narrow widths. It never draws
side borders. No runtime registry, browser CSS engine or shared product package
is introduced. Extend these tokens/primitives as later accepted components need
them, instead of implementing speculative screens.

Composer rules use separate semantic `line` and `line_info` colors. `line_info`
is RGB #495F81, indexed 60 or ANSI blue; no-color uses faint attributes. Recall
uses the Unicode dashed `┄` or ASCII `.` glyph, while drafts use solid `─` or `-`.
The rule occupies the same row and width in every box mode.

Component views use explicit props and profile-aware rendering. Their contracts
own geometry, height policy, focus and state behavior:
[Editor](components/editor.md#implementation-design),
[Option Picker](components/option-picker.md#implementation-design),
[Dialog](components/dialog.md#implementation-design),
[Command Palette](components/command-palette.md#implementation-design) and
[Composer](components/composer.md#implementation-design).

Accepted terminal limitation (2026-10-09): the user's iTerm2 profile renders
default-color bold text darker than ordinary text. A shell-only comparison
reproduced this independently of the library. The user accepts this appearance;
components retain default foreground and bold, with no terminal-setting change
or application workaround. Emitted label/title styles are covered by native PTY
tests in all four color modes.

Dialog bodies reuse shared rules, surfaces and focus styles. Their centered
geometry, title trail, background fade and prompt growth are owned by the
[Dialog contract](components/dialog.md#official-dialog-design). A body retains
its own component presentation instead of inheriting separate dialog editing rules.

## Tokens and fallback behavior

Body foreground/background remain terminal defaults. ANSI 16 secondary/live hint
text uses default foreground plus faint, rather than the profile's bright-black
slot. Obscured background may use that slot. Indexed/RGB modes resolve dim, accent, success, warning, error, info,
surface and selection. The supplied dark/light role values are centralized.
Editor selection uses an accent tint in extended modes and reverse otherwise.
Container headings are bold without reverse. In no-color only the focused field's
heading reverses, and obscured content receives no fade styling. Inactive chrome is
dim while body text remains readable. ASCII changes decorations/control display,
preserving document Unicode. Fonts and cursor shape/blink belong to the terminal.

In saved iTerm2 profiles inspected on 2026-10-10, “Use Bright Bold” is enabled:
ordinary cyan is green-cyan while bright cyan is grey-green. The terminal can
therefore display ANSI cyan plus bold as grey even when the app supplies the
correct slot and attribute. This matches iTerm2's
[bright-bold behavior](https://iterm2.com/documentation/2.1/documentation-preferences.html).
Strict ANSI 16 cannot guarantee bold cyan's RGB appearance independently of that
profile. Keep the requested cyan slot and bold attribute; do not modify terminal
settings or emit RGB behind a 16-color request. Native active-profile confirmation
remains separate from inspecting saved preferences.

Gallery resolves nonempty `NO_COLOR`, `COLORTERM` and `TERM` once; explicit CLI
options override it. Pure components perform no environment reads or terminal
queries. During its run the gallery enables backend color emission, so Crossterm
cannot independently suppress explicit profiles with `NO_COLOR`. Auto/no-color
presentation supplies no color tokens. The host restores the previous backend
policy after restoring the terminal, including on errors. OSC 11 background
probing is deferred; RGB polarity is explicit through
`--light` or Ctrl-T. Diff tint roles, runtime profile negotiation, pointer copy,
address routing and growing prompt composition remain later capabilities.

## Component contracts

The [Editor contract](components/editor.md) owns editing behavior and its view
requirements. Other component contracts are indexed under
[Components](components/README.md).

## Verification

### macOS Terminal color diagnosis — 2026-10-09

The failing host reports macOS 15.7.9 (`sw_vers`) and Terminal 2.14 (the system
app's `CFBundleShortVersionString`). The user observed gray chrome with explicit
RGB and cyan chrome with indexed/auto presentation. Apple's
[Tahoe feature list](https://www.apple.com/lae/os/pdf/All_New_Features_macOS_Tahoe_Sept_2025.pdf)
introduces Terminal's 24-bit color support. This host's Terminal supports the
indexed palette and ignores the requested RGB styling.

An owned PTY comparison used the debug gallery with `--component editor`,
`TERM=xterm-256color`, empty `COLORTERM` and absent `NO_COLOR`. Capture startup
output, strip cursor-positioning sequences for the startup marker, send Ctrl-C,
drain output and inspect SGR sequences. Auto and `--colors ansi256` both emit
`38;5;80;49` for the accent and `38;5;245;49` for dim chrome. Explicit truecolor
emits `38;2;86;194;201;49` and `38;2;133;135;140;49`. All three exit successfully.
The component and backend emit the requested colors; the emulator's RGB
capability explains the visible difference. An explicit profile is an override,
not a capability upgrade. The subsequent iTerm2 screenshot shows colored focused
chrome; RGB acceptance requires a supporting terminal. No OS-specific downgrade
or Terminal-version branch exists in the implementation.

The independently reproduced Crossterm `NO_COLOR` suppression is covered by the
PTY regression. Its correction preserves explicit overrides and auto/no-color
behavior; it does not explain the user's indexed-versus-RGB screenshots.

### Automated coverage

[Presentation tests](../tests/integration/presentation.rs) exercise terminal
defaults, no-color, tint/reverse, dark/light profiles, border-free text geometry,
host placeholders/footers, safe control display and notice expiration.
[Editor tests](../tests/integration/editor.rs) cover changed bindings, grouped
undo, paste cutting and no-fit selection preservation in addition to Unicode
properties and narrow rendering. The native macOS PTY gate exercises paste,
focus, modal input, resize and restored ordinary line input. The PTY also verifies
RGB accent output with an explicit profile under `NO_COLOR`. Tests do not accept
terminal emulator font/cursor behavior or establish native Linux/Windows coverage.

See [Human testing](human-testing.md) for the renewed gate, engineering evidence
and pending commit. Broad transcript, dialog and product composition redesigns
remain outside this slice.
