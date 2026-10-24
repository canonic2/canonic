/** @file Shallow equality for selections whose `read` builds an object or array. */

/**
 * Compares two values one level deep. Equal under `Object.is`, or two arrays of
 * the same length whose items are `Object.is`-equal, or two objects with the
 * same prototype and the same own enumerable keys whose values are
 * `Object.is`-equal. Intended for plain objects and arrays; other objects,
 * such as maps, are compared only by their own enumerable keys.
 * @template T
 * @param {T} a One value.
 * @param {T} b The other value.
 * @returns {boolean} Whether they are shallowly equal.
 */
export function shallow(a, b) {
  if (Object.is(a, b)) return true;
  if (typeof a !== 'object' || a === null || typeof b !== 'object' || b === null) return false;
  if (Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
  if (Array.isArray(a)) {
    const other = /** @type {unknown[]} */ (/** @type {unknown} */ (b));
    if (a.length !== other.length) return false;
    for (let index = 0; index < a.length; index++) {
      if (!Object.is(a[index], other[index])) return false;
    }
    return true;
  }
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  for (const key of keys) {
    // Own and enumerable, matching what Object.keys(a) listed; a non-enumerable
    // key on b would otherwise make the comparison asymmetric.
    if (!Object.prototype.propertyIsEnumerable.call(b, key)) return false;
    if (!Object.is(Reflect.get(a, key), Reflect.get(b, key))) return false;
  }
  return true;
}
