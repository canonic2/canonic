/** @file Application composition for keyed row lifetimes and retained ranges. */
import { TaskRow } from './task-row.js';
import { bindTaskRow } from './bindings.js';

/**
 * Coordinate collection membership and retained bindings outside presentation.
 * The view signal, rather than temporary DOM attachment, owns these resources.
 * @param {import('./task-list.js').TaskList} list Plain-input presentation.
 * @param {import('./tasks.js').Tasks} tasks Capability ports.
 * @param {{ signal: AbortSignal, report: (error: unknown) => void }} lifetime View ownership.
 * @returns {{ render: () => void, focusKey: (key: string) => void, dispose: () => void }} Application view commands.
 */
export function coordinateTaskList(list, tasks, { signal, report }) {
  const stopped = new AbortController(),
    own = AbortSignal.any([signal, stopped.signal]);
  /** @type {Map<string, { row: TaskRow, own: AbortController, index: number }>} */
  const rows = new Map();
  /** @type {readonly string[]} */
  let keys = [];
  let revision = 0,
    ended = false;
  /** @type {{ revision: number, start: number, end: number, focus: string | null } | null} */
  let completed = null;
  /**
   * Route secondary/setup or event failures without throwing from a listener.
   * @param {unknown} error Failure.
   * @returns {void}
   */
  function present(error) {
    try {
      report(error);
    } catch (failure) {
      reportError(failure);
    }
  }
  /**
   * End ownership before the next presentation input removes the node.
   * @param {string} key Incarnation.
   * @returns {void}
   */
  function end(key) {
    const item = rows.get(key);
    rows.delete(key);
    item?.own.abort();
  }
  /**
   * Publish an immutable projection through the element input.
   * @param {readonly string[]} order Retained order.
   * @returns {void}
   */
  function publish(order) {
    list.items = Object.freeze(
      order.map((key) => {
        const item = /** @type {{ row: TaskRow, index: number }} */ (rows.get(key));
        return { key, row: item.row, index: item.index };
      }),
    );
  }
  /**
   * Reconcile only a changed completed range; bindings are acquired before insertion.
   * @returns {void}
   */
  function render() {
    if (ended || !list.isConnected) return;
    const { start, end: finish, focus } = list.range(),
      old = completed;
    if (
      old &&
      old.revision === revision &&
      old.start === start &&
      old.end === finish &&
      old.focus === focus
    )
      return;
    completed = null;
    const desired = keys.slice(start, finish),
      positions = new Map(desired.map((key, index) => [key, start + index]));
    if (focus !== null && !desired.includes(focus)) {
      const index = keys.indexOf(focus);
      if (index >= 0) {
        if (index < start) desired.unshift(focus);
        else desired.push(focus);
        positions.set(focus, index);
      }
    }
    const retained = new Set(desired);
    for (const key of rows.keys()) if (!retained.has(key)) end(key);
    /** @type {string[]} */
    const acquired = [];
    try {
      for (const key of desired)
        if (!rows.has(key)) {
          const owner = new AbortController(),
            row = new TaskRow();
          row.dataset.key = key;
          row.tabIndex = -1;
          row.setAttribute('role', 'listitem');
          try {
            bindTaskRow(row, tasks, key, { signal: owner.signal, report });
          } catch (error) {
            owner.abort();
            throw error;
          }
          rows.set(key, { row, own: owner, index: /** @type {number} */ (positions.get(key)) });
          acquired.push(key);
        }
      for (const key of desired)
        /** @type {{ index: number }} */ (rows.get(key)).index = /** @type {number} */ (
          positions.get(key)
        );
      publish(desired);
      completed = { revision, start, end: finish, focus };
    } catch (error) {
      for (let index = acquired.length - 1; index >= 0; index--)
        end(/** @type {string} */ (acquired[index]));
      try {
        publish(keys.filter((key) => rows.has(key)));
      } catch (cleanup) {
        present(cleanup);
      }
      throw error;
    }
  }
  /**
   * Mount a destination before focusing, then drop an obsolete pin.
   * @param {string} key Destination.
   * @returns {void}
   */
  function focusKey(key) {
    if (ended || !keys.includes(key)) return;
    list.scrollToKey(key);
    render();
    list.focusRow(key);
    render();
  }
  /**
   * Apply committed membership, including replaced incarnations, before the range guard.
   * @param {readonly string[]} next Membership.
   * @param {readonly string[] | undefined} _previous Last delivered membership.
   * @param {import('@canonic2/lattice/collection').KeysChange} [change] Commit metadata.
   * @returns {void}
   */
  function membership(next, _previous, change) {
    const focused = list.focusedKey(),
      oldIndex = keys.indexOf(focused ?? '');
    list.keys = next; // Validation and anchoring finish before any row lifetime ends.
    keys = next;
    revision++;
    completed = null;
    if (change) for (const key of change.removed) end(key);
    const presentKeys = new Set(keys);
    for (const key of rows.keys()) if (!presentKeys.has(key)) end(key);
    render();
    if (focused !== null && (change?.removed.includes(focused) || !keys.includes(focused))) {
      const fallback = keys[Math.min(oldIndex, keys.length - 1)];
      if (fallback === undefined) list.focusRow(null);
      else focusKey(fallback);
    }
    if (!list.isConnected) {
      for (const [key, item] of rows) item.index = keys.indexOf(key);
      publish(keys.filter((key) => rows.has(key)));
    }
  }
  /**
   * Attempt every release before surfacing standalone cleanup failures.
   * @returns {void}
   */
  function dispose() {
    if (ended) return;
    ended = true;
    completed = null;
    const errors = [];
    try {
      stopped.abort();
    } catch (error) {
      errors.push(error);
    }
    const all = Array.from(rows.keys());
    for (let index = all.length - 1; index >= 0; index--)
      try {
        end(/** @type {string} */ (all[index]));
      } catch (error) {
        errors.push(error);
      }
    keys = [];
    try {
      list.items = [];
    } catch (error) {
      errors.push(error);
    }
    if (errors.length) throw new AggregateError(errors, 'Task view cleanup failed');
  }
  if (own.aborted) {
    ended = true;
    return { render, focusKey, dispose };
  }
  own.addEventListener(
    'abort',
    () => {
      try {
        dispose();
      } catch (error) {
        present(error);
      }
    },
    { once: true },
  );
  try {
    list.addEventListener(
      'acme-task-range',
      () => {
        try {
          render();
        } catch (error) {
          present(error);
        }
      },
      { signal: own },
    );
    list.addEventListener(
      'acme-task-focus',
      (event) => {
        const key = /** @type {CustomEvent<{ key: string }>} */ (event).detail.key;
        try {
          focusKey(key);
        } catch (error) {
          present(error);
        }
      },
      { signal: own },
    );
    const observer = new ResizeObserver(() => {
      if (!own.aborted)
        try {
          render();
        } catch (error) {
          present(error);
        }
    });
    own.addEventListener('abort', () => observer.disconnect(), { once: true });
    observer.observe(list);
    tasks.items.watchKeys(membership, { signal: own });
  } catch (error) {
    try {
      dispose();
    } catch (cleanup) {
      present(cleanup);
    }
    throw error;
  }
  return { render, focusKey, dispose };
}
