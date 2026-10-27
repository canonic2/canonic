/** @file Observes invalid Promise results before rejecting asynchronous aggregate callbacks. */
const nativeThen = Promise.prototype.then;

/** @returns {undefined} Discards either settlement without throwing or propagating its value. */
function discard() {
  return undefined;
}

/**
 * Validates a computed result without awaiting/assimilating arbitrary thenables.
 * The intrinsic operation acts as a cross-realm Promise brand probe; normal
 * non-Promise objects fail that probe before any user `then` is read or called.
 * Primitive count/sum values bypass object probing entirely.
 * @param {unknown} value Callback result.
 * @param {string} callback Callback name.
 * @returns {unknown} The original synchronous value only.
 */
export function synchronousResult(value, callback) {
  if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return value;
  let promise = false;
  try {
    Reflect.apply(nativeThen, value, [discard, discard]);
    promise = true;
  } catch (error) {
    if (value instanceof Promise) throw error;
  }
  if (promise)
    throw new TypeError(`Aggregate ${callback} must return a synchronous value, not a Promise.`);
  if (typeof (/** @type {{ then?: unknown }} */ (value).then) === 'function')
    throw new TypeError(`Aggregate ${callback} must return a synchronous value, not a thenable.`);
  return value;
}
