# Lattice JavaScript architecture

This folder describes how Lattice's JavaScript implementation is built: where its source lives, how
its modules depend on each other, and how each implementation meets its
contract. It is written for maintainers. Consumers never see it.

Architecture never defines observable behavior. Every guarantee is owned by a
[contract](../contracts/README.md); a page here explains how the code meets it.
When an implementation turns out to need a different guarantee, the page states
the required contract change explicitly and the contract is changed first.

**Status:** these pages describe current implementations and identify required
fixtures and known gaps separately. They do not assert that checks have run.

| Page                           | Covers                                                                                            |
| ------------------------------ | ------------------------------------------------------------------------------------------------- |
| [Store](store.md)              | The store core's internal structures and how they meet the state management contract              |
| [Collection](collection.md)    | The keyed collection's storage, undo log and delivery, and how they meet the collections contract |
| [Package](package.md)          | Entry-point wiring, file locations, type-check configurations and test fixtures                   |
| [Element helpers](elements.md) | How the element helpers meet the element lifecycle contract                                       |
| [Testing support](testing.md)  | Record and collection selection checks, shared diagnostics and coverage                           |

## Source map

| Location                     | Owns                                                                                                            | Status                                                                        |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `src/index.js`               | The root entry point                                                                                            | Exists; exports the store core                                                |
| `src/modules/store/`         | The store core                                                                                                  | Exists, as [store](store.md) describes                                        |
| `src/modules/collection/`    | The keyed collection behind the `@canonic2/lattice/collection` subpath                                          | Exists, as [collection](collection.md) describes                              |
| `src/report.js`              | Callback error handling shared by the store and the collection: the last-resort reporter and the thenable check | Exists                                                                        |
| `src/modules/elements/`      | The element helpers behind the `@canonic2/lattice/elements` subpath                                             | Exists, as [element helpers](elements.md) describes                           |
| `src/testing/`               | Test support behind `@canonic2/lattice/testing`                                                                 | Exists, as [testing support](testing.md) describes                            |
| `examples/`                  | Runnable examples written on the real API, with their tests                                                     | Filters and task collection examples; a loopback host serves unchanged source |
| `tests/`                     | Package-level import, tooling and platform tests                                                                | Exists                                                                        |
| Type-check configurations    | Pure, DOM and tooling checks                                                                                    | Exist, as [package](package.md) describes                                     |
| `types/shared-platform.d.ts` | Platform APIs pure modules may use; development only, not shipped                                               | Exists                                                                        |

## Dependency direction

```text
src/index.js        → src/modules/store/
src/testing/        → ECMAScript built-ins only; store and collection types in JSDoc
src/modules/store/  → src/report.js; ECMAScript built-ins and shared platform APIs
src/modules/collection/ → src/report.js; ECMAScript built-ins and shared platform APIs
src/report.js       → ECMAScript built-ins and shared platform APIs only
src/modules/elements/ → ECMAScript built-ins and methods of the objects it is given; DOM types in JSDoc
runtime modules     ↛ src/testing/, tests/ or development tools
```

The import rules these lines follow, purity of the root entry point, no cycles
and no aggregate internal imports, are owned by
[delivery](../contracts/delivery.md#native-delivery-and-import-isolation).

## Principles for these pages

- Start with the simplest structure that meets the contract. A more elaborate
  one, such as an index, a pool or a specialized queue, is proposed only with
  the comparison [performance and evidence](../contracts/performance.md)
  requires.
- Map each non-obvious guarantee to its mechanism and to the test that
  establishes it.
- Keep unresolved choices visible as open questions rather than deciding them
  in passing.
