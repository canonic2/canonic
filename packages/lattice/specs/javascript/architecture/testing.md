# Testing support

**Status:** current implementation. This page explains the record and collection
selection checkers behind `@canonic2/lattice/testing`. Their guarantees are owned
by [state management](../contracts/state-management.md#declaration-check) and
[collections](../contracts/collections.md#declaration-check). It adds
no behavior; where the contract and this page differ, the contract wins and
this page is corrected.

## Files

| File                             | Contents                                                                                        |
| -------------------------------- | ----------------------------------------------------------------------------------------------- |
| `src/testing/index.js`           | `checkSelection`, `checkCollectionSelection` and the `SelectionCheck` and `SelectionCase` types |
| `src/testing/index.test.js`      | Node unit tests, as [tests](#tests) lists                                                       |
| `src/testing/collection.test.js` | Collection traversal and item-field dependency checking                                         |

The entry point `@canonic2/lattice/testing` points at `src/testing/index.js`.
It is checked by the pure type-check configuration, since it runs in Node and
browsers alike. It imports no test framework, no development dependency and,
at run time, nothing from Lattice; store and collection types are imported
only through JSDoc. No runtime module imports it, which a tooling test checks.

## `checkSelection(name, selection, cases)`

`cases` maps a case name to a snapshot. For each case:

1. Wraps the snapshot in a `Proxy` and calls `selection.read` with it, exactly
   as the store calls `read`.
2. The `get` and `has` traps record the key. A key that is an own field of the
   snapshot is recorded as accessed; a key that is neither own nor inherited is
   recorded as unknown; inherited members, such as `toString`, and symbols are
   ignored. Property reads, destructuring and `in` all reach these traps.
3. The `ownKeys` and `getOwnPropertyDescriptor` traps, reached by
   `Object.keys`, `for…in`, object spread and descriptor reads, and the `set`,
   `defineProperty` and `deleteProperty` traps record an unsupported operation
   and throw into `read`. The check fails on the recorded operation after
   `read` returns, so a `read` that catches the error still fails.
4. An error `read` throws for its own reasons fails the check, wrapped with the
   case as its context and the original error as its cause.
5. A recorded field not in `fields`, or an unknown key, fails the check.

Cases are checked in the own-key order of `cases`, and the first failing case
ends the check. Every failure is an `Error` whose message names the selection,
that case, the declared fields and the unexpected fields, sorted
alphabetically, or the unsupported operation. A call with no cases throws.

When every case passes, the check returns a `SelectionCheck`: the name, the
declared fields, one `SelectionCase` per case with its name and the fields it
accessed, the union of accessed fields, and `unobserved`, the declared fields
absent from that union. Field lists are built by filtering the declared fields,
so they follow declaration order whatever order `read` touched them in.
Unobserved fields never fail the check; the test decides what to do with them.

Proxy tracking belongs only to test support; production subscriptions never
record access. Record proxies are cached per case to preserve identity. Prototype and
extensibility changes are refused along with property writes.

## `checkCollectionSelection(name, selection, cases)`

Cases supply collection traversal views. The checker shares the record check's
per-case tracking, diagnostic formatting and report construction. It passes
`read` a fresh traversal facade exposing only `keys`, `get`, `has` and `size`,
with methods that forward to the fixture. `get` instruments present item
records through the shared tracker; missing keys return `undefined`. Membership
and order reads do not register fields. Key arrays retain identity through a
per-case proxy cache, and reject writes. The facade rejects writes too.

Unsupported operations are recorded before throwing, so catching a refusal in
`read` cannot make a case pass. The view never exposes mutation or subscription
methods from a fixture that is itself a full collection. Nested item values are
not proxied; the check is limited to top-level field dependencies and branches
the cases reach.

## Typing

`checkSelection` is generic over the state `S` and the value `V`, taking a
`Selection<S, V>`, so a typed selection is checked without a cast, and
`Readonly<Record<string, S>>` as its cases.

`ItemSelection<S, V>` has the same structural shape and is accepted by
`checkSelection`. The collection checker takes `CollectionSelection<T, V>`
and `Readonly<Record<string, CollectionView<T>>>`; both checkers return
`SelectionCheck`.

## Tests

Node unit tests in `src/testing/index.test.js`:

| Guarantee                  | Mechanism                                                     | Test                                                                                                                                                        |
| -------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Declared access passes     | Recorded fields within `fields`                               | A selection reading only declared fields, across branches, returns the expected report, with each case's name and accessed fields                           |
| Undeclared access fails    | Recorded field not declared                                   | Accessing an undeclared field in one case throws, naming selection, case, declared and unexpected fields                                                    |
| Deterministic order        | Lists filtered from declared fields; unexpected fields sorted | Fields read in reverse declaration order are reported in declaration order; several unexpected fields are listed alphabetically, for the first failing case |
| Unobserved is a diagnostic | Judged across all cases                                       | A declared field reached by no case is reported in `unobserved` without failing; reached by any case, it is not                                             |
| Destructuring and `in`     | `get` and `has` traps                                         | Both count as access                                                                                                                                        |
| Unsupported access         | `ownKeys`, `getOwnPropertyDescriptor` and write traps         | `Object.keys`, spread, `for…in`, a descriptor read and a write fail, including when `read` catches the error                                                |
| Unknown and inherited keys | Own, inherited or neither                                     | An unknown key fails; inherited members and symbols are ignored                                                                                             |
| Errors from `read`         | Wrapped with the case                                         | A throwing `read` fails with the case and the cause; no cases throws                                                                                        |

The packed-archive consumer imports `@canonic2/lattice/testing` and runs a
check outside the package, and the filters example checks each of its
selections with it in `examples/filters/filters.test.js`.

`src/testing/collection.test.js` covers aggregate branches and report order,
membership-only reads, missing keys, stable fixture identities, unobserved,
unknown and undeclared fields, `in`, caught enumeration/descriptor/mutation
refusals, traversal isolation, read error causes and empty cases. The tasks
example checks its item, scalar and aggregate selections through the public
testing entry point. Packed consumer fixtures exercise both checker exports,
reader types and callback argument inference.
