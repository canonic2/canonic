# Testing helpers

Import from `@canonic2/lattice/testing`. These functions have no test-runner
dependency and throw ordinary `Error` objects on a failed check.

See [Testing](../guide/testing.md) for examples that combine output assertions
with dependency checks.

## `checkSelection(name, selection, cases)`

Checks a store or item selection against named record snapshots:

```js
checkSelection(
  'done',
  {
    fields: ['done'],
    read: (task) => task.done,
  },
  {
    open: { text: 'Write the guide', done: false },
    complete: { text: 'Review the guide', done: true },
  },
);
```

Calls `read` once per case. Top-level property reads, destructuring and `in`
tests count as access. Undeclared or unknown fields fail.

Record key enumeration, descriptor reads, writes, prototype changes and
extensibility changes are unsupported and fail even if the reader catches
the refusal. Inherited members and symbols are not recorded as state fields.

## `checkCollectionSelection(name, selection, cases)`

Accepts the same name, selection and named-case arrangement, with
`CollectionView<T>` fixtures instead of record snapshots.

The reader receives only `keys`, `get`, `has` and `size`. Full collections may
supply the fixtures, but mutation and subscription methods are not exposed.

`get(key)` returns an instrumented record or `undefined` for a missing item.
Item-field reads follow the record check's rules. Membership and order reads
do not count as fields because every collection selection already responds to
structural changes.

Writes to the view and its returned keys also fail. Repeated accesses preserve
record and key-array identity within the case.

## Result

Both helpers return `SelectionCheck`:

| Field        | Meaning                                  |
| ------------ | ---------------------------------------- |
| `name`       | Diagnostic selection name                |
| `declared`   | Declared fields                          |
| `cases`      | Each case's `name` and `accessed` fields |
| `accessed`   | Union of fields reached by the cases     |
| `unobserved` | Declared fields no case reached          |

Field lists follow declaration order. Cases follow the own-key order of the
supplied case object. Unobserved declarations are diagnostic and do not fail.

No supplied cases throws. A read failure is wrapped with the selection/case
context and original `cause`. Dependency failures identify the first failing
case and unexpected fields.

## Scope

Checks cover the supplied branches and top-level records. They do not prove
nested immutability, absence of external I/O, result correctness, equality
behavior or subscription timing.

Neither helper runs in the production subscription path.
