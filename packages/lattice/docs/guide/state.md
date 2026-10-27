# State

A store holds related values that your application changes together. For a
task, those values might be its text and completion state.

```js
import { createStore } from '@canonic2/lattice';

const task = createStore({
  text: 'Write the guide',
  done: false,
});
```

The initial object defines the store's fields. You can update those fields
throughout the store's lifetime.

## Read a snapshot

Call `get()` to read the current state:

```js
const before = task.get();

console.log(before.text); // 'Write the guide'
console.log(before.done); // false
```

The result is a shallowly frozen snapshot. Lattice returns the same object
until a changing write publishes another one.

```js
task.update({ done: true });

console.log(before.done); // false
console.log(task.get().done); // true
```

Keep a snapshot when you need to compare with an earlier state. Read again
when you need the current state.

## Update fields

Pass `update` an object containing the fields to change:

```js
task.update({ text: 'Review the guide' });
```

This patches the state. It leaves `done` unchanged.

When the new value depends on the current value, pass a function instead:

```js
task.update((state) => ({ done: !state.done }));
```

The function receives the current snapshot and returns a patch. It should
calculate that patch without making another write.

Unknown fields throw a `TypeError`. A misspelling does not silently add state:

```js
task.update({ completed: true }); // Throws: this store has no 'completed' field.
```

## Unchanged values

Updating a field to its current value does not create a snapshot or notify
watchers. Equality uses `Object.is`.

```js
const current = task.get();
task.update({ done: current.done });

console.log(task.get() === current); // true
```

This lets actions describe the desired state without first checking whether
the write is necessary.

## Replace nested values

Freezing is shallow. If a field contains an object or array, replace that value
rather than mutating it:

```js
const editor = createStore({
  task: { text: 'Write the guide', done: false },
});

editor.update((state) => ({
  task: { ...state.task, done: true },
}));
```

Mutating `editor.get().task.done` directly would bypass notification and alter
data referenced by earlier snapshots. Use a patch to give the change a new
identity.

## Group related writes

Use one patch when an action changes several fields:

```js
task.update({ text: 'Start the next task', done: false });
```

When several operations need to write independently, wrap them in `batch`:

```js
task.batch(() => {
  task.update({ text: 'Start the next task' });
  task.update({ done: false });
});
```

Reads inside the batch see its pending writes. Watchers receive the completed
result when the outermost batch returns.

If the callback throws, its writes roll back. A nested batch is a savepoint:
catching its error lets the outer batch continue with its earlier writes.
A batch with no net field change delivers nothing.

Batch callbacks must be synchronous. Returning a promise or another thenable
rolls back and throws a `TypeError`. Finish asynchronous work first, then make
a separate update or synchronous batch.

**Next:** [Selections](selections.md)
