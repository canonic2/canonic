/** Size controls receive a workflow owner and a connection lifetime. */
export function bindSizes({ switcher, add, editor }, owner, { signal, report }) {
  if (signal.aborted) return;
  const failed = new AbortController(),
    own = AbortSignal.any([signal, failed.signal]);
  function present(error) {
    if (own.aborted) return globalThis.reportError(error);
    try {
      report(error);
    } catch (failure) {
      globalThis.reportError(failure);
    }
  }
  function act(run) {
    try {
      Promise.resolve(run()).catch(present);
    } catch (error) {
      present(error);
    }
  }
  own.addEventListener(
    'abort',
    () => {
      add.open = false;
      editor.open = false;
    },
    { once: true },
  );
  try {
    owner.state.watch(
      owner.select.add,
      (value) => {
        add.allowPageOnly = value.allowPageOnly;
        add.notice = value.notice;
        add.error = value.error;
        add.pending = value.pending;
        add.open = value.open;
      },
      { signal: own },
    );
    let sizes;
    owner.state.watch(
      owner.select.edit,
      (value) => {
        if (sizes !== value.sizes) {
          sizes = value.sizes;
          editor.sizes = sizes;
        }
        editor.error = value.error;
        editor.pending = value.pending;
        editor.open = value.open;
      },
      { signal: own },
    );
    switcher.addEventListener(
      'wb-size-pick',
      (event) => act(() => owner.choose(event.detail.key)),
      { signal: own },
    );
    switcher.addEventListener('wb-size-custom', () => act(owner.openAdd), { signal: own });
    switcher.addEventListener('wb-size-edit', () => act(owner.openEdit), { signal: own });
    add.addEventListener('wb-size-add', (event) => act(() => owner.add(event.detail)), {
      signal: own,
    });
    add.addEventListener('wb-size-dialog-close', owner.closeAdd, { signal: own });
    editor.addEventListener('wb-sizes-save', (event) => act(() => owner.save(event.detail.sizes)), {
      signal: own,
    });
    editor.addEventListener('wb-sizes-close', owner.closeEdit, { signal: own });
  } catch (error) {
    failed.abort();
    throw error;
  }
}
