# Lattice architecture

This folder describes how Lattice itself is built: where its source lives, how
its modules depend on each other, and how each implementation meets its
contract. It is written for maintainers. Consumers never see it.

Architecture never defines observable behavior. Every guarantee is owned by a
[contract](../contracts/README.md); a page here explains how the code meets it.
When an implementation turns out to need a different guarantee, the page states
the required contract change explicitly and the contract is changed first.

**Status:** draft for review. No runtime code exists beyond the empty entry
point.

| Page | Covers |
| --- | --- |
| [Store](store.md) | The store core's internal structures and how they meet the state management contract |
| [Package](package.md) | Entry-point wiring, file locations, type-check configurations and test fixtures |
| [Element helpers](elements.md) | How `captureInputs`, `replayInputs` and `defineOnce` meet the element lifecycle contract |
| [Testing support](testing.md) | The declaration check's implementation; not started, written when work on the checker begins |

## Source map

| Location | Owns | Status |
| --- | --- | --- |
| `src/index.js` | The root entry point | Exists; exports nothing |
| `src/modules/store/` | The store core | Proposed in [store](store.md) |
| `src/modules/elements/` | The element helpers behind the `canonic-lattice/elements` subpath | Exists, as [element helpers](elements.md) describes |
| `src/testing/` | Test support behind `canonic-lattice/testing` | Location proposed in [package](package.md); no export until implemented |
| `tests/` | Package-level import, tooling and platform tests | Exists |
| Type-check configurations | Pure, DOM and tooling checks | Proposed in [package](package.md); one combined configuration exists today |

## Dependency direction

```text
src/index.js        → src/modules/store/
src/testing/        → src/modules/store/ and platform APIs
src/modules/store/  → ECMAScript built-ins and shared platform APIs only
src/modules/elements/ → ECMAScript built-ins only; DOM types in JSDoc
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
