/**
 * @file Callback error handling shared by the store and the collection: the
 * last-resort reporter, which is the default for their `report` option and the
 * path for errors their `report` throws, and the check that rejects an
 * asynchronous batch callback. Pure; importing it has no effects.
 */

/**
 * Reports an error through the platform's `reportError` when it exists, and
 * otherwise, or when that throws, rethrows it from a microtask so it surfaces
 * as uncaught. Never throws itself.
 * @param {unknown} error The error to report.
 * @returns {void}
 */
export function fallback(error) {
  if (typeof reportError === 'function') {
    try {
      reportError(error);
      return;
    } catch {
      // The platform reporter failed; surface the original error instead.
    }
  }
  queueMicrotask(() => {
    throw error;
  });
}

/**
 * Tells whether a value has a `then` method, as promises and thenables do.
 * Reading `then` may throw; callers run this where a throw is handled.
 * @param {unknown} value The value a batch callback returned.
 * @returns {boolean} Whether it is a thenable.
 */
export function isThenable(value) {
  return (
    value !== null &&
    (typeof value === 'object' || typeof value === 'function') &&
    typeof (/** @type {{ then?: unknown }} */ (value).then) === 'function'
  );
}
