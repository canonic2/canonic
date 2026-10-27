# Bindings and actions

We have a store and a component. A binding connects them:

- Selected state becomes a public component input.
- A component event calls an application action.

The binding is the only function that needs to know both sides.

## Give the task named actions

Instead of handing the writable store to every caller, keep it inside a factory.
Save this as `task.js`:

```js
import { createStore } from '@canonic2/lattice';

export function createTask() {
  const store = createStore({ text: 'Write the guide', done: false });

  return {
    state: { get: store.get, watch: store.watch },

    select: {
      done: { fields: ['done'], read: (state) => state.done },
      status: {
        fields: ['done'],
        read: (state) => (state.done ? 'Complete' : 'In progress'),
      },
    },

    setDone(done) {
      store.update({ done });
    },

    rename(text) {
      store.update({ text });
    },
  };
}
```

This application factory is called a **capability**: it owns one piece of
behavior and exposes the operations its callers need. `createTask`,
`setDone` and `rename` are application names, not Lattice exports.

Consumers can read and watch `state`, but the object does not expose `update`.
Selections live beside the actions that can change their dependencies.

## Write the binding

Save this as `bindings.js`:

```js
export function bindTaskToggle(toggle, task, { signal, report }) {
  if (signal.aborted) return;

  const failed = new AbortController();
  const own = AbortSignal.any([signal, failed.signal]);

  try {
    task.state.watch(
      task.select.done,
      (value) => {
        toggle.done = value;
      },
      { signal: own },
    );

    toggle.addEventListener(
      'acme-task-toggle',
      (event) => {
        try {
          task.setDone(event.detail.done);
        } catch (error) {
          report(error);
        }
      },
      { signal: own },
    );
  } catch (error) {
    failed.abort();
    throw error;
  }
}
```

The initial watcher callback assigns `done` before the binding returns.
If setup fails, the internal controller releases anything already acquired.
It never aborts the caller's signal.

The event handler passes the requested value to the action. It does not write
the store or inspect the component's shadow DOM. This example has synchronous
actions and expects a synchronous, non-throwing `report` function.

Bindings follow the same argument order: target, capability, then options.
An item binding adds its key after the capability; multiple targets can be
grouped in the first argument.

## Mount the view

Use the files above and `task-toggle.js` from the previous chapter. An
`app.js` can compose them as follows:

```js
import { TaskToggle, defineTaskToggle } from './task-toggle.js';
import { createTask } from './task.js';
import { bindTaskToggle } from './bindings.js';

defineTaskToggle();

const task = createTask();
const lifetime = new AbortController();
const toggle = new TaskToggle();
const status = document.createElement('p');

try {
  bindTaskToggle(toggle, task, {
    signal: lifetime.signal,
    report: (error) => console.error(error),
  });

  task.state.watch(
    task.select.status,
    (value) => {
      status.textContent = value;
    },
    { signal: lifetime.signal },
  );

  document.body.append(toggle, status);
} catch (error) {
  lifetime.abort();
  throw error;
}

window.addEventListener('pagehide', () => lifetime.abort(), { once: true });
```

Load `app.js` with `<script type="module" src="./app.js"></script>` after
the import map. Checking the toggle now changes task state and updates the
status paragraph.

The entry point creates the lifetime. The binding serves it. The component
receives only values and emits intent.

## Actions that do I/O

Inject external work into the capability. For example, a factory accepting
`{ save }` can implement its `setDone` action like this:

```js
setDone(done) {
  store.update({ done });
  return save(store.get());
}
```

This method replaces the corresponding method in the capability's returned
object; `store` is that capability's private store. The runnable
[filters capability](../../examples/filters/filters.js) shows a complete
`save` port and the corresponding
[bindings](../../examples/filters/bindings.js) handle both synchronous failures
and rejected promises.

When an event action returns a promise or thenable, its binding must observe
rejection. Report while the view is live; a rejection arriving after teardown
still needs global error handling. Cancellation and stale-result policy belong
to the capability that owns the operation.

**Next:** [Lifetimes](lifecycle.md)
