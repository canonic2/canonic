/** @file Application-owned task actions and selections on the shipping primitives. */
import { createStore } from '@canonic2/lattice';
import { createCollection } from '@canonic2/lattice/collection';

/** @typedef {{ text: string, done: boolean }} Task */
/** @typedef {{ selectedKey: string | null }} TaskState */
/**
 * @typedef {object} Tasks
 * @property {import('@canonic2/lattice/collection').CollectionReader<Task>} items Read-only item ports.
 * @property {import('@canonic2/lattice').StoreReader<TaskState>} state Selection state.
 * @property {{ text: import('@canonic2/lattice/collection').ItemSelection<Task, string>, done: import('@canonic2/lattice/collection').ItemSelection<Task, boolean>, selectedKey: import('@canonic2/lattice').Selection<TaskState, string | null>, summary: import('@canonic2/lattice/collection').CollectionSelection<Task, string> }} select Shared input selections.
 * @property {(key: string | null) => void} choose Selects an existing item or clears selection.
 * @property {(key: string, text: string) => void} rename Changes one item.
 * @property {(key: string) => void} toggle Toggles completion.
 * @property {(before?: string) => string} insert Inserts an application-generated key.
 * @property {(key: string) => void} remove Ends an item; clears its selection.
 * @property {(key: string) => void} reuse Replaces a key in one batch with a new incarnation.
 * @property {(done: boolean) => void} markAll Changes all items in one commit.
 */

/**
 * Create task state independently of the lifetime of a rendered view.
 * @param {number} [count] Initial item count.
 * @param {{ report?: (error: unknown) => void }} [ports] Subscription-error presenter.
 * @returns {Tasks} Read-only views, selections and application actions.
 */
export function createTasks(count = 1000, { report } = {}) {
  if (!Number.isInteger(count) || count < 0) throw new TypeError('Invalid task count');
  const options = report ? { report } : {};
  /** @type {import('@canonic2/lattice/collection').Collection<Task>} */
  const items = createCollection(
    Array.from(
      { length: count },
      (_, index) =>
        /** @type {[string, Task]} */ ([`t${index}`, { text: `Task ${index + 1}`, done: false }]),
    ),
    options,
  );
  const state = createStore(/** @type {TaskState} */ ({ selectedKey: null }), options);
  let next = count;
  return {
    items: {
      keys: items.keys,
      get: items.get,
      has: items.has,
      /** @returns {number} Live membership count. */
      get size() {
        return items.size;
      },
      watchKeys: items.watchKeys,
      watchItem: items.watchItem,
      watch: items.watch,
      watchAggregate: items.watchAggregate,
    },
    state: { get: state.get, watch: state.watch },
    select: {
      text: { fields: ['text'], read: (item) => item.text },
      done: { fields: ['done'], read: (item) => item.done },
      selectedKey: { fields: ['selectedKey'], read: (value) => value.selectedKey },
      summary: {
        fields: ['done'],
        read: (view) => {
          const keys = view.keys();
          let open = 0;
          for (let index = 0; index < keys.length; index++)
            if (!view.get(/** @type {string} */ (keys[index]))?.done) open++;
          return `${open} open / ${keys.length} tasks`;
        },
      },
    },
    choose(key) {
      if (key !== null && !items.has(key)) throw new TypeError('Missing task');
      state.update({ selectedKey: key });
    },
    rename(key, text) {
      items.update(key, { text });
    },
    toggle(key) {
      items.update(key, (item) => ({ done: !item.done }));
    },
    insert(before) {
      const key = `t${next++}`;
      items.insert(key, { text: `Added task ${next}`, done: false }, before);
      return key;
    },
    remove(key) {
      if (!items.has(key)) throw new TypeError('Missing task');
      if (state.get().selectedKey === key) state.update({ selectedKey: null });
      items.remove(key);
    },
    reuse(key) {
      if (!items.has(key)) throw new TypeError('Missing task');
      const keys = items.keys(),
        before = keys[keys.indexOf(key) + 1];
      if (state.get().selectedKey === key) state.update({ selectedKey: null });
      items.batch(() => {
        items.remove(key);
        items.insert(key, { text: `Replacement ${key}`, done: false }, before);
      });
    },
    markAll(done) {
      const keys = items.keys();
      items.batch(() => {
        for (let index = 0; index < keys.length; index++)
          items.update(/** @type {string} */ (keys[index]), { done });
      });
    },
  };
}
