/**
 * @file Lattice's root entry point: the pure store core. It loads no DOM code,
 * and importing it performs no registration, DOM access, I/O or background work.
 */

export { createStore } from './modules/store/store.js';
export { shallow } from './modules/store/shallow.js';

/**
 * A selection over a store's state; see the store module.
 * @template S The store's state.
 * @template V The selected value.
 * @typedef {import('./modules/store/store.js').Selection<S, V>} Selection
 */

/**
 * A store; see `createStore`.
 * @template S The state.
 * @typedef {import('./modules/store/store.js').Store<S>} Store
 */

/** @typedef {import('./modules/store/store.js').StoreOptions} StoreOptions */

/** @typedef {import('./modules/store/store.js').WatchOptions} WatchOptions */

/**
 * Read-and-subscribe access to a store.
 * @template S The state.
 * @typedef {import('./modules/store/store.js').StoreReader<S>} StoreReader
 */
