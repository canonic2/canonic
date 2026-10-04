import { add, change, createState, remove, select, size, supportsSize } from './model.ts';
import type { Artboard, CanvasState, Size, Target, ViewInfo } from './model.ts';
import type { Runtime } from './runtime.ts';
export function createController(ports: {
  mount(board: Artboard, callbacks: { view(info: ViewInfo): void; focus(): void; host(data: unknown): void }): Runtime;
  render(state: CanvasState): void;
  host(board: Artboard, data: unknown): void;
  error(error: unknown): void;
}) {
  let state = createState(crypto.randomUUID());
  const runtimes = new Map<string, Runtime>();
  let busy = false;
  let disposed = false;
  const notify = () => ports.render(state);
  function live() { if (disposed) throw new Error('Canvas closed.'); }
  function choose(id: string) { live(); state = select(state, id); notify(); }
  function create(target: Target, dimensions: Size) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const previous = state;
    state = add(state, crypto.randomUUID(), target, dimensions);
    const board = state.artboards.at(-1)!;
    try { mount(board); } catch (error) { state = previous; notify(); throw error; }
    return board.id;
  }
  function mount(board: Artboard) {
    const runtime = ports.mount(board, {
      view(info) {
        const current = state.artboards.find(b => b.id === board.id); if (disposed || !current || current.generation !== board.generation) return;
        const parsed = { ...current.target, src: info.src, state: info.state, lens: info.lens };
        const dimensions = supportsSize(current.size, info.sizes) ? current.size : info.sizes?.includes('1512') ? { width: 1512, height: 982 } : { width: 393, height: 852 };
        state = change(state, board.id, { view: info, target: parsed, size: dimensions }); notify();
        if (dimensions.width !== current.size.width || dimensions.height !== current.size.height) void runtime.request('size', dimensions).catch(ports.error);
      }, focus() { if (state.selected !== board.id) choose(board.id); }, host(data) { ports.host(board, data); },
    });
    runtimes.set(board.id, runtime); notify();
    runtime.element.addEventListener('load', () => {
      const current = state.artboards.find(b => b.id === board.id);
      if (disposed || !current || current.generation !== board.generation) return;
      void runtime.request('size', current.size).catch(ports.error);
    }, { once: true });
  }
  function replace(id: string, target: Target) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const previous = state.artboards.find(b => b.id === id); if (!previous) throw new Error('Artboard closed.');
    runtimes.get(id)?.dispose(); runtimes.delete(id);
    state = change(state, id, { target, view: null, generation: previous.generation + 1 });
    mount(state.artboards.find(b => b.id === id)!);
  }
  async function command(id: string, name: string, value?: unknown) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    const runtime = runtimes.get(id); if (!runtime) throw new Error('Artboard closed.');
    if (name === 'size') {
      const input = value as Size; const dimensions = size(input.width, input.height);
      if (!supportsSize(dimensions, state.artboards.find(b => b.id === id)?.view?.sizes)) throw new Error('This page does not support that artboard size.');
      state = change(state, id, { size: dimensions }); notify(); value = dimensions;
    }
    await runtime.request(name, value);
  }
  function close(id: string) {
    live();
    if (busy) throw new Error('Wait for the canvas capture to finish.');
    runtimes.get(id)?.dispose(); runtimes.delete(id); state = remove(state, id); notify();
  }
  return { snapshot: () => state, create, replace, choose, command, close, runtime: (id: string) => runtimes.get(id),
    async capture<T>(run: (snapshot: CanvasState, get: (id: string) => Runtime) => Promise<T>): Promise<T> {
      live();
      if (busy) throw new Error('A canvas capture is already running.');
      busy = true;
      const snapshot = structuredClone(state);
      const locked = snapshot.artboards.filter(b => b.view?.ready).map(b => runtimes.get(b.id)!);
      try {
        const results = await Promise.allSettled(locked.map(r => r.request('lock')));
        const failure = results.find(r => r.status === 'rejected');
        if (failure?.status === 'rejected') throw failure.reason;
        return await run(snapshot, id => { const r = runtimes.get(id); if (!r) throw new Error('Artboard closed.'); return r; });
      } finally {
        await Promise.allSettled(locked.map(r => r.request('unlock'))); busy = false;
      }
    },
    dispose() { if (disposed) return; disposed = true; for (const runtime of runtimes.values()) runtime.dispose(); runtimes.clear(); },
  };
}
