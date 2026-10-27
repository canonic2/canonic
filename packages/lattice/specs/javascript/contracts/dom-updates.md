# DOM updates

## Purpose and status

This contract defines how an element turns a changed input into DOM changes:
which nodes it keeps, what it writes, how collections update, and what must be
preserved. The goal is that a small change does a small amount of work, both in
the DOM and in the JavaScript that decides what to write.

**Status:** agreed application authoring contract. The filters and tasks
examples use native DOM APIs; Lattice exports no rendering API. A keyed
collection helper is a candidate under the conditions in [decisions](#decisions).
[Element lifecycle](element-lifecycle.md)
owns construction and connection; [bindings](bindings.md) decides which input
receives a value.

## Static structure

- An element builds its shadow tree once, at construction, by creating nodes or
  cloning a `<template>` created on first use. It keeps references to every node
  it will update in private fields.
- Updates never query the tree to find a node. No `querySelector` runs in an
  update path.
- Parts of the tree that appear and disappear, such as an optional badge, are
  created on first need and kept; showing and hiding them toggles `hidden` or
  moves the existing node rather than recreating it.

## Inputs to nodes

- A setter compares the new value with the value the element holds, using
  `Object.is` or the comparison the input documents. When they are equal it
  returns without touching the DOM.
- Otherwise it stores the value and updates only the nodes that depend on that
  input. An element whose nodes depend on several inputs keeps one small update
  function per dependent part, called by the setters that affect it, not one
  function that rewrites the whole tree.
- The element keeps the values it renders. It never reads the DOM to recover a
  value only it can change, such as reading `textContent` or an attribute to
  compare.
- An update that computes the same output as the last one writes nothing. A
  part that derives text from two inputs compares the derived text with the
  last text it wrote.

### Inputs that drive live controls

Some DOM state changes without the element: a person types into a text field,
toggles a checkbox, opens a `<details>`, or a form reset restores defaults. For
an input that drives such live state, a cached copy of the last value the
element wrote can be stale.

- The element compares an input for live state with the control's current live
  value, such as `input.value`, `input.checked` or `details.open`, not with its
  cache. Reading these properties is a targeted read of the control's own state,
  not a layout read, and is allowed in update paths.
- An assignment whose value differs from the live value writes it, even when it
  equals the last value the application assigned. Receiving `"Acme"`, being
  edited to `"Other"` by the person, then receiving `"Acme"` again restores
  `"Acme"`.
- An assignment equal to the live value writes nothing. When the application
  echoes back what the person typed, the control is not touched.
- Scroll offsets are live state too, but reading them can require layout; they
  follow the [layout](#layout) rules instead.
- The element reports a person's edits as events, so the owning capability can
  keep its state in step. Whether an application value may replace a value the
  person is still editing is the capability's decision; the element applies
  what it is assigned.

## Writes

- Text uses `textContent` or a text node's `data`. Data is never written with
  `innerHTML`, `outerHTML` or `insertAdjacentHTML`.
- Attributes use `setAttribute`, `removeAttribute` and `toggleAttribute`;
  properties such as `disabled`, `checked` or `value` are set directly when the
  element owns that control.
- Discrete visual state, such as selected, open or busy, is expressed with
  attributes or classes that the element's stylesheet matches, not with inline
  style.
- A value that varies continuously, such as a size, position or transform, is
  written inline. The element writes the owned style property directly, such as
  `transform` on the node it moves, when one node uses the value. It writes a
  custom property when several rules or nodes in its shadow tree consume the same
  value. Inheritance and variable resolution have their own cost, so neither
  form is mandated; where it matters, the two are compared as
  [performance and evidence](performance.md) describes.
- Icons and other generated subtrees are rebuilt only when their own input
  changes, never as a side effect of another input.

## Keyed collections

These are requirements for fully mounted keyed presentation and full-list
structural updates, not a description of the pure `createCollection` API.
An ordinary item-field update assigns inputs to an existing row and need not
reconcile membership. A presentation list identifies each item by key and
incarnation:

- Each retained incarnation keeps one node and a binding lifetime. Structural
  updates reuse nodes for retained incarnations, create nodes for new ones and
  remove nodes for ended ones. Removing and reinserting a key replaces its
  incarnation and node even when the final key strings are unchanged.
- Removing a key ends that item's lifetime: its signal aborts and its bindings
  and resources are released.
- Reordering moves only nodes whose position changed, using `moveBefore` so that
  moved elements keep focus, selection, scroll and running state. Items that
  keep their order are not moved.
- Changing an item's data updates its existing node through its inputs; it does
  not recreate the node.
- Duplicate keys are an error reported to the caller; the collection is left
  unchanged: no node is created, moved, removed or updated and no item lifetime
  ends. To guarantee that, the update validates before it mutates. A first pass
  over the new list builds a map from key to position and stops at the first
  duplicate; only a list without duplicates reaches the second pass, which
  reconciles nodes using that map.
- A full-list structural update uses two passes over the new list and one key
  map of its length, plus the work for added, removed, moved and changed items.
  The map is the same one reconciliation needs to find existing nodes by key, so
  validation adds a pass, not a second structure. The update does not rebuild,
  serialize or deeply compare the list.
- Windowed retention, focus pinning and anchoring follow the separate
  [collection presentation pattern](../framework/collections.md). The
  full-list pass budget above is not a requirement to scan every key on each
  scroll or item update. There is no exported reconciler or windowing helper.

## Preserving interaction

Retaining a node is necessary for keeping a person's interaction state, but not
sufficient: some changes to a retained node alter that state by platform
definition.

- **Preserved:** focus, text selection, an input's value and caret, scroll
  offsets, an open `<details>` or `<dialog>` and a running animation survive:
  - updates that do not change that node's own value, visibility, disabled
    state, size or animation properties, such as an update to a sibling or to
    unrelated text;
  - moves with `moveBefore` of elements that keep their state when moved, as
    [element lifecycle](element-lifecycle.md#moves) describes.
- **Changed on purpose:** some updates change interaction state as the platform
  defines, and the element documents which of its inputs do so:
  - setting a text control's value moves or resets its selection and caret;
  - hiding, disabling or making a focused node inert loses its focus;
  - shrinking content can clamp a scroll offset;
  - changing animation properties can restart an animation.
- Updates never replace a node a person is using. When an update removes or
  hides the focused node, its owner documents where focus goes, such as the next
  item, the previous item or the collection itself, and moves it there.

## Layout

- Update paths separate geometry reads from DOM writes. Reading geometry such
  as `getBoundingClientRect` or `offsetWidth` after layout-affecting writes can
  force synchronous layout; repeated reads and writes can multiply it.
- Geometry an element needs comes preferably from `ResizeObserver` or
  `IntersectionObserver` callbacks owned by its connection. A direct read done
  before the element's own writes reduces the risk of a forced layout but does
  not exclude it, since another owner may have changed layout first. Frame-rate
  work and read/write ordering across owners belong to [scheduling](scheduling.md).

## Cost and evidence

Work is counted, not assumed. Three measures answer different questions and are
kept apart:

| Measure            | Answers                                      | How                                                                                                                                                                                                |
| ------------------ | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Write attempts     | Did the element try to write?                | Test-side counters on the writes an element performs, including property assignments such as `value`, `checked`, `disabled`, `scrollTop` and `style` properties, installed on the test's instances |
| Observed mutations | Which attributes, text and children changed? | A `MutationObserver` on the element's shadow root and host, recording attribute, character data and child-list mutations                                                                           |
| Browser work       | What did the browser recompute?              | Style recalculation and layout counts or a performance trace in a named environment                                                                                                                |

A `MutationObserver` does not see property assignments, so "no mutation records"
does not prove "no writes"; tests that assert no writes check write attempts
too. Other counts:

| Measure                 | How                                                                                     |
| ----------------------- | --------------------------------------------------------------------------------------- |
| Nodes created and moved | Counting created nodes per collection update, mutation records and node identity checks |
| JavaScript work         | Counters around update functions, as state management describes for selections          |

Expected counts for an update are part of an element's tests. Timing claims
name the environment and workload, as [performance and evidence](performance.md)
requires.

## Testing

Browser tests in Chromium:

| Case               | Assertion                                                                                                                                                                                           |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Equal input        | Setting an input to its current value produces no write attempts and no mutation records                                                                                                            |
| One input          | Changing one input writes and mutates only the nodes that depend on it                                                                                                                              |
| Edit then reset    | A text input receives `"Acme"`, the person types `"Other"`, the input receives `"Acme"` again: the control shows `"Acme"`                                                                           |
| Echo of an edit    | Assigning the value the person just typed produces no write attempt on the control                                                                                                                  |
| Native reset       | After a form reset restores a control's default, assigning the previous application value writes it again                                                                                           |
| Derived text       | Changing an input that leaves derived text the same produces no text mutation                                                                                                                       |
| No HTML writes     | Text containing markup renders as text                                                                                                                                                              |
| Collection reuse   | Updating data for existing keys creates no nodes; node identity is unchanged                                                                                                                        |
| Collection changes | Adding and removing keys creates and removes exactly those nodes and ends removed items' lifetimes                                                                                                  |
| Reorder            | A reordered collection keeps node identity, focus, an input's typed value and scroll position                                                                                                       |
| Unrelated update   | Updating a sibling or unrelated text keeps focus, selection and scroll of a retained control                                                                                                        |
| Duplicate keys     | A list whose duplicate key is near the end, after earlier items whose data and order changed, throws; no node is created, moved, removed or updated, no input is assigned and no item lifetime ends |
| Removed focus      | Removing the focused item moves focus to the documented target                                                                                                                                      |
| Layout             | An update performs no forced synchronous layout, observed with a performance trace in a named environment                                                                                           |

### Coverage and gaps

The table above states required behavior. The filters browser tests cover
selective input assignments, native control properties and ordinary teardown.
The task browser tests cover row identity, incarnation replacement, retained
focus/drafts, anchoring, keyboard destinations and duplicate membership
validation. Helper fixtures cover input upgrade behavior separately.

Dedicated cases remain for every control-specific reset/echo guarantee,
HTML-as-text behavior, the complete write-attempt matrix and forced-layout
measurement. A `MutationObserver` alone does not close those gaps. The
[large-collections research](../../../research/large-collections.md) records
rendering/presentation limits for its named baselines; it does not establish
that the current example meets all performance requirements.

## Decisions

- **Native DOM first.** Retained nodes and guarded setters keep updates small
  without a virtual DOM, templating compiler or diffing of whole trees, all of
  which add work to discover a change the element already knows about.
- **Keyed collection helper as a candidate.** Keyed reconciliation is the one
  update that is easy to get wrong by hand. A helper is added when two elements
  need it. It must reuse nodes by key, move with `moveBefore`, end removed
  items' lifetimes, validate before mutating, and cost two passes and one key
  map plus the changed items, with tests for each.
- **Live values over caches for live controls.** A cache cannot know about a
  person's edits or a native reset; comparing with the control's own value is
  one property read and keeps explicit resets working.
- **Preservation as a scoped guarantee.** Promising that every retained node
  keeps all its state would contradict the platform; naming the updates that
  preserve state, and documenting those that change it, keeps the guarantee
  testable.
- **No HTML strings for data.** Writing data as markup risks injection and
  replaces nodes, which loses interaction state.

## Open questions

- Whether the [retained-mechanics proposal](../framework/collection-mechanics.md)
  should become a public helper. Its callback and lifetime boundaries remain
  provisional; the reported comparison does not meet its adoption rule.
- Which presentation resources a future helper should support. Native custom
  elements are the agreed element authoring shape, but that does not require
  every retained list item to be a custom element. Plain nodes and other
  resources need their own workload and ownership evidence.
