# API reference

Use the [guide](../README.md#essentials) to learn the concepts. These pages
collect signatures, options, types and edge cases for lookup.

## Entry points

| Import                         | Runtime exports                                                       |
| ------------------------------ | --------------------------------------------------------------------- |
| `@canonic2/lattice`            | `createStore`, `shallow`                                              |
| `@canonic2/lattice/collection` | `createCollection`                                                    |
| `@canonic2/lattice/elements`   | `captureInputs`, `replayInputs`, `skipUpgradeAttribute`, `defineOnce` |
| `@canonic2/lattice/testing`    | `checkSelection`, `checkCollectionSelection`                          |

The root, collection and testing entries work without a DOM. Element helpers
use browser objects supplied by their callers. Importing an entry registers
nothing and starts no background work.

The [quick start](../guide/quick-start.md#use-lattice-in-your-own-project)
explains native-browser import maps and local package dependencies.

## Shared patterns

| Operation              | Store                            | Collection                                |
| ---------------------- | -------------------------------- | ----------------------------------------- |
| Read a record          | `get()`                          | `get(key)`                                |
| Patch a record         | `update(patch)`                  | `update(key, patch)`                      |
| Group writes           | `batch(fn)`                      | `batch(fn)`                               |
| Watch a selected value | `watch(selection, run, options)` | `watch(selection, run, options)`          |
| Watch one item         | —                                | `watchItem(key, selection, run, options)` |
| Watch membership       | —                                | `watchKeys(run, options)`                 |
| Change membership once | —                                | `updateMembership(changes)`               |
| Maintain a reduction   | —                                | `watchAggregate(selection, run, options)` |

A selection uses `fields`, `read` and optional `equals`. Value callbacks receive
`(value, previous)`. Membership callbacks add `change` third. Subscription
options come last and accept an optional `signal`.

Each owner commits independently. There is no cross-store or
store-and-collection transaction.

## Public types

| Entry      | Types                                                                                                                                                                                                                               |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Root       | `Store<S>`, `StoreReader<S>`, `Selection<S, V>`, `StoreOptions`, `WatchOptions`                                                                                                                                                     |
| Collection | `Collection<T>`, `CollectionReader<T>`, `CollectionView<T>`, `ItemSelection<T, V>`, `CollectionSelection<T, V>`, `AggregateSelection<T, C, V>`, `MembershipChanges<T>`, `KeysChange`, `CollectionOptions`, `CollectionWatchOptions` |
| Testing    | `SelectionCheck`, `SelectionCase`                                                                                                                                                                                                   |

See [TypeScript and JSDoc](../guide/typescript.md) for consumer configuration
and typed selections.

## Application conventions

Capabilities, bindings and view mount functions are application code.
Lattice does not export `createTask`, `bindTaskToggle` or a list widget.

Capabilities expose read-only state, selections and named actions. Bindings
use `bindX(target, capability, options)`, adding an item key after the capability
when needed. A capability exposes `dispose()` only when it owns resources
that need it.

## Reference pages

- [Stores](store.md)
- [Collections](collection.md)
- [Element helpers](elements.md)
- [Testing helpers](testing.md)
