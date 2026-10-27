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
      watchAggregate: items.watchAggregate,
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
