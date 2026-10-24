/**
 * @file Narrow helpers for native custom elements: capturing and replaying
 * inputs assigned before upgrade, and idempotent registration. Importing this
 * module defines no element and touches no DOM API.
 */

/**
 * Moves inputs assigned before upgrade off the host into a new map, so that the
 * element's own accessors are reached afterwards. Only the declared names are
 * checked; no setter is called and no attribute is written, so it is safe to
 * call from a constructor.
 * @template {HTMLElement} T
 * @param {T} host The element being constructed.
 * @param {readonly (keyof T & string)[]} names The element's declared inputs.
 * @returns {Map<keyof T & string, unknown> | null} The captured values by
 *   input name, or `null` when none was assigned before upgrade.
 */
export function captureInputs(host, names) {
  /** @type {Map<keyof T & string, unknown> | null} */
  let pending = null;
  for (const name of names) {
    if (!Object.hasOwn(host, name)) continue;
    pending ??= new Map();
    pending.set(name, Reflect.get(host, name));
    Reflect.deleteProperty(host, name);
  }
  return pending;
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
 */
export function replayInputs(host, pending) {
  for (const [name, value] of pending) Reflect.set(host, name, value);
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
