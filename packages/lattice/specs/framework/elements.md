# Elements

**Status:** agreed authoring shape. The helpers this page imports are
implemented in `src/modules/elements/` and exported as
`@canonic2/lattice/elements`; both patterns below, constructor application and
replay in a microtask, are the ones their browser tests exercise. The guarantees behind each step are owned by
[element lifecycle](../contracts/element-lifecycle.md) and
[DOM updates](../contracts/dom-updates.md); this page shows how an author meets
them.

An element is a native class that extends `HTMLElement`. Its author writes the
class, its inputs and its callbacks by hand. Lattice supplies four narrow
helpers, `captureInputs`, `replayInputs`, `skipUpgradeAttribute` and
`defineOnce`, and nothing that renders, schedules or generates the class. The shape was chosen by measurement;
see the [component authoring record](../../research/component-authoring.md).

## A complete element

```js
import { captureInputs, defineOnce, replayInputs } from '@canonic2/lattice/elements';

/** A counter that patches one retained text node. */
export class AcmeCounter extends HTMLElement {
  #text = document.createTextNode('0');
  #count = 0;

  /** Builds the view once and applies a count assigned before upgrade. */
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).append(this.#text);
    const early = captureInputs(this, ['count']);
    if (early !== null) replayInputs(this, early);
  }

  /** @returns {number} The current count. */
  get count() {
    return this.#count;
  }

  /** @param {number} value The count to display. */
  set count(value) {
    if (Object.is(value, this.#count)) return;
    this.#count = value;
    this.#text.data = String(value);
  }
}

/**
 * Defines `<acme-counter>` in a registry.
 * @param {CustomElementRegistry} [registry] The registry to define it in.
 * @returns {void}
 */
export function defineAcmeCounter(registry = customElements) {
  defineOnce(registry, 'acme-counter', AcmeCounter);
}
```

## The parts

### Building the view

The constructor attaches an open shadow root and builds the element's nodes
once, keeping a reference to every node an input updates. Initial nodes already
show the inputs' defaults, so construction writes nothing extra: the counter's
text node starts as `'0'` because `#count` starts at `0`. A stylesheet shared by
all instances is created on first construction, not when the module loads.
Listeners on the element's own nodes are installed here once.

### Inputs

Each input is a getter and setter pair over a private field. The field holds the
value the element has rendered. The setter returns when the value equals the
rendered one; otherwise it stores the value and writes only the nodes that
depend on it.

A control a person can edit, such as a text field, is compared with its live
value instead of the field, as [DOM updates](../contracts/dom-updates.md)
requires.

### Inputs set before upgrade

A value assigned before the class is defined sits on the instance and hides the
setter. The constructor hands the declared input names to `captureInputs`, which
moves those values into a map, or returns `null` when there are none.

For inputs without an attribute, like the counter's `count`, the constructor
passes the map straight to `replayInputs`. That is allowed because their setters
write only the element's own nodes and fields: no host attribute, no event and
nothing acquired.

### Attributes

An input with an attribute lists it in `observedAttributes`, and its setter
reflects the value to the attribute. The constructor may not write attributes,
and attributes present at upgrade arrive after it, so a captured value for such
an input is replayed in a microtask instead:

- the constructor keeps the map in a private field and queues one microtask that
  calls `replayInputs` and then clears the field;
- the getter returns a value still in the map, so code reading the element right
  after upgrade sees what it assigned;
- the setter first removes its input from the map, so a newer assignment wins;
- `captureInputs` receives each input's attribute name and records whether that
  attribute is present for inputs it captured;
- `attributeChangedCallback` asks `skipUpgradeAttribute` whether this is the
  upgrade's own callback for such an attribute. Only that one callback is
  skipped, so the captured property wins over an attribute present at upgrade,
  while an attribute set or changed afterwards wins over the property.

Recording presence means the constructor inspects the host's attributes, which
the HTML Standard says a constructor must not do. Lattice departs from that
requirement on purpose, only in this upgrade case and only for presence;
[element lifecycle](../contracts/element-lifecycle.md#inputs-set-before-upgrade)
explains why.

An element with both kinds of input calls `captureInputs` once for each kind.

```js
static observedAttributes = ['label'];

constructor() {
  super();
  this.attachShadow({ mode: 'open' }).append(this.#labelText);
  const pending = captureInputs(this, ['label'], { label: 'label' });
  if (pending !== null) {
    this.#pending = pending;
    queueMicrotask(() => {
      replayInputs(this, pending);
      this.#pending = null;
    });
  }
}

/** @returns {string} The label, including one not yet replayed. */
get label() {
  return this.#pending?.has('label') ? String(this.#pending.get('label')) : this.#label;
}

/** @param {string} value */
set label(value) {
  this.#pending?.delete('label');
  if (!Object.is(value, this.#label)) {
    this.#label = value;
    this.#labelText.data = value;
  }
  if (this.getAttribute('label') !== value) this.setAttribute('label', value);
}

/** @param {string} name @param {string | null} oldValue @param {string | null} newValue */
attributeChangedCallback(name, oldValue, newValue) {
  // The upgrade's own callback loses to a property captured before it.
  if (skipUpgradeAttribute(this.#pending, name)) return;
  this.label = newValue ?? '';
}
```

The setter guards its nodes and its attribute separately. A captured value
equal to the rendered one, such as an empty label over an attribute present at
upgrade, writes no node but still corrects the attribute. Reflecting calls
`attributeChangedCallback` again with the same value, which then changes
nothing.

### Connection resources

An element that acquires anything outside its own nodes, such as an observer or
a document listener, creates one `AbortController` per connection in
`connectedCallback`, after checking `isConnected`, and aborts it in
`disconnectedCallback`. An element with none, like the counter, defines neither
callback. An element that should keep its connection when moved defines
`connectedMoveCallback`.

### Events

User intent leaves the element as a bubbling, composed `CustomEvent` from the
host. A handler installed in the constructor checks `isConnected` before
dispatching. An element never imports a store or calls application code.

### Registration

The module exports the class and a `defineX(registry = customElements)`
function that calls `defineOnce`. Importing the module defines nothing.
