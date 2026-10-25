import { createStore } from '@canonic2/lattice';

/** Transient inputs and reported actions, isolated by owning renderer. */
export function createPreviewControls({ send }) {
  const store = createStore({ instances: {}, selected: null, opened: false });
  const entry = (state) => state.instances[state.selected] || null;
  function update(id, value) {
    store.update((state) => ({ instances: { ...state.instances, [id]: value } }));
  }
  function selected() {
    const state = store.get();
    return [state.selected, entry(state)];
  }
  return {
    state: { get: store.get, watch: store.watch },
    select: {
      model: { fields: ['instances', 'selected'], read: entry },
      available: { fields: ['instances', 'selected'], read: (state) => !!entry(state) },
      opened: { fields: ['opened'], read: (state) => state.opened },
    },
    choose(id) {
      if (store.get().selected !== id) store.update({ selected: id, opened: false });
    },
    receive(id, data) {
      if (data?.event === 'ready') {
        update(id, {
          instance: id,
          id: data.id,
          scenario: data.scenario,
          controls: data.controls || {},
          inputs: { ...data.inputs },
          actions: (data.actions || []).slice(-30),
          docs: data.docs || '',
        });
      } else if (data?.event === 'action') {
        const current = store.get().instances[id];
        if (!current || current.id !== data.id) return;
        update(id, {
          ...current,
          actions: [...current.actions, { name: data.name, values: data.values || [] }].slice(-30),
        });
      }
    },
    changeInput(name, value) {
      const [id, current] = selected();
      if (!current || !Object.hasOwn(current.controls, name)) return;
      const inputs = { ...current.inputs, [name]: value };
      update(id, { ...current, inputs });
      return send(id, { scenario: current.scenario, inputs });
    },
    resetScenario() {
      const [id, current] = selected();
      if (!current) return;
      update(id, { ...current, actions: [] });
      return send(id, { command: 'reset', scenario: current.scenario, inputs: {} });
    },
    toggle() {
      if (entry(store.get())) store.update((state) => ({ opened: !state.opened }));
    },
    deselect() {
      store.update({ selected: null, opened: false });
    },
    clear() {
      store.update({ instances: {}, selected: null, opened: false });
    },
  };
}
