/**
 * @file Narrow helpers for native custom elements: capturing and replaying
 * inputs assigned before upgrade, recognizing the upgrade's own attribute
 * callbacks, and idempotent registration. Importing this module defines no
 * element and touches no DOM API.
 */

/**
 * For each map `captureInputs` returned, the attributes that were present when
 * their input was captured: attribute name to input name. Each entry stands
 * for one initial upgrade callback not yet seen.
 * @type {WeakMap<Map<string, unknown>, Map<string, string>>}
 */
const upgradeAttributes = new WeakMap();

/**
 * Moves inputs assigned before upgrade off the host into a new map, so that the
 * element's own accessors are reached afterwards. Only the declared names are
 * checked; no setter is called and no attribute is written, so it is safe to
 * call from a constructor.
 *
 * For a captured input listed in `attributes`, it also records whether its
 * attribute is present, so that `skipUpgradeAttribute` can recognize the
 * upgrade's initial callback for it. That presence check departs deliberately
 * from the HTML Standard's requirement that constructors not inspect
 * attributes; it is made only for inputs both captured and reflected, and the
 * attribute's value is never read.
 * @template {HTMLElement} T
 * @template {keyof T & string} K
 * @param {T} host The element being constructed.
 * @param {readonly K[]} names The element's declared inputs.
 * @param {Partial<Record<K, string>>} [attributes] Attribute name of each of
 *   those inputs that has one.
 * @returns {Map<K, unknown> | null} The captured values by input name, or
 *   `null` when none was assigned before upgrade.
 * @throws {TypeError} When an early property cannot be deleted, for example
 *   because it was defined as non-configurable; it would keep hiding the
 *   element's accessor.
 */
export function captureInputs(host, names, attributes = {}) {
  /** @type {Map<K, unknown> | null} */
  let pending = null;
  /** @type {Map<string, string> | null} */
  let present = null;
  for (const name of names) {
    if (!Object.hasOwn(host, name)) continue;
    const value = Reflect.get(host, name);
    if (!Reflect.deleteProperty(host, name)) {
      throw new TypeError(
        `Cannot capture input "${name}": its own property cannot be deleted and hides the element's accessor.`,
      );
    }
    pending ??= new Map();
    pending.set(name, value);
    const attribute = attributes[name];
    if (attribute !== undefined && host.hasAttribute(attribute)) {
      (present ??= new Map()).set(attribute, name);
    }
  }
  if (pending !== null && present !== null) upgradeAttributes.set(pending, present);
  return pending;
}

/**
 * Tells whether an attribute callback is the upgrade's initial one for an
 * input captured as a property, which the element skips so that the property
 * wins. It returns `true` at most once per such attribute; every later
 * callback returns `false` and should be applied, superseding the capture.
 * @param {Map<string, unknown> | null} pending The element's pending map, or
 *   `null` when nothing is pending.
 * @param {string} attribute The attribute name the callback received.
 * @returns {boolean} Whether the element should ignore this callback.
 */
export function skipUpgradeAttribute(pending, attribute) {
  if (pending === null) return false;
  const present = upgradeAttributes.get(pending);
  const name = present?.get(attribute);
  if (present === undefined || name === undefined) return false;
  present.delete(attribute);
  return pending.has(name);
}

/**
 * Assigns each captured value still in the map through the host's setter, in
 * capture order. Setters remove their own entry before comparing, so an entry
 * removed by a newer assignment, before or during the replay, is skipped. The
 * element queues the microtask that calls this and clears its reference to the
 * map afterwards.
 * @template {HTMLElement} T
 * @param {T} host The upgraded element.
 * @param {Map<keyof T & string, unknown>} pending The map `captureInputs` returned.
 * @returns {void}
 * @throws {TypeError} When an assignment fails, for example because the input
 *   has no setter. Entries after the failing one are not applied.
 */
export function replayInputs(host, pending) {
  for (const [name, value] of pending) {
    if (!Reflect.set(host, name, value)) {
      throw new TypeError(`Cannot replay input "${name}": the element did not accept the assignment.`);
    }
  }
}

/**
 * Defines a constructor under a tag name, doing nothing when that constructor
 * is already defined there.
 * @param {CustomElementRegistry} registry The registry to define it in.
 * @param {string} name The tag name.
 * @param {CustomElementConstructor} constructor The element class.
 * @returns {void}
 * @throws {Error} When another constructor is already defined under `name`.
 *   Errors raised by the registry itself, such as for an invalid name, propagate.
 */
export function defineOnce(registry, name, constructor) {
  const existing = registry.get(name);
  if (existing === constructor) return;
  if (existing !== undefined) {
    throw new Error(`<${name}> is already defined by another constructor.`);
  }
  registry.define(name, constructor);
}
