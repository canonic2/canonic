# Rendering lists

A collection owns records. A list view owns rows. Connect them by keeping one
row and one binding lifetime for each retained item incarnation.

This chapter uses the runnable [tutorial](../../examples/tutorial/README.md),
whose [browser cases](../../examples/tutorial/list.browser.test.js) import the
same capability and row mount shown below. It starts with a small, fully
mounted list. Use that approach until
your application's measurements justify [windowing](windowing.md).

For the `examples/hello-lattice/` page, add this entry to the existing import map:

```json
"@canonic2/lattice/collection": "../../src/modules/collection/collection.js"
```

## Give the list application actions

The tutorial's [tasks.js](../../examples/tutorial/tasks.js) can also be saved
beside your example page. It extends the single-task
capability into a collection:

```js
/** @file Tutorial capability: independent task records and explicit synchronous actions. */
import { createCollection } from '@canonic2/lattice/collection';

/** @typedef {{ text: string, done: boolean }} Task */
/**
 * Read-only observation and application-owned actions.
 * @typedef {object} Tasks
 * @property {import('@canonic2/lattice/collection').CollectionReader<Task>} items Read and subscribe.
 * @property {{text: import('@canonic2/lattice/collection').ItemSelection<Task, string>, done: import('@canonic2/lattice/collection').ItemSelection<Task, boolean>}} select Shared selections.
 * @property {(key: string) => void} toggle Flip completion.
 * @property {(key: string, done: boolean) => void} setDone Set completion.
 * @property {(key: string, text: string) => void} rename Change a task's text.
 * @property {(key: string) => void} remove Remove a task.
 * @property {(key: string, text: string, before?: string) => void} add Insert a task.
 * @property {(key: string, text: string) => void} replace Replace an incarnation in its current position.
 * @property {() => void} reverse Reverse task order.
 */

/**
 * Creates the two tasks used throughout the guide.
 * @returns {Tasks} A capability with no exposed collection writes.
 */
export function createTasks() {
  const items = createCollection([
    ['guide', { text: 'Write the guide', done: false }],
    ['review', { text: 'Review the examples', done: false }],
  ]);
  return {
    items: {
      keys: items.keys,
      get: items.get,
      has: items.has,
      watch: items.watch,
      watchKeys: items.watchKeys,
      watchItem: items.watchItem,
      /** @returns {number} The current membership size. */
      get size() {
        return items.size;
      },
    },
    select: {
      text: { fields: ['text'], read: (task) => task.text },
      done: { fields: ['done'], read: (task) => task.done },
    },
    /** @param {string} key The task to toggle. */
    toggle(key) {
      items.update(key, (task) => ({ done: !task.done }));
    },
    /**
     * Sets completion from an observed component event.
     * @param {string} key The task.
     * @param {boolean} done The desired completion.
     */
    setDone(key, done) {
      items.update(key, { done });
    },
    /**
     * Renames a task.
     * @param {string} key The task.
     * @param {string} text The desired text.
     */
    rename(key, text) {
      items.update(key, { text });
    },
    /** @param {string} key The task to remove. */
    remove(key) {
      items.remove(key);
    },
    /**
     * Inserts a new task.
     * @param {string} key The new key.
     * @param {string} text The initial text.
     * @param {string} [before] An existing insertion anchor.
     */
    add(key, text, before) {
      items.insert(key, { text, done: false }, before);
    },
    /**
     * Replaces an existing task incarnation without changing its position.
     * @param {string} key The task to replace.
     * @param {string} text The replacement text.
     */
    replace(key, text) {
      const keys = items.keys();
      const index = keys.indexOf(key);
      if (index < 0) throw new TypeError('Cannot replace a missing task.');
      const before = keys[index + 1];
      items.batch(() => {
        items.remove(key);
        items.insert(key, { text, done: false }, before);
      });
    },
    /** Reverses order while preserving each incarnation. */
    reverse() {
      items.reorder([...items.keys()].reverse());
    },
  };
}
```

The view can subscribe, but it changes data only through named actions.

## Build and bind one row

A row can use native elements. The tutorial's
[task-list.js](../../examples/tutorial/task-list.js) begins with this helper:

