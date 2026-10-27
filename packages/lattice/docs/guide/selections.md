# Selections

State often needs a little interpretation before it reaches the screen. A
boolean `done` field becomes a status message; a task's text becomes a heading.

A selection keeps that interpretation beside its dependencies:

```js
const status = {
  fields: ['done'],
  read: (state) => (state.done ? 'Complete' : 'In progress'),
};
```

The selection is a plain object. `fields` declares which state fields matter,
and `read` calculates the value.

## Use a selection

Here is a complete example:

```js
import { createStore } from '@canonic2/lattice';

const task = createStore({ text: 'Write the guide', done: false });
const status = {
  fields: ['done'],
  read: (state) => (state.done ? 'Complete' : 'In progress'),
};

task.watch(status, (value) => console.log(value));
// 'In progress'

task.update({ text: 'Review the guide' }); // Does not call status.read.
task.update({ done: true }); // Logs 'Complete'.
```

A field change makes its dependent selections eligible to run. Their selected
values then decide whether a callback is needed.

You can also call `status.read(task.get())` to calculate the value once without
subscribing.

## Declare every dependency

Suppose the message includes the task text:

```js
const description = {
  fields: ['text', 'done'],
  read: (state) => state.text + (state.done ? ' — complete' : ' — in progress'),
};
```

Both fields belong in `fields`. If you leave one out, a change to it can fail
to update the message.

Keep `read` pure: calculate from the supplied state without changing it,
reading another store or performing I/O. Test representative branches with
the [selection checker](testing.md#check-dependencies).

Store and item selections require at least one field. Collection-wide
selections can use `fields: []` for membership-only values such as item count.

## Compare derived values

A callback runs only when the selected result differs under the selection's
equality function. The default is `Object.is`.

For primitives and references already in state, that is usually enough. If a
reader creates an object each time, you can compare its immediate contents:

```js
import { shallow } from '@canonic2/lattice';

const heading = {
  fields: ['text', 'done'],
  read: (state) => ({ text: state.text, tone: state.done ? 'muted' : 'normal' }),
  equals: shallow,
};
```

`shallow` compares plain object fields or array entries with `Object.is`.
It does not compare nested data deeply.

A custom `equals(next, previous)` receives two results to compare. Treat it as
a pure equality test; it must return `true` for values the callback can consider
unchanged.

Prefer returning a primitive or an existing reference when it expresses what
you need. An equality function can suppress delivery, but the reader still
does the work of constructing its result.

## Share the definition

Selections do not belong to a particular subscription. Several consumers can
use the same selection, and item selections can be reused for different keys.

Keep application selections with the actions that own their state. The
[bindings chapter](bindings.md) shows how to expose them to a view.

**Next:** [Watchers](watchers.md)
