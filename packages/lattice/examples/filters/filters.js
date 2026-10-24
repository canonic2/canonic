/**
 * @file The filters capability: one query shared by a group of filter fields,
 * a lock that disables them, and a revision no binding reads. Pure: no DOM.
 */
import { createStore } from '@canonic2/lattice';

/**
 * @typedef {object} FiltersState
 * @property {string} query The text every field shows.
 * @property {boolean} locked Whether the fields are disabled.
 * @property {number} revision Counts saves; no binding reads it.
 */

/**
 * @typedef {object} Filters
 * @property {Pick<import('@canonic2/lattice').Store<FiltersState>, 'get' | 'watch'>} state Read-only view.
 * @property {FilterSelections} select The capability's selections.
 * @property {(query: string) => Promise<void> | void} setQuery Sets the query, then saves it.
 * @property {() => void} toggleLocked Locks or unlocks every field.
 * @property {() => void} touch Changes a field no binding reads.
 */

/**
 * @typedef {object} FilterSelections
 * @property {import('@canonic2/lattice').Selection<FiltersState, string>} query The query.
 * @property {import('@canonic2/lattice').Selection<FiltersState, boolean>} locked Whether fields are disabled.
 * @property {import('@canonic2/lattice').Selection<FiltersState, string>} summary Text describing the filter.
 */

/**
 * Creates the filters capability.
 * @param {object} ports Injected I/O.
 * @param {(query: string) => Promise<void> | void} ports.save Persists the query.
 * @returns {Filters} Its state view, selections and actions.
 */
export function createFilters({ save }) {
  const store = createStore({ query: '', locked: false, revision: 0 });
  return {
    state: { get: store.get, watch: store.watch },
    select: {
      query: { fields: ['query'], read: (state) => state.query },
      locked: { fields: ['locked'], read: (state) => state.locked },
      summary: {
        fields: ['query'],
        read: (state) => (state.query === '' ? 'All items' : `Matching “${state.query}”`),
      },
    },
    setQuery(query) {
      store.set({ query });
      return save(query);
    },
    toggleLocked() {
      store.set((state) => ({ locked: !state.locked }));
    },
    touch() {
      store.set((state) => ({ revision: state.revision + 1 }));
    },
  };
}