```js
/** @file Tutorial keyed rows: subscriptions and native controls share a row lifetime. */
/** @typedef {import('./tasks.js').Tasks} Tasks */
/** @typedef {{node: HTMLLIElement, stop: () => void}} Row */

/**
 * Builds a native row and its two item watchers; partial failure releases both.
 * @param {Tasks} tasks The application capability.
 * @param {string} key The item incarnation currently held by this key.
 * @param {AbortSignal} signal The list lifetime.
 * @returns {Row} Retained presentation and idempotent binding cleanup.
 */
function createRow(tasks, key, signal) {
  const own = new AbortController();
  const active = AbortSignal.any([signal, own.signal]);
  const node = document.createElement('li');
  const label = document.createElement('label');
  const checkbox = document.createElement('input');
  const text = document.createElement('span');

  checkbox.type = 'checkbox';
  label.append(checkbox, text);
  node.append(label);

  try {
    tasks.items.watchItem(
      key,
      tasks.select.text,
      (value) => {
        text.textContent = value;
      },
      { signal: active },
    );

    tasks.items.watchItem(
      key,
      tasks.select.done,
      (value) => {
        if (checkbox.checked !== value) checkbox.checked = value;
      },
      { signal: active },
    );

    checkbox.addEventListener(
      'change',
      () => {
        try {
          tasks.toggle(key);
        } catch (error) {
          reportError(error);
        }
      },
      {
        signal: active,
      },
    );
  } catch (error) {
    own.abort();
    throw error;
  }

  return { node, stop: () => own.abort() };
}
```

The row gets its current values before it is inserted. Updating its text
changes one text span. Other rows do not need to run their readers.

The item's removal ends its item watchers, but the checkbox listener is a
separate resource. `stop` ends the complete row binding.

## Keep rows by key

Add this export below `createRow` in the same file:

```js
/**
 * Mounts keyed rows into an empty, view-owned list; abort releases bindings before nodes.
 * @param {HTMLUListElement} list The presentation container.
 * @param {Tasks} tasks Application observation and actions.
 * @param {{signal: AbortSignal}} options The view lifetime.
 * @returns {void}
 */
export function mountTaskRows(list, tasks, { signal }) {
  if (signal.aborted) return;

  const failed = new AbortController();
  const own = AbortSignal.any([signal, failed.signal]);
  /** @type {Map<string, Row>} */
  const rows = new Map();

  /**
   * Ends one row binding before removing its presentation.
   * @param {string} key The retained row.
   * @returns {void}
   */
  function removeRow(key) {
    const row = rows.get(key);
    if (!row) return;
    rows.delete(key);
    row.stop();
    row.node.remove();
  }

  own.addEventListener(
    'abort',
    () => {
      for (const key of [...rows.keys()].reverse()) {
        try {
          removeRow(key);
        } catch (error) {
          reportError(error);
        }
      }
    },
    { once: true },
  );

  try {
    tasks.items.watchKeys(
      (keys, previous, change) => {
        const present = new Set(keys);
        const removed = new Set(change?.removed ?? []);

        for (const key of rows.keys()) {
          if (!present.has(key) || removed.has(key)) removeRow(key);
        }

        /** @type {Element | null} */
        let before = list.firstElementChild;
        for (const key of keys) {
          let row = rows.get(key);
          if (row === undefined) {
            row = createRow(tasks, key, own);
            rows.set(key, row);
          }
          const node = row.node;
          if (node !== before) {
            if (node.isConnected && list.isConnected) list.moveBefore(node, before);
            else list.insertBefore(node, before);
          }
          before = node.nextElementSibling;
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

Use this mount function with an empty list owned by the view. It retains nodes
for existing items and moves them when order changes. A key in `change.removed`
ends its row even if the same string appears in the new keys: that is a new
incarnation.

Cleanup aborts row bindings before removing their nodes. The example uses
native controls and synchronous actions. Promise-returning actions also need
rejection handling, as described in [Bindings and actions](bindings.md).

To put it on a page, load this entry after the appropriate import map:

```js
import { createTasks } from './tasks.js';
import { mountTaskRows } from './task-list.js';

const tasks = createTasks();
const lifetime = new AbortController();
const list = document.createElement('ul');

mountTaskRows(list, tasks, { signal: lifetime.signal });
document.body.append(list);

window.addEventListener('pagehide', () => lifetime.abort(), { once: true });
```

Checking either row updates its record. `tasks.rename('guide', 'Review the guide')`
updates the first label without rebuilding its checkbox.

## Keep selection at list level

For a list with one selected item, keep one selected key in application state.
Bind it to one input on your list component, rather than making every row watch
the shared key.

The runnable task example does this:

```js
tasks.state.watch(
  tasks.select.selectedKey,
  (key) => {
    list.selectedKey = key;
  },
  { signal },
);
```

Here `tasks` and `list` are the capability and custom list from
`examples/tasks/`. The list's setter updates only the old and new retained rows.
Setting the input emits no selection event; the capability owns the selection
policy.

## Preserve interaction

Retain the row a person is editing. Use state-preserving moves for reorder,
and define where focus goes when an item is removed.

A more complex row may need to retain draft text, selection and its connection
resources. The complete [task-list example](../../examples/tasks/README.md)
covers key replacement, focus fallback, keyboard navigation and anchoring.
Its responsibilities remain separated between capability, binding,
presentation and view composition.

**Next:** [Choosing state ownership](state-ownership.md)
