import { createStore } from '@canonic2/lattice';

/** Size form workflows; canvas owns the applied dimensions and configuration owns writes. */
export function createSizeControls(ports) {
  const closed = {
    open: false,
    pending: false,
    error: '',
    allowPageOnly: false,
    notice: '',
    sizes: [],
  };
  const store = createStore({ add: closed, edit: closed });
  /** @type {import('@canonic2/lattice').StoreReader<ReturnType<typeof store.get>>} */
  const state = { get: store.get, watch: store.watch };
  let disposed = false;
  const update = (key, patch) => store.update((s) => ({ [key]: { ...s[key], ...patch } }));
  return {
    state,
    select: {
      add: { fields: ['add'], read: (s) => s.add },
      edit: { fields: ['edit'], read: (s) => s.edit },
    },
    choose: (key) => ports.choose(key),
    openAdd() {
      if (!disposed && !store.get().add.pending)
        update('add', { ...closed, ...ports.context(), open: true });
    },
    openEdit() {
      if (!disposed && !store.get().edit.pending)
        update('edit', { ...closed, sizes: ports.sizes(), open: true });
    },
    closeAdd() {
      if (!store.get().add.pending) update('add', { open: false });
    },
    closeEdit() {
      if (!store.get().edit.pending) update('edit', { open: false });
    },
    async add(detail) {
      if (disposed || store.get().add.pending) return;
      const context = ports.context();
      update('add', { pending: true, error: '' });
      try {
        const result = await ports.post({ action: 'add', size: { ...detail, page: context.page } });
        if (disposed) return;
        update('add', { pending: false, open: false });
        ports.added(result, detail, context.docs);
      } catch (error) {
        if (disposed) {
          ports.report(error);
          return;
        }
        update('add', { pending: false, error: String(error.message || error) });
      }
    },
    async save(sizes) {
      if (disposed || store.get().edit.pending) return;
      update('edit', { pending: true, error: '' });
      try {
        await ports.post({ action: 'update', sizes });
        if (disposed) return;
        update('edit', { pending: false, open: false });
        ports.updated();
      } catch (error) {
        if (disposed) {
          ports.report(error);
          return;
        }
        update('edit', { pending: false, error: String(error.message || error) });
      }
    },
    dispose() {
      disposed = true;
    },
  };
}
