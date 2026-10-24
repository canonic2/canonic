# Element helpers

**Status:** draft for review, implemented as described. This page explains how the
element helpers meet [element lifecycle](../contracts/element-lifecycle.md#helpers).
It adds no behavior; where the contract and this page differ, the contract wins
and this page is corrected. How authors use the helpers is in the
[elements guide](../framework/elements.md).

## Files

| File | Contents |
| --- | --- |
| `src/modules/elements/elements.js` | `captureInputs`, `replayInputs`, `skipUpgradeAttribute` and `defineOnce`, with their JSDoc types |
| `src/modules/elements/elements.browser.test.js` | Chromium tests, as [tests](#tests) lists |

The module is a DOM capability with its own entry point,
`canonic-lattice/elements`, in the `exports` map, as
[package](package.md#entry-points) describes. The root entry point does not
re-export it. Until the type-check configurations are split, the module is
checked by the single existing configuration.

## Dependencies

The module imports nothing. At run time it uses ECMAScript built-ins,
`Object.hasOwn`, `Reflect`, `Map` and `WeakMap`, and two methods of objects it
is given: the host's `hasAttribute` and the registry's `get` and `define`. It
never refers to `customElements`, `document` or any other DOM global; the
registry is always a parameter, and the default `customElements` belongs to
each element's own registration function.
Its JSDoc types name DOM types such as `HTMLElement` and
`CustomElementRegistry`, so it is checked with the DOM library, in the DOM
configuration [package](package.md#type-check-configurations) describes.

```text
src/modules/elements/  → ECMAScript built-ins only
src/modules/elements/  ↛ src/modules/store/
```

## `captureInputs(host, names, attributes)`

1. Starts with no map.
2. For each name in `names`, in order, checks `Object.hasOwn(host, name)`. A name
   the host does not own is skipped; no other property is examined.
3. For an owned name, reads the value with `Reflect.get` and deletes the own
   property with `Reflect.deleteProperty`. When the deletion returns `false`,
   as for a non-configurable property, it throws a `TypeError` naming the
   input: the property would keep hiding the accessor. Otherwise it creates the
   map on the first capture and stores the value under the name.
4. When `attributes` names an attribute for that input, calls
   `host.hasAttribute` for it and, when present, records the attribute against
   the input in a second, local map.
5. When any attribute was recorded, stores the second map in a module-level
   `WeakMap` keyed by the returned map, so `skipUpgradeAttribute` can find it
   and the record is collected with the element's pending map.
6. Returns the map, or `null` when nothing was captured, so an element with no
   early inputs allocates nothing, checks no attribute and queues no replay.

Deleting the own property uncovers the class's accessor, so later reads and
writes reach the element's getter and setter. The helper calls no setter and
writes no attribute, which keeps it safe in a constructor. Step 4 is the
contract's deliberate departure from the HTML Standard's requirement that
constructors not inspect attributes; it runs only for captured inputs.

## `skipUpgradeAttribute(pending, attribute)`

1. Returns `false` when `pending` is `null`.
2. Looks up the record `captureInputs` stored for `pending`. Returns `false`
   when there is none or it has no entry for `attribute`.
3. Deletes the entry, so it can answer `true` at most once, and returns whether
   the input is still in `pending`.

The upgrade enqueues an `attributeChangedCallback` for each attribute already
present before it runs the constructor, and runs them right after it, before
any other script. The first callback for a recorded attribute is therefore the
upgrade's own. Any later callback finds no entry and returns `false`, so it is
applied through the setter, which removes the input from `pending`.

## `replayInputs(host, pending)`

Iterates the map and assigns each value with `Reflect.set(host, name, value)`,
which runs the element's setter with the host as receiver. When `Reflect.set`
returns `false`, as for an input with no setter, it throws a `TypeError` naming
the input and applies no later entry.

The setter removes its own entry before comparing, so each replayed input is
removed as it is applied. An entry an earlier assignment already removed is not
visited: `Map` iteration skips entries deleted before they are reached,
including entries a setter removes for another input. The helper therefore
needs no state of its own to honor newer assignments; the map is the only
record of what is still pending.

The element, not the helper, decides when the replay runs. For inputs without
an attribute it calls `replayInputs` in the constructor, right after capture.
For inputs with an attribute it queues the microtask, keeps the map in a
private field and clears that field after the replay. That keeps the helper
free of scheduling and lets the getter read the same map.

## `defineOnce(registry, name, constructor)`

1. Reads `registry.get(name)`.
2. Returns when it is the same constructor.
3. Throws an `Error` naming the tag when it is a different constructor.
4. Otherwise calls `registry.define(name, constructor)`. Errors the registry
   raises, such as for an invalid name or a constructor already defined under
   another name, propagate unchanged.

## Typing

- `captureInputs` is generic over the host type `T`, which extends
  `HTMLElement`, and over the input names `K`, which extend `keyof T & string`
  and are inferred from `names`, so a name the class does not declare is a type
  error. `attributes` is a partial record from `K` to attribute names, so an
  entry for an input not in `names` is a type error too. It returns
  `Map<K, unknown> | null`. A separate `K` is needed because inside a class
  `this` is a deferred type, against which a record keyed by `keyof this`
  accepts no object literal.
- `replayInputs` accepts the same host type and map.
- `skipUpgradeAttribute` takes `Map<string, unknown> | null`, the type of an
  element's private pending field, and an attribute name.
- `defineOnce` takes a `CustomElementRegistry`, a string and a
  `CustomElementConstructor`.
- `Reflect.get`, `Reflect.set` and `Reflect.deleteProperty` avoid casting the
  host to an indexable type.

## Tests

Browser tests in Chromium, in `elements.browser.test.js`:

| Guarantee | Mechanism | Test |
| --- | --- | --- |
| Only declared names | Loop over `names` with `Object.hasOwn` | An own property not in `names` stays on the host |
| Own property removed | `Reflect.deleteProperty` | After capture, the host has no own property for the name and the getter is reached |
| No capture, no map | `null` result | An element upgraded without early properties gets `null` and queues no microtask |
| Replay through setters | `Reflect.set` | A spy on the setter sees each captured value once |
| Newer assignment wins | Map iteration skips removed entries | An assignment after upgrade and before the replay keeps its value |
| Superseded equal value | The setter removes before comparing | Assigning the rendered value before the replay writes nothing and the captured value is not applied |
| Attribute at upgrade | `skipUpgradeAttribute` answers `true` for the recorded attribute | A pre-upgrade property beats an attribute present at upgrade and is reflected |
| New attribute before replay | No record for an attribute absent at capture | An attribute first set after upgrade and before the replay wins over the captured property |
| Changed attribute before replay | The record is consumed by the upgrade's callback | An attribute present at upgrade and changed before the replay wins over the captured property |
| Presence checked only when captured | Presence checked inside the capture branch | Creating and parsing elements without early properties calls no `hasAttribute` |
| No record | `null` or unrecorded attribute | `skipUpgradeAttribute` returns `false` |
| Captured default | The setter guards nodes and attribute separately | A pre-upgrade empty label over a `label="Attr"` attribute ends with the label empty and the attribute replaced |
| Failed deletion | `Reflect.deleteProperty` result checked | A non-configurable early property makes `captureInputs` throw a `TypeError` naming the input |
| Failed assignment | `Reflect.set` result checked | An input with no setter makes `replayInputs` throw a `TypeError` naming the input |
| Constructor application | `replayInputs` called in the constructor | An input without an attribute is rendered and returned as soon as upgrade returns, with no attribute written and no event dispatched |
| Mixed inputs | One capture per kind of input | A constructor-applied and a replayed input both end rendered, with derived output and the attribute correct |
| Parser and clone | No early inputs to capture | Parsed and cloned elements render their attributes and defaults |
| Same constructor | `registry.get` comparison | `defineOnce` twice with one constructor defines once and does not throw |
| Conflicting constructor | Error on a different constructor | `defineOnce` with another constructor under a defined name throws and leaves the first defined |

The contract's other element cases use test elements written in the
[elements guide](../framework/elements.md)'s shape, in the same file.

## Open questions

- Whether the conflict error is a plain `Error` or a `TypeError`, given that a
  different constructor under the same name is a caller mistake rather than an
  invalid argument type.
