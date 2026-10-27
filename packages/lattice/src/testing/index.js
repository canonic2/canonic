/**
 * @file Test support behind `@canonic2/lattice/testing`: record and collection
 * selection declaration checks. It imports no test framework and no runtime module, and
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

/**
 * Checks that a selection's `read` accesses only the fields it declares, by
 * calling `read` with each supplied snapshot and recording what it touches.
 * Property reads, including destructuring, and `in` tests count as access.
 * Enumerating the state's keys, reading property descriptors and writing to
 * the state are unsupported and fail the check.
 *
 * Accepts store and item selections, which share the same record shape. The
 * check covers only branches the snapshots reach; supply cases for each branch.
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
  return checkCases(name, selection.fields, cases, (snapshot, track) =>
    selection.read(/** @type {S} */ (track(/** @type {object} */ (snapshot)))),
  );
}

/**
 * Checks item-field dependencies of a collection-wide selection. Cases supply
 * traversal views; only `keys`, `get`, `has` and `size` reach the reader.
 * Membership reads need no field declaration. Item reads follow the same
 * rules as `checkSelection`; caught refusals still fail the check. Only the
 * branches reached by the supplied cases are checked, and nested data is not
 * instrumented.
 * @template T, V
 * @param {string} name The selection's diagnostic name.
 * @param {import('../modules/collection/collection.js').CollectionSelection<T, V>} selection The collection-wide selection to check.
 * @param {Readonly<Record<string, import('../modules/collection/collection.js').CollectionView<T>>>} cases Named traversal fixtures.
 * @returns {SelectionCheck} Declared, accessed and unobserved item fields.
 * @throws {Error} For unknown or undeclared fields, unsupported access, read
 *   failures or empty cases; failures name the selection and case.
 */
export function checkCollectionSelection(name, selection, cases) {
  return checkCases(name, selection.fields, cases, (source, track, refuse) => {
    /** @type {WeakMap<readonly string[], readonly string[]>} */
    const keyViews = new WeakMap();
    /** @type {import('../modules/collection/collection.js').CollectionView<T>} */
    const view = {
      /** @returns {readonly string[]} Instrumented ordered keys. */
      keys() {
        const keys = source.keys();
        let result = keyViews.get(keys);
        if (result === undefined) {
          result = new Proxy(keys, {
            set: () => refuse('writes to the collection keys'),
            defineProperty: () => refuse('writes to the collection keys'),
            deleteProperty: () => refuse('writes to the collection keys'),
            setPrototypeOf: () => refuse('writes to the collection keys'),
            preventExtensions: () => refuse('changes the collection keys'),
          });
          keyViews.set(keys, result);
        }
        return result;
      },
      /**
       * @param {string} key Item key.
       * @returns {Readonly<T> | undefined} Instrumented record.
       */
      get(key) {
        const item = source.get(key);
        return item === undefined
          ? undefined
          : /** @type {Readonly<T>} */ (track(/** @type {object} */ (item)));
      },
      /**
       * @param {string} key Item key.
       * @returns {boolean} Fixture membership.
       */
      has: (key) => source.has(key),
      /** @returns {number} Current fixture membership count. */
      get size() {
        return source.size;
      },
    };
    return selection.read(
      new Proxy(view, {
        set: () => refuse('writes to the collection view'),
        defineProperty: () => refuse('writes to the collection view'),
        deleteProperty: () => refuse('writes to the collection view'),
        setPrototypeOf: () => refuse('writes to the collection view'),
        preventExtensions: () => refuse('changes the collection view'),
      }),
    );
  });
}

/**
 * Runs either kind of selection with per-case record access tracking and the
 * same deterministic diagnostic report.
 * @template S, V
 * @param {string} name The diagnostic name.
 * @param {readonly string[]} fields Declared record fields.
 * @param {Readonly<Record<string, S>>} cases Named sources.
 * @param {(source: S, track: (record: object) => object, refuse: (operation: string) => never) => V} read Invokes the selection through its appropriate view.
 * @returns {SelectionCheck} The dependency report.
 * @throws {Error} When a case fails or no cases were supplied.
 */
function checkCases(name, fields, cases, read) {
  /** @type {string[]} */
  const declared = [...fields];
  const caseNames = Object.keys(cases);
  if (caseNames.length === 0) {
    throw new Error(`Selection "${name}": the check needs at least one snapshot case.`);
  }
  /**
   * Names the selection, the case and the declared fields for a message.
   * @param {string} caseName The snapshot case.
   * @returns {string} The message prefix.
   */
  const context = (caseName) =>
    `Selection "${name}", case "${caseName}" (declares ${declared.join(', ')})`;
  /** @type {Set<string>} */
  const union = new Set();
  /** @type {SelectionCase[]} */
  const checked = [];

  for (const caseName of caseNames) {
    const source = /** @type {S} */ (cases[caseName]);
    /** @type {WeakMap<object, object>} */
    const records = new WeakMap();
    /** @type {string[]} */
    const seen = [];
    /** @type {string[]} */
    const unknown = [];
    /** @type {string | null} */
    let unsupported = null;

    /**
     * Records a property key `read` touched, as a field or an unknown key.
     * @param {object} snapshot The record being read.
     * @param {string | symbol} key The key read or tested.
     * @returns {void}
     */
    const record = (snapshot, key) => {
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
      throw new Error(operation);
    };

    /**
     * Wraps a record once, preserving identity for repeated reads in this case.
     * @param {object} snapshot The state or item record.
     * @returns {object} Its instrumented view.
     */
    const track = (snapshot) => {
      const existing = records.get(snapshot);
      if (existing !== undefined) return existing;
      const result = new Proxy(snapshot, {
        get(target, key, receiver) {
          record(target, key);
          return Reflect.get(target, key, receiver);
        },
        has(target, key) {
          record(target, key);
          return Reflect.has(target, key);
        },
        ownKeys: () => refuse('enumerates the state’s keys, as Object.keys, for…in or spread do'),
        getOwnPropertyDescriptor: () => refuse('reads a property descriptor'),
        set: () => refuse('writes to the state'),
        defineProperty: () => refuse('writes to the state'),
        deleteProperty: () => refuse('writes to the state'),
        setPrototypeOf: () => refuse('writes to the state'),
        preventExtensions: () => refuse('changes the state'),
      });
      records.set(snapshot, result);
      return result;
    };

    try {
      read(source, track, refuse);
    } catch (error) {
      if (unsupported === null) {
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
