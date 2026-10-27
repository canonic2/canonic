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
