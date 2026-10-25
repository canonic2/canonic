# Retained collection mechanics proposal

**Status:** proposed for review, not exported or adopted. The runnable
[task example](../../examples/tasks/README.md) is the hand-written baseline.
The [research record](../../research/large-collections.md) owns the comparison
and measurement limits. This proposal adds no state-model-specific API.

## Responsibility

One coordinator owns retained keyed items and their `AbortController`s. It
accepts read-only membership and invokes explicit application callbacks.
It has no DOM, store or collection dependency. An item may be an element or
another presentation resource. Full-list reconciliation and window retention
use the same mechanism; windowing is optional.

The application still supplies capability actions, selections, bindings,
markup, styling, viewport measurement, scroll anchoring, keyboard behavior
and deterministic focus fallback. It passes the focused key for pinning.
The coordinator never chooses which item is selected, queries the DOM, writes
CSS or subscribes to state. Recycling is excluded.

## Candidate surface

Provisional names, with no package import or public entry point selected:

```js
const retained = createRetainedItems({
  create(key, signal) { /* construct presentation and bind its inputs */ },
  place(key, item, index, previous) { /* native placement and geometry */ },
  remove(key, item) { /* remove presentation after its signal aborts */ },
  report(error) { /* present a secondary cleanup failure */ },
});

retained.setKeys(keys, removedKeys);
retained.retain(start, end, focusedKey);
const item = retained.get(key);
retained.dispose();
```

| Operation | Proposed behavior |
| --- | --- |
| `setKeys(keys, removedKeys = [])` | Validate a new immutable array of unique string keys before mutation; an identical validated array can be reused. Borrow it without copying. Advance the membership revision on every call. End retained items absent from membership or explicitly listed in `removedKeys`, including reused keys |
| `retain(start = 0, end = keys.length, pin = null)` | Retain the half-open index range plus at most one valid key outside it. End leaving lifetimes before creating entering items. Invoke placement in key order with the full collection index and previous retained item, or null for the first. Skip a completed range only when membership revision, bounds and pin match |
| `get(key)` | Return a currently retained item or undefined, without mounting it |
| `dispose()` | Mark the coordinator ended, abort and remove every retained item, release references and remain idempotent. Ended coordinators acquire nothing; a new view creates a new coordinator |

`create` receives the new incarnation's owned signal. If it throws, that
signal aborts. `place` receives plain retained items; an element may use native
`moveBefore` to preserve focus. `remove` runs after the corresponding signal
aborts. Construction binds partial resources to the signal and returns its item
before placement. The owner calls `dispose` when its view lifetime ends.

The factory adds no scheduler. A completed reconciliation is cached only after
all callbacks finish. The immutable membership and explicit removed metadata
prevent key reuse from being mistaken for retained identity. Applications can
advance the revision with `setKeys` when another presentation input invalidates
placement. No full-list traversal belongs on an unchanged scroll range.

## Failure and cleanup

Duplicate keys, invalid indices and a missing pin fail before changing the
retained range. Callback failures do not promise transactional DOM rollback.
Creation or placement failure ends all newly acquired items from that call,
in reverse acquisition order, before rethrowing the original failure; cleanup
failures go to `report`. Existing retained items remain owned until repaired
or disposed. A disposed view releases every item even if one removal fails,
then throws an aggregate cleanup error. Repeated disposal retries nothing.
Callbacks may query retained items or dispose the owner. Disposal during a
callback stops further acquisition and cleans an item returned by an in-flight
creation. Reentrant membership or range reconciliation fails before mutation.

The comparison must cover key reuse, no-op ranges, selective inputs, focus
pinning, visible keyboard destinations, anchoring, failure rollback and final
resource release. A useful result removes repeated ownership work without a
meaningful time or retained-heap regression. It does not establish the 4 ms or
100 ms readiness targets from synchronous callback timing. The owner decides
whether to adopt a surface after reviewing the comparison.

## Comparison status

The [reported comparison](../../research/large-collections.md#example-mechanics-outcome-2026-10-25)
preserves the checked example behavior and finds similar retained heap.
It fails the proposed no-regression rule for unchanged-range calls at the
smaller size and adds source footprint. The proposal remains open; the
hand-written example remains the shipping-API reference. A completed-range
path and the cost of the failure protocol need review before integration.
These results establish no frame-script or presentation-readiness claim.
