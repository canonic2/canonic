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

## Remaining gaps

Native visual acceptance remains pending in [Gate 2c](../human-testing.md#gate-2c--commands-and-bindings).
Source discovery and command execution belong to hosts. The standalone
[Completion Menu](completion-menu.md) owns insertion and source-result generations.
