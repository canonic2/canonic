# Testing support

**Status:** implemented as described. This page explains how the selection
declaration check behind `@canonic2/lattice/testing` meets the
[declaration check](../contracts/state-management.md#declaration-check). It adds
no behavior; where the contract and this page differ, the contract wins and
this page is corrected.

## Files

| File | Contents |
| --- | --- |
| `src/testing/index.js` | `checkSelection` and the `SelectionCheck` and `SelectionCase` types |
| `src/testing/index.test.js` | Node unit tests, as [tests](#tests) lists |

The entry point `@canonic2/lattice/testing` points at `src/testing/index.js`.
It is checked by the pure type-check configuration, since it runs in Node and
browsers alike. It imports no test framework, no development dependency and,
at run time, nothing from Lattice; it names the store's `Selection` type only
in JSDoc. No runtime module imports it, which a tooling test checks.

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

The `Proxy` exists only during the check; production code never records
access.

## Typing

`checkSelection` is generic over the state `S` and the value `V`, taking a
`Selection<S, V>`, so a typed selection is checked without a cast, and
`Readonly<Record<string, S>>` as its cases.

## Tests

Node unit tests in `src/testing/index.test.js`:

| Guarantee | Mechanism | Test |
| --- | --- | --- |
| Declared access passes | Recorded fields within `fields` | A selection reading only declared fields, across branches, returns the expected report, with each case's name and accessed fields |
| Undeclared access fails | Recorded field not declared | Accessing an undeclared field in one case throws, naming selection, case, declared and unexpected fields |
| Deterministic order | Lists filtered from declared fields; unexpected fields sorted | Fields read in reverse declaration order are reported in declaration order; several unexpected fields are listed alphabetically, for the first failing case |
| Unobserved is a diagnostic | Judged across all cases | A declared field reached by no case is reported in `unobserved` without failing; reached by any case, it is not |
| Destructuring and `in` | `get` and `has` traps | Both count as access |
| Unsupported access | `ownKeys`, `getOwnPropertyDescriptor` and write traps | `Object.keys`, spread, `for…in`, a descriptor read and a write fail, including when `read` catches the error |
| Unknown and inherited keys | Own, inherited or neither | An unknown key fails; inherited members and symbols are ignored |
| Errors from `read` | Wrapped with the case | A throwing `read` fails with the case and the cause; no cases throws |

The packed-archive consumer imports `@canonic2/lattice/testing` and runs a
check outside the package, and the filters example checks each of its
selections with it in `examples/filters/filters.test.js`.
