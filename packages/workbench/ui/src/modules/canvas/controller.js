import { add, arrange, change, createState, remove, select, size, supportsSize } from './model.js';
import {
  dimensions as sizeDimensions,
  validDimensions,
} from '../../../../contracts/src/sizes/browser/geometry.js';
import { EXPORT_FILL } from '../../../../contracts/src/sizes/browser/size.js';

export function createController(ports) {
  let state = createState(crypto.randomUUID());
  const runtimes = new Map();
  const parked = new Map();
  let busy = false;
  let disposed = false;
  const notify = () => {
    if (ports.rowWidth) state = arrange(state, ports.rowWidth);
    ports.render(state);
  };
  function live() {
    if (disposed) throw new Error('Canvas closed.');
  }
  function choose(id) {
    live();
    state = select(state, id);
    notify();
  }
  function create(target, dimensions) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const previous = state;
    state = add(state, crypto.randomUUID(), target, dimensions);
    const board = state.artboards.at(-1);
    try {
      mount(board);
    } catch (error) {
      state = previous;
      notify();
      throw error;
    }
    return board.id;
  }
  function mount(board) {
    const runtime = ports.mount(board, {
      view(info) {
        const current = state.artboards.find((b) => b.id === board.id);
        if (disposed || !current || current.generation !== board.generation) return;
        const parsed = {
          ...current.target,
          src: info.src,
          state: info.state,
          lens: info.lens,
          label: info.label,
        };
        /* A page that can't be shown at the artboard's size takes its first size, a filled axis at the canvas's length now. */
        const first = info.sizes?.[0];
        let dimensions =
          supportsSize(current.size, info.sizes) || !first
            ? current.size
            : sizeDimensions(first, ports.available?.() || EXPORT_FILL, current.size);
        if (
          !busy &&
          info.ready &&
          validDimensions(info.contentSize) &&
          supportsSize(info.contentSize, info.sizes)
        )
          dimensions = size(info.contentSize.width, info.contentSize.height);
        state = change(state, board.id, { view: info, target: parsed, size: dimensions });
        notify();
        if (dimensions.width !== current.size.width || dimensions.height !== current.size.height)
          void runtime.request('size', dimensions).catch(ports.error);
      },
      focus() {
        if (!state.artboards.some((b) => b.id === board.id && b.generation === board.generation))
          return;
        ports.activity?.();
        if (state.selected !== board.id) choose(board.id);
      },
      host(data) {
        if (state.artboards.some((b) => b.id === board.id && b.generation === board.generation))
          ports.host(board, data);
      },
    });
    runtimes.set(board.id, runtime);
    notify();
    runtime.element.addEventListener(
      'load',
      () => {
        const current = state.artboards.find((b) => b.id === board.id);
        if (disposed || !current || current.generation !== board.generation) return;
        void runtime.request('size', current.size).catch(ports.error);
      },
      { once: true },
    );
  }
  function replace(id, target, dimensions) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const previous = state.artboards.find((b) => b.id === id);
    if (!previous) throw new Error('Artboard closed.');
    const nextSize = dimensions ? size(dimensions.width, dimensions.height) : previous.size;
    runtimes.get(id)?.dispose();
    runtimes.delete(id);
    state = change(state, id, {
      target,
      view: null,
      generation: previous.generation + 1,
      size: nextSize,
    });
    mount(state.artboards.find((b) => b.id === id));
  }
  async function command(id, name, value) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const runtime = runtimes.get(id);
    if (!runtime) throw new Error('Artboard closed.');
    if (name === 'size') {
      const input = value;
      const dimensions = size(input.width, input.height);
      if (!supportsSize(dimensions, state.artboards.find((b) => b.id === id)?.view?.sizes))
        throw new Error('This page does not support that artboard size.');
      state = change(state, id, { size: dimensions });
      notify();
      value = dimensions;
    }
    await runtime.request(name, value);
  }
  function close(id) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    runtimes.get(id)?.dispose();
    runtimes.delete(id);
    parked.delete(id);
    state = remove(state, id);
    notify();
  }
  function park(id) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const board = state.artboards.find((b) => b.id === id);
    if (!board) throw new Error('Artboard closed.');
    parked.set(id, board);
    state = remove(state, id);
    notify();
  }
  function resume(id) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const board = parked.get(id);
    if (!board) throw new Error('Artboard is not parked.');
    state = change(add(state, id, board.target, board.size), id, {
      view: board.view,
      generation: board.generation,
    });
    parked.delete(id);
    notify();
  }
  return {
    snapshot: () => state,
    create,
    replace,
    choose,
    command,
    close,
    park,
    resume,
    runtime: (id) => runtimes.get(id),
    async capture(run) {
      live();
      if (busy) throw new Error('A canvas capture is already running.');
      busy = true;
      const snapshot = structuredClone(state);
      const locked = snapshot.artboards.filter((b) => b.view?.ready).map((b) => runtimes.get(b.id));
      try {
        const results = await Promise.allSettled(locked.map((r) => r.request('lock')));
        const failure = results.find((r) => r.status === 'rejected');
        if (failure?.status === 'rejected') throw failure.reason;
        return await run(snapshot, (id) => {
          const r = runtimes.get(id);
          if (!r) throw new Error('Artboard closed.');
          return r;
        });
      } finally {
        await Promise.allSettled(locked.map((r) => r.request('unlock')));
        busy = false;
        // Unlock can report newly laid-out content before the response arrives.
        // Apply those deferred bounds after releasing the immutable capture.
        for (const board of state.artboards) {
          const measured = board.view?.contentSize;
          if (
            !disposed &&
            board.view?.ready &&
            validDimensions(measured) &&
            supportsSize(measured, board.view.sizes) &&
            (measured.width !== board.size.width || measured.height !== board.size.height)
          ) {
            state = change(state, board.id, { size: size(measured.width, measured.height) });
            notify();
            void runtimes.get(board.id)?.request('size', measured).catch(ports.error);
          }
        }
      }
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      for (const runtime of runtimes.values()) runtime.dispose();
      runtimes.clear();
      parked.clear();
    },
  };
}
