/**
 * @file Bindings between the filters capability and its elements, written by
 * hand on `state.watch` and `addEventListener`, following the bindings
 * contract. Each binding lasts until its signal aborts.
 */

/**
 * @typedef {object} Lifetime
 * @property {AbortSignal} signal Ends the binding when it aborts.
 * @property {(error: unknown) => void} report Presents a failed action;
 *   synchronous and non-throwing.
 */

/**
 * Connects a filter field to the filters capability.
 * @param {import('./filter-field.js').AcmeFilterField} element The field it drives.
 * @param {import('./filters.js').Filters} filters The capability it shows and changes.
 * @param {Lifetime} lifetime The binding's lifetime and failure presenter.
 * @returns {void}
 */
export function bindFilterField(element, filters, { signal, report }) {
  if (signal.aborted) return;
  const failed = new AbortController();
  const own = AbortSignal.any([signal, failed.signal]);

  /**
   * Presents a failure while the binding is live; otherwise hands it to the platform.
   * @param {unknown} error The action's failure.
   * @returns {void}
   */
  const present = (error) => {
    if (own.aborted) return reportError(error);
    try {
      report(error);
    } catch (reportFailure) {
      reportError(reportFailure);
    }
  };

  const { state, select } = filters;
  try {
    state.watch(select.query, (query) => (element.value = query), { signal: own });
    state.watch(select.locked, (locked) => (element.disabled = locked), { signal: own });
  } catch (error) {
    failed.abort();
    throw error;
  }
  element.addEventListener(
    'acme-filter-input',
    (event) => {
      try {
        const result = filters.setQuery(
          /** @type {CustomEvent<{ value: string }>} */ (event).detail.value,
        );
        if (typeof result?.then === 'function') Promise.resolve(result).then(undefined, present);
      } catch (error) {
        present(error);
      }
    },
    { signal: own },
  );
}

/**
 * Connects a summary to the filters capability.
 * @param {import('./filter-summary.js').AcmeFilterSummary} element The summary it drives.
 * @param {import('./filters.js').Filters} filters The capability it shows.
 * @param {Lifetime} lifetime The binding's lifetime.
 * @returns {void}
 */
export function bindFilterSummary(element, filters, { signal }) {
  if (signal.aborted) return;
  filters.state.watch(filters.select.summary, (text) => (element.text = text), { signal });
}
