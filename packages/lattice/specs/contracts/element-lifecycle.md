# Element lifecycle

## Purpose and status

This contract defines how a Web Component written for Lattice is constructed,
receives inputs, connects, disconnects, reconnects, owns resources and is
registered. It covers autonomous custom elements with open Shadow DOM. How an
element turns an input into DOM changes is [DOM updates](dom-updates.md); how
application state reaches its inputs is [bindings](bindings.md).

**Status:** agreed contract. The [helpers](#helpers) are implemented; the
remaining cases in [testing](#testing) are written as elements need them.
Elements are native classes
written by their authors, using the platform's custom element callbacks
directly. Lattice provides narrow helpers for the parts that are repeated and
easy to get wrong, listed in [helpers](#helpers), and no base class or
generated class; [decisions](#decisions) records why. How an author writes an
element is described in the [elements guide](../framework/elements.md).
[Resource lifecycle](resources.md) owns shared cleanup guarantees;
[delivery](delivery.md) owns side-effect-free imports.

## Phases

| Phase | Callback | The element |
| --- | --- | --- |
| Construction | `constructor` | Builds its shadow tree once, captures inputs set before upgrade and applies those without an attribute, installs listeners on nodes it owns |
| Upgrade completion | Initial `attributeChangedCallback`s, then a replay microtask | Applies attributes present at upgrade, except for inputs captured as properties, then replays captured properties of inputs with an attribute |
| Connection | `connectedCallback` | If still connected and it owns connection resources, creates a connection lifetime and acquires them |
| Move | `connectedMoveCallback` | Keeps its connection when moved atomically, if it defines this callback |
| Disconnection | `disconnectedCallback` | Ends the connection lifetime; everything acquired for it is released |
| Reconnection | `connectedCallback` again | Starts a fresh connection lifetime; inputs and owned nodes are unchanged |

## Construction

- The constructor attaches an open shadow root, adopts the element's
  stylesheet and builds the nodes it owns, keeping references to the nodes it
  will update. The shadow tree is built once and never rebuilt for an input.
- A stylesheet shared by every instance of a class is created on the first
  construction, not when the module is evaluated, so importing the module
  touches no DOM API.
- The constructor does not read or write the host's attributes or children,
  does not dispatch events and acquires nothing outside the element, as the
  custom elements specification requires of upgrades. There is one deliberate
  exception, described in
  [inputs set before upgrade](#inputs-set-before-upgrade): for an input with an
  attribute that was captured as a property, the constructor checks whether
  that attribute is present.
- The constructor initializes inputs to their defaults through a
  constructor-safe path: it sets the element's private values and updates its
  own nodes, and never writes host attributes. Reflection to host attributes
  happens only outside the constructor.
- Listeners on nodes inside the element's own shadow tree may be installed once
  in the constructor. They live and die with the element and cost nothing on
  reconnection. Because they survive disconnection, a handler that dispatches a
  user-intent event first checks `this.isConnected` and dispatches nothing when
  the element is disconnected.

### Inputs set before upgrade

A property can be assigned to an element before its class is defined. That
value sits on the instance and would hide the class's setter. The platform then
upgrades the element in this order: it runs the constructor, then delivers an
`attributeChangedCallback` for each observed attribute already present, then,
if connected, `connectedCallback`. How the element handles an early property
depends on whether the input has an attribute.

**Inputs without an attribute** are captured and applied in the constructor:
the element captures them and assigns each captured value through its setter
immediately. This is allowed only when the input's setter, in every branch,
writes nothing but the element's own nodes and private fields: no host
attribute, no event, no read of host attributes or children, and no resource
acquisition. Such an input has no pending window, so its getter and setter need
no pending checks. An input whose setter cannot meet these conditions is
handled like an input with an attribute.

**Inputs with an attribute** are handled in two steps:

1. **Capture, in the constructor.** For each declared input that exists as an
   own property, the element records the value in a private pending map and
   deletes the own property. It calls no setter and writes no host attribute.
   Only declared inputs are checked; it does not scan its properties. For each
   input it captured, it also records whether that input's attribute is
   present, so that the upgrade's initial callback for it can be recognized.
   When nothing is captured, nothing else happens and no attribute is checked.
2. **Replay, after the initial attribute callbacks.** When something was
   captured, the constructor queues one microtask. It runs after the upgrade's
   initial attribute callbacks, and assigns each pending value through its
   setter, which may then reflect it to a host attribute.

The microtask exists because the constructor may not write host attributes and
because attributes present at upgrade are delivered after the constructor;
neither concern applies to an input without an attribute.

**Departure from the HTML Standard.** The HTML Standard requires that a custom
element constructor not inspect the element's attributes. Checking presence
for a captured input departs from that requirement on purpose. An initial
upgrade callback and a first `setAttribute` made after the upgrade both arrive
with a `null` old value, and the attribute is present in both cases, so
nothing available in the callback tells them apart. Only presence at
construction does. The departure is limited:

- it happens only during an upgrade in which a property was assigned before
  definition, since otherwise nothing is captured;
- it covers only captured inputs that have an attribute;
- it checks presence and never reads the attribute's value;
- nothing the element renders depends on it; it only decides which single
  callback to skip.

Elements created with `document.createElement`, by the parser or by cloning
capture nothing and check no attribute.

Precedence and consistency, for inputs with an attribute:

- A property assigned before upgrade wins over an attribute present at upgrade
  for the same input. The element skips exactly one `attributeChangedCallback`
  for that input: the upgrade's initial one, for an attribute recorded as
  present at capture. The replay then applies the property, and the reflected
  attribute then matches it.
- Every other assignment to an input, by property or attribute, while its
  replay is still pending is newer than the captured value, including an
  attribute first added after the upgrade: it removes that input from the
  pending map, then applies like any assignment. Removing the entry is enough to
  supersede the captured value, so an assignment equal to the value already
  rendered writes no node.
- A setter guards its node writes and its attribute reflection separately: the
  nodes by comparing with the rendered value, the attribute by comparing with
  the attribute's current value. An attribute present at upgrade is not
  applied while a captured property is pending, so a captured value equal to
  the rendered default must still replace that attribute.
- While a value is pending, the input's getter returns it, so code that reads the
  element right after upgrade sees the value it assigned. The element's own
  field keeps the value it has rendered; a pending value lives only in the
  pending map.

## Inputs

- Inputs are the element's documented properties and attributes. Properties
  carry structured values; attributes carry simple declarative values. The
  element documents which attributes reflect which properties, and boolean
  attributes follow HTML presence semantics.
- Setters work whether or not the element is connected. They update only the
  element's own nodes, following [DOM updates](dom-updates.md), and acquire no
  resources.
- Setting an input never dispatches an event. Events describe user intent, not
  input changes.
- An element does not mutate a value it receives. It keeps the reference, so an
  identity comparison in its setter can skip unchanged values, and expects a
  changed value to arrive as a new object rather than a mutation of the old one.
  It copies a value only when it must isolate itself from the caller, and
  documents that input as copied.

## Connection

- `connectedCallback` first checks `this.isConnected` and returns without
  acquiring anything when it is false. Custom element callbacks are queued, so a
  connection callback can run after another callback, such as an ancestor's, has
  already removed the element again.
- An element that acquires connection resources creates one `AbortController`
  per connection. Everything it acquires outside its own nodes uses that signal
  or is released when it aborts: listeners on the document, the window or other
  elements, observers, timers, animation frames, media queries and
  subscriptions.
- An element with no connection resources, such as one whose only listeners are
  on its own nodes, creates no controller and needs no connection or
  disconnection work.
- Listeners on the host element itself that respond to the outside, such as
  keyboard handling on the host, are connection resources, so a disconnected
  element reacts to nothing.
- If acquiring a resource throws, the element aborts its connection controller
  before the error propagates, releasing everything acquired so far.
- Connecting while already connected cannot occur without a disconnection in
  between; an element that receives `connectedCallback` with a live controller
  aborts the old one first, so it never holds two.

## Disconnection and reconnection

- `disconnectedCallback` aborts the connection controller, if there is one.
  Aborting is idempotent; a second disconnection does nothing.
- Asynchronous work started during a connection, such as a frame callback, a
  transition end or a focus restoration, captures that connection's signal when
  it starts, and checks the captured signal before it touches the DOM. It never
  checks the element's current controller: after a disconnection and
  reconnection, that controller belongs to the new connection and would let
  stale work through.
- Reconnection runs `connectedCallback` again and creates a new controller. It
  reacquires connection resources and creates no duplicates. Inputs, owned nodes
  and interaction-local state that should survive, such as text being typed,
  are unchanged.
- A disconnected element keeps its inputs. It is not disposed: it may be
  reconnected, and it is garbage collected when nothing references it.

## Moves

Moving a connected element with `Element.moveBefore` keeps it connected when
the element defines `connectedMoveCallback`; without that callback the platform
disconnects and reconnects it. An element whose connection resources or
interaction state should survive reordering, such as an item in a keyed
collection, defines `connectedMoveCallback`, which does nothing or only updates
what depends on position. Collections that move items use `moveBefore`, as
[DOM updates](dom-updates.md) specifies.

## Events

- Events are dispatched from the host as `CustomEvent`s with a documented name
  and `detail`. Events meant for an ancestor's binding bubble and are composed.
- An element dispatches events only for user intent it observed while
  connected. Handlers installed at construction check `this.isConnected` before
  dispatching, since they outlive disconnection.
- An element never calls application code directly; it has no callbacks into a
  capability and imports no store.

## Registration

- Importing an element's module never defines it. Registration is an explicit
  call, made by the consuming entry point:

  ```js
  /**
   * Defines `<acme-toggle>` in a registry.
   * @param {CustomElementRegistry} [registry] The registry to define it in.
   * @returns {void}
   */
  export function defineAcmeToggle(registry = customElements) {
    defineOnce(registry, 'acme-toggle', AcmeToggle);
  }
  ```

- Registering the same class twice does nothing. A different class already
  registered under the same name is an error; registration never silently keeps
  or replaces an incompatible definition.
- The tag name is the registration function's concern. The class itself works
  under any name a registry gives it.

## Helpers

Lattice provides four functions for elements, from a DOM subpath of their own.
Each does one step of this contract; the element keeps its state, setters,
callbacks and connection controller.

| Helper | Does | Does not |
| --- | --- | --- |
| `captureInputs(host, names, attributes)` | For each declared name that exists as an own property of the host, records its value in a new map and deletes the property. For a captured name listed in `attributes`, records whether that attribute is present. Returns the map, or `null` when nothing was captured. Throws a `TypeError` naming the input when the property cannot be deleted | Scan other properties, check attributes of inputs it did not capture, read attribute values, call setters, write attributes, queue anything, or report a capture that left the property in place |
| `skipUpgradeAttribute(pending, attribute)` | Returns `true` once, for the upgrade's initial callback of an attribute recorded as present at capture while its input is still pending; `false` for every other callback and when `pending` is `null` | Decide anything from the old or new value |
| `replayInputs(host, pending)` | Assigns each entry still in the map through the host's setter, in the map's order. Throws a `TypeError` naming the input when an assignment is refused, applying no later entry | Apply values directly, skip setters, schedule itself or ignore a refused assignment; the element queues the microtask that calls it |
| `defineOnce(registry, name, constructor)` | Defines the constructor under the name; does nothing when that constructor is already defined there; throws when another constructor is | Choose a tag name or define anything on import |

For inputs without an attribute, the constructor calls `replayInputs`
immediately with the map `captureInputs` returned. For inputs with an
attribute, the element holds the map in a private field until the replay has
run; their getters read a pending value from the map, their setters remove
their input from the map before comparing, and `attributeChangedCallback`
returns early when `skipUpgradeAttribute` answers `true`, as
[inputs set before upgrade](#inputs-set-before-upgrade) requires. An element
with both kinds calls `captureInputs` once for each kind.

The helpers add no render pass, no property scanning beyond the declared names
and no scheduling. A further helper, or a change to these, is a change to this
contract, made with the evidence [performance](performance.md) requires.

## Testing

Browser tests in Chromium:

| Case | Assertion |
| --- | --- |
| Import | Importing the module defines no element and touches no DOM API |
| Registration | Defining twice with the same class succeeds; a conflicting class throws |
| Early inputs | A property assigned before definition is rendered after upgrade and read back through the getter, including right after upgrade before the replay runs |
| Early input without an attribute | A property assigned before definition is rendered and returned by the getter as soon as upgrade returns, before any microtask, with no host attribute written and no event dispatched |
| Mixed inputs | An element with an input applied in the constructor and a replayed input with an attribute, both assigned before upgrade, ends with both rendered, the attribute matching and derived output correct |
| Parser and clone | Elements created by parsing and by `cloneNode` after definition render their inputs and attributes the same way |
| Early input and attribute | An element with an attribute and a pre-upgrade property for the same reflected input ends with the property's value, the attribute reflecting it, and no host attribute written during construction |
| Assignment during replay | An input assigned after upgrade but before the replay runs keeps the newer value |
| Superseded equal value | An input assigned, before the replay runs, the value it already renders is not written again, and the replay does not apply the captured value |
| Captured default over an attribute | A pre-upgrade property equal to the default, with a different attribute present at upgrade, ends with the default rendered and the attribute replaced by it |
| New attribute before replay | An attribute first added after upgrade and before the replay wins over a property captured before upgrade |
| Changed attribute before replay | An attribute present at upgrade and changed after it, before the replay, wins over the captured property |
| Presence checked only when captured | Upgrading or creating an element with no early properties checks no attribute |
| Failed capture or replay | `captureInputs` throws for an early property that cannot be deleted, and `replayInputs` throws for an assignment the element refuses, each naming the input |
| Capture helper | `captureInputs` captures only declared names that are own properties, deletes them, and returns `null` when there are none |
| Replay helper | `replayInputs` assigns through setters and skips entries removed from the map before it runs |
| Registration helper | `defineOnce` succeeds for the same constructor twice and throws for a different one |
| No capture | An element upgraded with no early properties queues no replay |
| Inputs while disconnected | Setting inputs on a disconnected element updates its nodes and dispatches nothing |
| Connection resources | A document listener, observer and timer acquired on connection are released on disconnection; counts return to zero |
| Removed before connection runs | An element removed by an ancestor's `connectedCallback` before its own runs acquires nothing |
| No resources | An element with only internal listeners creates no controller on connection |
| Disconnected intent | Activating an internal control of a disconnected element dispatches no event |
| Reconnection | Disconnecting and reconnecting several times leaves exactly one of each connection resource |
| Partial failure | A throw during connection leaves no acquired resource behind |
| Late work | A frame or transition callback scheduled before disconnection makes no DOM change after it, including when the element has been reconnected before the callback runs |
| Retained inputs | An object input is kept by reference; setting the same object again writes nothing |
| Moves | An element with `connectedMoveCallback` moved with `moveBefore` keeps its connection resources and focus |

## Decisions

- **Native classes with narrow helpers** (2026-10-24). A lab compared a
  hand-written class, a class with these helpers, a setup function returning
  one closure per input and a setup function with shared update functions,
  each behind one internal class Lattice generated per definition. See the
  [component authoring record](../../research/component-authoring.md). All four
  made identical DOM writes and rendering work, and their interaction,
  responsiveness and lifecycle timings were within noise of each other at the
  sizes measured. The class with helpers matched the hand-written class on the
  confirmed interaction workloads, memory and cold start, and used less memory
  per added element than either setup function: 310 bytes against 580 for
  closures and 336 for shared update functions. The shapes
  were not shown to differ in speed, and classes are not claimed to be faster
  in general; confirmation at large sizes was inconclusive. A base class was
  not measured; adding one, or any generated class, is a contract change that
  needs its own comparison.
- **Capture in the constructor, replay in a microtask, for inputs with an
  attribute.** The constructor may not write host attributes, and the platform
  delivers attributes present at upgrade after the constructor. Replaying
  captured properties in one microtask after those callbacks keeps the
  constructor safe and makes the property win. The microtask is queued only
  when something was captured, so an ordinary element pays nothing. This is a
  one-time upgrade step, not a deferral of input or state updates.
- **Constructor application for inputs without an attribute** (2026-10-24).
  Neither reason for the microtask applies to an input without an attribute
  whose setter writes only the element's own nodes and fields, so such inputs
  are applied in the constructor, removing the pending window and the checks it
  needs. Adopted on correctness evidence: browser tests of early application,
  constructor safety, mixed inputs, elements with no early inputs, and parsed
  and cloned elements. No performance lab was run; the change removes work
  rather than trading one cost for another.
- **Checking attribute presence for captured inputs** (2026-10-24). Without
  it, an attribute first set between the upgrade and the replay was mistaken
  for the upgrade's own callback and lost to an older captured property. No
  information in the callback distinguishes the two. Checking presence in the
  constructor departs from the HTML Standard's requirement that constructors
  not inspect attributes; it was accepted as a narrow, documented exception
  because the alternative was to let a newer assignment be overwritten.
- **Owned-node listeners at construction, outside resources per connection.**
  Installing internal listeners once avoids work on every reconnection; tying
  everything external to the connection signal makes cleanup a single abort.
- **Explicit registration.** Defining elements on import would break
  side-effect-free imports and tree shaking, and would choose tag names for the
  consumer.

## Open questions

- Whether elements should support scoped custom element registries once they
  are available in the target runtimes, so two applications can register
  different versions in one document.
