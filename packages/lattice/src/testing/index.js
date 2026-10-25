/**
 * @file Test support behind `@canonic2/lattice/testing`: the selection
 * declaration check. It imports no test framework and no runtime module, and
 * reports a failed check by throwing an `Error`. Importing it has no effects.
 */

/**
 * One checked snapshot.
 * @typedef {object} SelectionCase
 * @property {string} name The case's name, its key in `cases`.
 * @property {readonly string[]} accessed The fields `read` accessed with this
 *   snapshot, in the selection's declaration order.
 */

/**
 * The result of a check that found no undeclared or unsupported access. Every
 * list has a deterministic order, so assertions and messages stay stable.
 * @typedef {object} SelectionCheck
 * @property {string} name The selection's name, as the test gave it.
 * @property {readonly string[]} declared The fields the selection declares, in
 *   declaration order.
 * @property {readonly SelectionCase[]} cases Each case, in the own-key order of
 *   `cases`, with the fields it accessed.
 * @property {readonly string[]} accessed The union of the fields accessed
 *   across all cases, in declaration order.
 * @property {readonly string[]} unobserved Declared fields absent from that
 *   union, in declaration order: a diagnostic, not a failure. It points at a
 *   branch no case reached or at an unnecessary dependency.
 */

/** Marks the error thrown into `read` when it uses an unsupported operation. */
class UnsupportedAccess extends Error {}

/**
 * Checks that a selection's `read` accesses only the fields it declares, by
 * calling `read` with each supplied snapshot and recording what it touches.
 * Property reads, including destructuring, and `in` tests count as access.
 * Enumerating the state's keys, reading property descriptors and writing to
 * the state are unsupported and fail the check.
 *
 * The check covers only the branches the snapshots reach; a test supplies
 * snapshots that reach each branch of `read`.
 * @template S, V
 * @param {string} name A name for the selection, used in every message.
 * @param {import('../modules/store/store.js').Selection<S, V>} selection The selection to check.
 * @param {Readonly<Record<string, S>>} cases Named snapshots to call `read` with.
 * @returns {SelectionCheck} What was accessed and which declared fields were not.
 * @throws {Error} When `read` accesses an undeclared or unknown field, uses an
 *   unsupported operation, or throws, naming the selection, the first failing
 *   case in `cases` order, the declared fields and the unexpected fields,
 *   sorted alphabetically; or when no case is supplied.
 */
export function checkSelection(name, selection, cases) {
  /** @type {string[]} */
  const declared = [...selection.fields];
  const caseNames = Object.keys(cases);
  if (caseNames.length === 0) {
    throw new Error(`Selection "${name}": the check needs at least one snapshot case.`);
  }
  /**
   * Names the selection, the case and the declared fields for a message.
   * @param {string} caseName The snapshot case.
   * @returns {string} The message prefix.
   */
  const context = (caseName) => `Selection "${name}", case "${caseName}" (declares ${declared.join(', ')})`;
  /** @type {Set<string>} */
  const union = new Set();
  /** @type {SelectionCase[]} */
  const checked = [];

  for (const caseName of caseNames) {
    const snapshot = /** @type {object} */ (cases[caseName]);
    /** @type {string[]} */
    const seen = [];
    /** @type {string[]} */
    const unknown = [];
    /** @type {string | null} */
    let unsupported = null;

    /**
     * Records a property key `read` touched, as a field or an unknown key.
     * @param {string | symbol} key The key read or tested.
     * @returns {void}
     */
    const record = (key) => {
      if (typeof key === 'symbol') return;
      if (Object.hasOwn(snapshot, key)) {
        if (!seen.includes(key)) seen.push(key);
      } else if (!(key in snapshot) && !unknown.includes(key)) {
        unknown.push(key);
      }
    };
    /**
     * Records an unsupported operation and stops `read`.
     * @param {string} operation What `read` tried to do.
     * @returns {never} Never returns; it throws into `read`.
     */
    const refuse = (operation) => {
      unsupported ??= operation;
      throw new UnsupportedAccess(operation);
    };

    const state = new Proxy(snapshot, {
      get(target, key, receiver) {
        record(key);
        return Reflect.get(target, key, receiver);
      },
      has(target, key) {
        record(key);
        return Reflect.has(target, key);
      },
      ownKeys: () => refuse('enumerates the state’s keys, as Object.keys, for…in or spread do'),
      getOwnPropertyDescriptor: () => refuse('reads a property descriptor'),
      set: () => refuse('writes to the state'),
      defineProperty: () => refuse('writes to the state'),
      deleteProperty: () => refuse('writes to the state'),
    });

    try {
      selection.read(/** @type {S} */ (state));
    } catch (error) {
      if (unsupported === null && !(error instanceof UnsupportedAccess)) {
        throw new Error(`${context(caseName)}: read threw.`, { cause: error });
      }
    }
    if (unsupported !== null) {
      throw new Error(`${context(caseName)}: read ${unsupported}, which the check cannot follow.`);
    }
    const undeclared = [...seen.filter((field) => !declared.includes(field)), ...unknown].sort();
    if (undeclared.length > 0) {
      throw new Error(
        `${context(caseName)}: read accessed ${undeclared.length === 1 ? 'an undeclared field' : 'undeclared fields'}: ${undeclared.join(', ')}.`,
      );
    }
    for (const field of seen) union.add(field);
    checked.push({ name: caseName, accessed: declared.filter((field) => seen.includes(field)) });
  }

  return {
    name,
    declared,
    cases: checked,
    accessed: declared.filter((field) => union.has(field)),
    unobserved: declared.filter((field) => !union.has(field)),
  };
}
