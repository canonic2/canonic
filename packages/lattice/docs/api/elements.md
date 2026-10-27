# Element helpers

Import from `@canonic2/lattice/elements`. These helpers do not register
components or access DOM globals during import.

Start with [Components](../guide/components.md) for a complete basic element.

## `captureInputs(host, names, attributes?)`

Captures declared own properties assigned before custom-element upgrade.
For each captured property it reads the value and deletes the own property,
uncovering the class accessor.

Returns a `Map` from input names to captured values, or `null` when none exist.
A non-configurable property that cannot be deleted throws a `TypeError`.

`attributes` maps reflected input names to attribute names. For captured inputs
only, the helper records whether the corresponding attribute is present.
It calls no setters, reads no attribute values, writes no attributes and
schedules no replay.

## `replayInputs(host, pending)`

Assigns each remaining map entry through the host's setter, in map order.
A setter removes its pending entry before comparing values, allowing a newer
assignment to supersede the captured one.

An assignment that returns false from `Reflect.set` throws a `TypeError`;
a setter's own error propagates. Later entries are not applied after a failure.

Constructor-safe setters can replay immediately. Setters that reflect host
attributes or place supplied children replay after construction.

## `skipUpgradeAttribute(pending, attribute)`

Returns `true` once for an upgrade's initial callback when that attribute was
recorded as present and its input is still pending. Returns `false` otherwise,
including when `pending` is `null`.

Use it in `attributeChangedCallback` so an early property wins over an
attribute present at upgrade. Later property and attribute assignments
supersede the captured value.

## Reflected inputs

The runnable tutorial's [TaskLabel](../../examples/tutorial/task-label.js)
implements reflected text; its [browser cases](../../examples/tutorial/components.browser.test.js)
exercise the upgrade precedence below. Its constructor retains the pending
map; the getter sees pending input immediately, and the setter removes that
entry before updating:

```js
/** @file Tutorial reflected text input, including pre-upgrade assignment precedence. */
import {
  captureInputs,
  replayInputs,
  skipUpgradeAttribute,
  defineOnce,
} from '@canonic2/lattice/elements';

/** A reflected text element whose pending input can be read before replay. */
export class TaskLabel extends HTMLElement {
  static observedAttributes = ['text'];
  #text = '';
  #node = document.createTextNode('');
  /** @type {Map<'text', unknown> | null} */
  #pending = null;

  /** Builds the shadow text and defers any input that would reflect attributes. */
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).append(this.#node);
    const pending = captureInputs(this, ['text'], { text: 'text' });
    if (pending !== null) {
      this.#pending = pending;
      queueMicrotask(() => {
        replayInputs(this, pending);
        this.#pending = null;
      });
    }
  }

  /** @returns {string} The current text, including captured input awaiting replay. */
  get text() {
    return this.#pending?.has('text') ? String(this.#pending.get('text')) : this.#text;
  }

  /** @param {string} value The text to render and reflect, superseding pending input. */
  set text(value) {
    this.#pending?.delete('text');
    if (value !== this.#text) {
      this.#text = value;
      this.#node.data = value;
    }
    if (this.getAttribute('text') !== value) this.setAttribute('text', value);
  }

  /**
   * Applies attributes, skipping the upgrade callback superseded by early input.
   * @param {string} name The changed attribute.
   * @param {string | null} oldValue The preceding attribute value.
   * @param {string | null} value The next value, or null for removal.
   * @returns {void}
   */
  attributeChangedCallback(name, oldValue, value) {
    if (!skipUpgradeAttribute(this.#pending, name)) this.text = value ?? '';
  }
}

/**
 * Registers the tutorial label without import-time registration.
 * @param {CustomElementRegistry} [registry] The destination registry.
 * @returns {void}
 */
export function defineTaskLabel(registry = customElements) {
  defineOnce(registry, 'acme-task-label', TaskLabel);
}
```

The pending map supports this precedence:

1. A property assigned before upgrade wins over an attribute already present.
2. An assignment after upgrade supersedes the captured value, even before replay.
3. Assigning the rendered default also supersedes pending input.

Node updates and reflection have separate guards. A captured default can leave
the text node unchanged while still correcting a different host attribute.

Recognizing the initial attribute callback requires inspecting attribute
presence for captured inputs during construction. Lattice deliberately departs
from the standard constructor guidance in this narrow case. Inputs without
reflection and constructor-safe setters need no attribute check or deferred
replay.

## `defineOnce(registry, name, constructor)`

Defines the constructor when the name is unused. Repeating the same constructor
under that name does nothing. A different constructor already registered there
throws an `Error`.

Native registry errors, including invalid tag names, propagate unchanged.
The helper chooses no global registry or tag name for its caller.

**Guide:** [Components](../guide/components.md) · [Lifetimes](../guide/lifecycle.md)
