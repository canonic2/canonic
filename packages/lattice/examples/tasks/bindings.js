/** @file Application bindings: task selections to row/list inputs and row intent to actions. */

/**
 * Bind one incarnation, rolling back partial setup without aborting its owner.
 * @param {import('./task-row.js').TaskRow} row Visual component.
 * @param {import('./tasks.js').Tasks} tasks Capability ports.
 * @param {string} key The row's incarnation key.
 * @param {{ signal: AbortSignal, report: (error: unknown) => void }} lifetime Ownership and error presentation.
 * @returns {void}
 */
export function bindTaskRow(row, tasks, key, { signal, report }) {
  if (signal.aborted) return;
  const failed = new AbortController(), own = AbortSignal.any([signal, failed.signal]);
  /** Present a synchronous application action failure. @param {() => void} action Intent action. @returns {void} */
  const act = (action) => {
    try { action(); } catch (error) { try { report(error); } catch (failure) { reportError(failure); } }
  };
  try {
    tasks.items.watchItem(key, tasks.select.text, (value) => { row.text = value; }, { signal: own });
    tasks.items.watchItem(key, tasks.select.done, (value) => { row.done = value; }, { signal: own });
    row.addEventListener('acme-task-select', () => act(() => tasks.choose(key)), { signal: own });
    row.addEventListener('acme-task-toggle', () => act(() => tasks.toggle(key)), { signal: own });
    row.addEventListener('acme-task-rename', (event) => {
      const detail = /** @type {CustomEvent<{ text: string }>} */ (event).detail;
      act(() => tasks.rename(key, detail.text));
    }, { signal: own });
  } catch (error) { failed.abort(); throw error; }
}

/**
 * Apply one selectedKey input and summary for the view lifetime.
 * @param {import('./task-list.js').TaskList} list Presentation owner.
 * @param {HTMLOutputElement} summary Native summary input.
 * @param {import('./tasks.js').Tasks} tasks Application capability.
 * @param {AbortSignal} signal View lifetime.
 * @returns {void}
 */
export function bindTaskList(list, summary, tasks, signal) {
  if (signal.aborted) return;
  const failed = new AbortController(), own = AbortSignal.any([signal, failed.signal]);
  try {
    tasks.state.watch(tasks.select.selectedKey, (key) => { list.selectedKey = key; }, { signal: own });
    tasks.items.watch(tasks.select.summary, (text) => { summary.value = text; }, { signal: own });
  } catch (error) { failed.abort(); throw error; }
}
