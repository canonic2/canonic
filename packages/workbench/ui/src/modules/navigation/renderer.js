import { createStore } from '@canonic2/lattice';
import { initial, transition } from './state.js';

/** The renderer's request identity belongs to its own document lifetime. */
export function createRendererNavigation(id, destination, slot = '') {
  const store = createStore({ ...initial(), from: null });
  /** @type {import('@canonic2/lattice').StoreReader<ReturnType<typeof store.get>>} */
  const state = { get: store.get, watch: store.watch };
  const namespace = 'renderer:' + crypto.randomUUID() + ':';
  let serial = 0;
  function picked(next, incoming) {
    const previous = store.get().id;
    const id = incoming || namespace + ++serial;
    store.update({
      ...transition(store.get(), { type: 'picked', id, destination: next }),
      slots: [{ id: slot, status: 'pending', view: null, problem: null }],
      from: incoming ? null : previous,
    });
  }
  picked(destination, id);
  return {
    state,
    picked,
    resolved(destination) {
      store.update({ requested: destination });
    },
    identify(id) {
      if (!id || id === store.get().id) return;
      picked(store.get().requested, id);
    },
    report(view) {
      const current = store.get();
      store.update(
        transition(current, {
          type: view.problem ? 'failed' : 'ready',
          id: current.id,
          slot,
          view,
          problem: view.problem || undefined,
        }),
      );
      return { navigation: store.get().id, from: store.get().from };
    },
    identity: () => ({ navigation: store.get().id, from: store.get().from }),
  };
}
