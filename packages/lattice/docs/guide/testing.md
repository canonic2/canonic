# Testing

Start by testing the behavior your application promises: actions change the
right values, selections derive the right result, and a disposed view stops
responding.

Lattice's testing helpers add a focused check for selection dependencies.
They work with any test runner and throw ordinary errors.

## Test the result

A pure selection is easy to exercise with plain records:

```js
const status = {
  fields: ['done'],
  read: (task) => (task.done ? 'Complete' : 'In progress'),
};

console.assert(status.read({ done: false }) === 'In progress');
console.assert(status.read({ done: true }) === 'Complete');
```

Use your runner's assertions in a test suite. The console assertions here make
the two expected results visible without requiring a particular runner.

## Check dependencies

A correct result for one snapshot does not prove that `fields` lists every
dependency. Use `checkSelection` for store and item selections:

```js
import { checkSelection } from '@canonic2/lattice/testing';

const description = {
  fields: ['text', 'done'],
  read: (task) => (task.done ? 'Complete' : task.text),
};

const checked = checkSelection('description', description, {
  open: { text: 'Write the guide', done: false },
  complete: { text: 'Review the guide', done: true },
});

console.log(checked.accessed); // ['text', 'done']
console.log(checked.unobserved); // []
```

Both branches matter. If `text` were missing from `fields`, the open case would
fail. A declared field no case reaches appears in `unobserved` instead of
failing: it may indicate an extra dependency or a branch you still need to test.

The helper checks declarations, not the correctness of the selected result.
Keep both kinds of assertion.

## Check a collection selection

Supply named traversal fixtures to `checkCollectionSelection`:

```js
import { createCollection } from '@canonic2/lattice/collection';
import { checkCollectionSelection } from '@canonic2/lattice/testing';

const openCount = {
  fields: ['done'],
  read: (view) => view.keys().filter((key) => !view.get(key).done).length,
};

const checked = checkCollectionSelection('openCount', openCount, {
  empty: createCollection([]),
  mixed: createCollection([
    ['guide', { text: 'Write the guide', done: false }],
    ['review', { text: 'Review the examples', done: true }],
  ]),
});

console.log(checked.accessed); // ['done']
```

The checker exposes only `keys`, `get`, `has` and `size`, even if the fixture is
a full writable collection. Reads of membership need no field declaration.
Reads through `get(key)` check the item fields that were accessed.

## Understand the limits

The helpers record top-level property reads and `in` tests. Unknown or
undeclared fields fail. Enumerating a record's fields, reading descriptors and
mutating records are refused. The collection checker also refuses mutation of
its view and returned keys. Catching a refusal inside `read` still fails.

Only executed branches are checked. These helpers do not prove general purity,
nested immutability, equality behavior or delivery timing.

For report fields and error behavior, see the
[testing-helper reference](../api/testing.md).

## Test the boundary that matters

Pure capability tests can use injected persistence or network functions.
Component and binding behavior needs real browser tests when native controls,
focus, custom-element upgrade or connection callbacks determine the result.

For a task list, test that editing one item preserves neighboring nodes, that
removing a focused item chooses its documented fallback, and that replacing a
key ends the old row's bindings.

For cleanup, cover partial setup and late completion as well as the happy path.
The [filters browser tests](../../examples/filters/filters.browser.test.js)
and [task browser tests](../../examples/tasks/tasks.browser.test.js) demonstrate
those boundaries with shipping APIs.

The [package README](../../README.md#development) lists Lattice's own test commands.

**Next:** [TypeScript and JSDoc](typescript.md)
