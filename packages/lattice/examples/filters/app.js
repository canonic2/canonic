/**
 * @file The example's entry: defines its elements, creates the capability,
 * mounts a group of fields and a summary, binds them, and tears everything down
 * when the given signal aborts.
 */
import { bindFilterField, bindFilterSummary } from './bindings.js';
import { AcmeFilterField, defineAcmeFilterField } from './filter-field.js';
import { AcmeFilterSummary, defineAcmeFilterSummary } from './filter-summary.js';
import { createFilters } from './filters.js';

/**
 * @typedef {object} MountOptions
 * @property {AbortSignal} signal Tears the example down when it aborts.
 * @property {number} [count] How many fields to mount. Defaults to 3.
 * @property {(query: string) => Promise<void> | void} [save] Persists the
 *   query. Defaults to doing nothing.
 * @property {(error: unknown) => void} [report] Presents failed actions.
 *   Defaults to the platform's `reportError`.
 */

/**
 * Mounts the example into a container until `signal` aborts.
 * @param {HTMLElement} container Where the fields and summary are appended.
 * @param {MountOptions} options Lifetime and ports.
 * @returns {import('./filters.js').Filters} The capability, for commands and tests.
 */
export function mountFilters(container, { signal, count = 3, save = () => {}, report = reportError }) {
  defineAcmeFilterField();
  defineAcmeFilterSummary();
  const filters = createFilters({ save });
  if (signal.aborted) return filters;

  const summary = new AcmeFilterSummary();
  const fields = Array.from({ length: count }, (_, index) => {
    const field = new AcmeFilterField();
    field.setAttribute('label', `Filter ${index + 1}`);
    return field;
  });
  const lifetime = { signal, report };
  for (const field of fields) bindFilterField(field, filters, lifetime);
  bindFilterSummary(summary, filters, lifetime);
  container.append(summary, ...fields);
  signal.addEventListener('abort', () => {
    summary.remove();
    for (const field of fields) field.remove();
  }, { once: true });
  return filters;
}
