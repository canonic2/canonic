# Command Palette

## Purpose

Search a context-specific command map and derive searchable keyboard help from
the same descriptors and effective bindings.

## Current status

Status: palette, keyboard help and scoped bindings are implemented in slice 2c,
with their human acceptance gate pending. Hosts supply descriptors and execute
selected command intents.

## Behavior and state transitions

### Commands, completion and help

Slice 2c implements `commands::Commands` for palette and searchable keyboard help,
using one supplied descriptor map, command-specific columns and shared rules/tokens. Labels, categories,
descriptions, aliases, disabled reasons and effective binding text are projected
from that map. Selection returns a stable command ID; it never executes anything.
Availability must be rechecked by the host. Replace a context map atomically by
constructing a new validated view and dispatcher before swapping either instance.
Aliases are searchable here; Composer slash/file completion follows the
[Completion Menu contract](completion-menu.md).

Host supplies command ID, label/category, aliases/optional slash spelling,
description, availability/disabled reason and effective binding. Palette searches
and returns an invocation ID; it performs no dispatch, shell execution or parsing
of agent commands. Help derives from the same descriptors/bindings, with scope groups, source/conflict details
and delayed pending-sequence hints. Current context controls visibility, not a persistent
global registry. Availability is rechecked by host after selection.

The [Completion Menu contract](completion-menu.md) owns slash/file completion
acceptance and result generations. Palette and completion share host descriptors;
custom skills/agent commands remain host-provided rows.
Model, provider, agent, permission mode, variant and theme selectors reuse the
picker/browser; discovery, credentials and changing backend settings are host work.

### Search and focus

An exact effective key match ranks first. Next come label or key prefixes, label
word starts, matching aliases, then fuzzy label/description matches. Prefix key
matches sort by effective key; other ties use host-supplied recent IDs, then source
order. An alias match shows that spelling beside the command label. Disabled rows
remain searchable, show their reason in the right column in place of the key and
never receive selection. The query is bounded to 4 KB and paste is sanitized to
one line. Editing, search, help and inspection never invoke a command.

Arrows, Home/End and PageUp/PageDown select enabled rows. Enter on an unmodified
press emits one Selected ID; repeats/releases and modified Enter emit nothing.
Escape clears a nonempty filter first, then dismisses. F1 opens keys through the
host; Tab cycles supplied help scopes, including explicitly supplied empty scopes.
Help Enter never invokes. `o` emits an Inspect ID only when binding source metadata
exists. Hosts preserve the Composer draft, caret and transcript anchor while the
palette owns focus. Timed [Dialog](dialog.md) guards and closure apply to the host
overlay; the reusable Commands view neither owns terminal modes nor app quit.

### Rendering and recovery

Capture opening content height before filtering, including filtering received
before the first render. Keep that height and pad with blank rows when matches
shrink; clamp to the current host area on every render. Very short areas remove
group headings before removing command rows. The normal palette docks above the
Composer, centered horizontally in a 72-column panel; below 40 columns it uses the available
width. RGB/indexed profiles use a surface and faded backdrop. ANSI 16/NO_COLOR
use a box, and NO_COLOR preserves the backdrop. ASCII changes glyphs independently.
Keyboard help uses the supplied full-stage area.

Labels use normal foreground, details use muted foreground and effective keys
align right. Below 50 columns hide optional hints while retaining Escape and host
exit guidance. Empty source reads `no commands here`, with `this view has none ·
F1 lists every key`. No palette match reads `no command matches "<query>"`, followed
by `F1 lists every key`. Help uses `no key matches "<query>"`; a host may assert a
key is free in every scope only when it supplies complete scope evidence.
Custom bindings use the info role. Conflicts say `wins` or `overridden`; `ignored`
is reserved for rejected reserved bindings. Attribute-only warnings remain bold
with a visible `!`; selection uses reverse video.

After selection, the host rechecks availability. A refusal closes the palette and
shows its reason for three seconds without changing the draft or performing the
operation. Delayed leader hints remain separate from the palette: they appear
after 400 ms, expire after three seconds and cannot leak input into the draft.

## Implementation design

[commands.rs](../../src/commands.rs) owns validated descriptors, query, matches,
selection and recent commands. [The view](../../src/commands/view.rs) owns
terminal-cell columns, grouping, matched-grapheme emphasis and help details.
Palette height is retained while filtering and clamped to the host area; help
uses its supplied area. Shared rules and semantic tokens own chrome.

The gallery chooses docked Ctrl-P palette and full-stage F1 keys; those are
[gallery composition](../features/gallery.md#implemented-command-composition)
choices, rather than terminal ownership inside the palette.

## Host boundary

Hosts supply descriptors, scope, availability, remap metadata and the dispatcher.
They replace view/dispatcher maps atomically and recheck an invocation before
execution. Source inspection is a host intent. Palette rendering starts no work
and never parses or executes a product command.

## Acceptance criteria

- **D-03:** Palette/help/completion agree on enabled descriptors/bindings and emit
  exactly one intended action without execution or background leakage.

Gallery fixtures cover filtering by text/key, unavailable commands, no matches,
empty sources, custom help, binding conflicts and pending leader sequences.

- **CP-02:** Search classes and recent/source ties select the intended enabled ID;
  unavailable reasons replace keys and matched aliases remain visible.
- **CP-03:** Opening height survives typing/paste/clear before and after drawing,
  with short/narrow/ASCII/light/NO_COLOR behavior and safe empty-source feedback.
- **CP-04:** Conflict and reserved-key help preserve effective dispatch, show host
  source evidence and support explicitly empty scopes without invocation.

## Remaining gaps

Native visual acceptance remains pending in [Gate 2c](../human-testing.md#gate-2c--commands-and-bindings).
Source discovery and command execution belong to hosts. The standalone
[Completion Menu](completion-menu.md) owns insertion and source-result generations.
