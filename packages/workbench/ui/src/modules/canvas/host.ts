/** Canvas-only composition. The existing shell owns all chrome, configuration,
 * routing, sources and export; this module renders explicitly declared artboards. */
import { WbCanvas } from '../../components/canvas/element.ts';
import { createController } from '../../canvas/controller.ts';
import { createRuntime } from '../../canvas/runtime.ts';
import { capture, reference } from '../../canvas/review.ts';
import { createPublisher } from '../../canvas/publication.ts';
import { createStoryCatalog, fetchStoryCatalog } from './story-catalog.ts';
import { dimensions } from '../../../../contracts/src/sizes/browser/geometry.ts';
import type { Artboard, Size, Space } from '../../canvas/model.ts';
import type { ResolvedSize } from '../../../../contracts/src/sizes/browser/size.ts';

interface Selection {
  src: string; state: string | null; lens: string | null; example?: string | null;
  docs: boolean; size: ResolvedSize; actions: boolean;
  artboards?: { id: string; label: string; width: number; height: number }[];
  select(state: string | null, size: Size, actions?: boolean, src?: string, lens?: string | null): void;
}
interface ChromeWindow extends Window {
  wbMultiArtboardHost?: boolean;
  wbArtboardHeld?: boolean;
  wbMultiCanvas?: ReturnType<typeof mount>;
  wbZoom: { scale(): number; update(): void; fit(): void; hold(): void; key(event: KeyboardEvent): boolean; command(event: KeyboardEvent): string | null; mode(mode: string): void };
  wbPreviewControls: { connect(port: { send(value: unknown): void }): void; reset(): void; receive(value: unknown): void };
  wbReference: { copy(value: string): Promise<void> };
}
const chrome = window as unknown as ChromeWindow;
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
async function json(url: string, body?: unknown) {
  const response = await fetch(url, body === undefined ? undefined : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  const value = await response.json();
  if (!response.ok || value.ok === false) throw new Error(value.error || 'Workbench request failed.');
  return value;
}
export function initialize() {
  if (!chrome.wbMultiArtboardHost) return;
  chrome.wbMultiCanvas = mount();
  window.dispatchEvent(new Event('wb-multi-ready'));
}
function mount() {
  if (!customElements.get('wb-canvas')) customElements.define('wb-canvas', WbCanvas);
  const canvas = document.createElement('wb-canvas') as WbCanvas;
  canvas.id = 'artboards';
  canvas.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;display:none';
  $('canvas').append(canvas);
  const style = document.createElement('style');
  style.textContent = '.wb-canvas>#artboard,.wb-canvas>#artboardLabel{display:none!important}';
  document.head.append(style);
  const connection = new AbortController(), signal = connection.signal;
  let disposed = false, busy = false, expanding = false, group = '', sequence = 0;
  let selection: Selection | undefined, space: Space, registry: Space[] = [], activity = Date.now();
  let previewBoard = '', toastTimer: ReturnType<typeof setTimeout> | undefined;
  const previews = new Map<string, unknown>();
  const catalog = createStoryCatalog(fetchStoryCatalog);
  let parkedOrdinary: string | undefined, parkTimer: ReturnType<typeof setTimeout> | undefined;
  function expireOrdinary() {
    if (disposed || !parkedOrdinary) return;
    if (busy) { parkTimer = setTimeout(expireOrdinary, 1000); return; }
    controller.close(parkedOrdinary); parkedOrdinary = undefined;
  }
  const ready = json('/_workbench/spaces').then(async listed => {
    registry = listed.spaces;
    const resolved = await json('/_workbench/canvas/space', { id: listed.current, operation: 'config' });
    space = { ...registry.find(s => s.id === listed.current)!, url: resolved.url };
  });
  void ready.catch(say);
  function say(error: unknown) {
    $('toast').textContent = error instanceof Error ? error.message : String(error);
    $('toast').hidden = false; clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { $('toast').hidden = true; }, 5000);
  }
  function act(run: () => unknown) {
    try { const result = run(); if (result instanceof Promise) void result.catch(say); } catch (error) { say(error); }
  }
  const selected = () => controller.snapshot().artboards.find(b => b.id === controller.snapshot().selected);
  const available = () => ({ width: $('canvas').clientWidth, height: $('canvas').clientHeight });
  function command(name: string, value?: unknown) {
    const board = selected(); if (!board) return Promise.resolve();
    return controller.command(board.id, name, value);
  }
  function activityChanged() { activity = Date.now(); publisher.schedule(); }
  const controller = createController({
    mount(board, callbacks) {
      const runtime = createRuntime(board, { ...callbacks, stories: catalog.stories });
      runtime.element.slot = board.id; canvas.append(runtime.element);
      return runtime;
    },
    render(state) {
      // The shell owns the empty/error message; the canvas owns populated views.
      canvas.style.display = state.artboards.length ? '' : 'none';
      canvas.state = state; publisher?.schedule();
      queueMicrotask(focusSelection);
      const board = state.artboards.find(b => b.id === state.selected);
      if (!board) return;
      const count = state.artboards.reduce((total, b) => total + (Array.isArray(b.view?.payload.annotations) ? b.view.payload.annotations.length : 0), 0);
      $('annotationCount').hidden = !count;
      $('annotationCount').textContent = count === 1 ? '1 annotation' : count + ' annotations';
      if (board.id !== previewBoard) {
        previewBoard = board.id;
        chrome.wbPreviewControls.reset();
        if (previews.has(board.id)) chrome.wbPreviewControls.receive(previews.get(board.id));
      }
      if (board.view?.tool) document.querySelectorAll<HTMLElement>('[data-tool]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.tool === board.view!.tool)));
    },
    host(board, data) {
      const message = data as { type: string; value?: unknown; [key: string]: unknown };
      if (message.type === 'wb-runtime-preview') {
        const value = message.value as { event?: string };
        if (value?.event === 'ready') previews.set(board.id, value);
        if (controller.snapshot().selected === board.id) chrome.wbPreviewControls.receive(value);
        return;
      }
      if (message.type === 'wb-runtime-camera') {
        const value = message.value as { key?: string; wheel?: number; x?: number; y?: number };
        if (value.key === 'pan-start' || value.key === 'pan-stop') canvas.panReady(value.key === 'pan-start');
        else if (value.key) camera(value.key);
        else if (typeof value.wheel === 'number') {
          const rect = controller.runtime(board.id)!.element.getBoundingClientRect(), outer = canvas.getBoundingClientRect();
          canvas.wheelZoom(value.wheel, { x: rect.left - outer.left + (value.x || 0) * canvas.scale, y: rect.top - outer.top + (value.y || 0) * canvas.scale });
        }
        return;
      }
      if (message.type === 'wb-here') {
        // Only the selected renderer's requested destination may acknowledge
        // navigation to the editor. Pending reports do not settle selection.
        if (!message.pending && selected()?.id === board.id && message.src === selection?.src &&
            (message.state || null) === selection?.state && (message.lens || null) === selection?.lens &&
            window.parent !== window) window.parent.postMessage(message, '*');
        return;
      }
      if (message.type === 'wb-frame-change') return;
      if (message.type === 'wb-context-menu') {
        const rect = controller.runtime(board.id)!.element.getBoundingClientRect();
        message.x = rect.left + Number(message.x || 0) * canvas.scale;
        message.y = rect.top + Number(message.y || 0) * canvas.scale;
      }
      if (window.parent !== window) window.parent.postMessage(message, '*');
    },
    error: say, rowWidth: 1600, activity: activityChanged,
    available,
  });
  const publisher = createPublisher({
    read: () => ({ state: controller.snapshot(), spaces: registry, browsed: space?.id || '', activity }),
    send: request => json('/_workbench/canvas/space', request),
    withdraw: request => { void fetch('/_workbench/canvas/space', { method: 'POST', keepalive: true, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(request) }).catch(() => {}); },
    error: say,
  });
  function select(board: Artboard) {
    controller.choose(board.id); activityChanged();
    selection?.select(board.target.state, board.size, board.view?.actions?.checked, board.target.src, board.target.lens);
    act(() => command('preview', { command: 'inspect' }));
  }
  canvas.addEventListener('wb-artboard-select', event => {
    const board = controller.snapshot().artboards.find(b => b.id === (event as CustomEvent<{ id: string }>).detail.id);
    if (board) select(board);
  }, { signal });
  canvas.addEventListener('wb-artboard-resize', event => {
    const { id, width, height } = (event as CustomEvent<{ id: string; width: number; height: number }>).detail;
    act(() => controller.command(id, 'size', { width, height }));
  }, { signal });
  canvas.addEventListener('wb-artboard-deselect', () => act(() => command('deselect')), { signal });
  // A click inside a preview also selects its owning artboard.
  let lastSelected = '';
  const identity = (board: Artboard) => JSON.stringify([board.id, board.target.src, board.target.state, board.target.lens]);
  const focusSelection = () => {
    const board = selected();
    if (board?.view?.ready && !expanding && lastSelected !== identity(board)) { lastSelected = identity(board); selection?.select(board.target.state, board.size, board.view.actions?.checked, board.target.src, board.target.lens); }
  };
  canvas.addEventListener('focusin', focusSelection, { signal });
  chrome.wbPreviewControls.connect({ send: value => act(() => command('preview', value)) });
  function camera(key: string) {
    if (key === 'fit' || key === 'fit-all') canvas.fitAll();
    else if (key === 'fit-selected') canvas.fitSelected();
    else if (key === 'actual' || key === '100') canvas.zoomTo(1);
    else canvas.zoomTo(Math.pow(2, key === 'out' ? Math.ceil(Math.log2(canvas.scale) - .000001) - 1 : Math.floor(Math.log2(canvas.scale) + .000001) + 1));
  }
  canvas.addEventListener('wb-camera-change', () => { $('zoomValue').textContent = Math.round(canvas.scale * 100) + '%'; }, { signal });
  const oldZoom = chrome.wbZoom;
  chrome.wbZoom = { ...oldZoom, scale: () => canvas.scale, fit: () => canvas.fitAll(), key(event) {
    const key = oldZoom.command(event); if (!key) return false; event.preventDefault(); camera(key); return true;
  } };
  $('zoomCenter').addEventListener('click', () => canvas.recenter(), { signal });
  for (const [id, key] of [['zoomOut', 'out'], ['zoomIn', 'in']]) $(id!).addEventListener('click', () => camera(key!), { signal });
  $('zoomValue').addEventListener('click', () => {
    const menu = $('zoomMenu'); menu.hidden = !menu.hidden;
    $('zoomValue').setAttribute('aria-expanded', String(!menu.hidden));
    menu.replaceChildren(...[['Zoom to fit', 0], ['100%', 1], ['50%', .5], ['200%', 2]].map(([label, scale]) => {
      const button = document.createElement('button'); button.className = 'wb-menu-row'; button.type = 'button'; button.setAttribute('role', 'menuitem'); button.textContent = String(label);
      button.addEventListener('click', () => { if (scale) canvas.zoomTo(Number(scale)); else canvas.fitAll(); menu.hidden = true; $('zoomValue').setAttribute('aria-expanded', 'false'); });
      return button;
    }));
  }, { signal });
  document.addEventListener('keydown', event => {
    const node = event.composedPath()[0] as HTMLElement;
    if (node?.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(node?.tagName)) return;
    if (chrome.wbZoom.key(event)) event.stopImmediatePropagation();
    else if (event.key === 'Escape') { act(() => command('tool', 'pointer')); $('zoomMenu').hidden = true; }
    else if ((event.metaKey || event.ctrlKey) && event.key === 'z') { event.preventDefault(); act(() => command('undo')); }
  }, { signal, capture: true });
  document.addEventListener('pointerdown', activityChanged, { signal, capture: true });
  document.addEventListener('pointerdown', event => { if (!(event.target as Element).closest('#viewControls')) $('zoomMenu').hidden = true; }, { signal });
  for (const button of document.querySelectorAll<HTMLButtonElement>('[data-tool]')) button.addEventListener('click', () => act(() => command('tool', selected()?.view?.tool === button.dataset.tool ? 'pointer' : button.dataset.tool)), { signal });
  $('shapeCaret').addEventListener('click', () => { $('shapeMenu').hidden = !$('shapeMenu').hidden; }, { signal });
  document.querySelectorAll<HTMLButtonElement>('[data-shape]').forEach(button => button.addEventListener('click', () => {
    const shape = button.dataset.shape!; $('shapeTool').dataset.tool = shape; $('shapeTool').setAttribute('aria-label', button.getAttribute('aria-label')!); $('shapeTool').replaceChildren(button.firstElementChild!.cloneNode(true)); $('shapeMenu').hidden = true;
    act(() => command('tool', shape));
  }, { signal }));
  const initialShape = document.querySelector('[data-shape=rect]')?.firstElementChild;
  if (initialShape) $('shapeTool').replaceChildren(initialShape.cloneNode(true));
  for (const [id, name] of [['undo', 'undo'], ['clearAnnotations', 'clear'], ['reload', 'reload']]) $(id!).addEventListener('click', event => { event.stopImmediatePropagation(); act(() => command(name!)); }, { signal, capture: true });
  $('copyReference').addEventListener('click', event => { event.stopImmediatePropagation(); act(async () => { await chrome.wbReference.copy(reference(controller.snapshot())); say('Copied reference'); }); }, { signal, capture: true });
  async function review(handoff: boolean) {
    if (busy) throw new Error('A canvas capture is already running.');
    busy = true; chrome.wbArtboardHeld = true;
    const locked = Array.from(document.querySelectorAll<HTMLElement>('.wb-topbar,.wb-sidebar,#toolbar,#viewControls,#artboards'));
    locked.forEach(node => { node.inert = true; });
    try {
      await controller.capture(async (snapshot, runtime) => {
        const result = await capture(snapshot, runtime);
        if (handoff) {
          const response = await fetch('/_workbench/shot?name=workbench-canvas', { method: 'POST', headers: { 'Content-Type': 'image/jpeg' }, body: result.blob });
          const saved = await response.json(); if (!response.ok) throw new Error(saved.error || 'Could not save screenshot.');
          await json('/_workbench/handoff', { ...result.payload, file: saved.file });
          await Promise.all(snapshot.artboards.filter(b => b.view?.ready).map(b => runtime(b.id).request('clear-captured')));
          say('Copied handoff');
        } else {
          const url = URL.createObjectURL(result.blob), link = document.createElement('a'); link.href = url; link.download = 'workbench-canvas.jpg'; link.click(); setTimeout(() => URL.revokeObjectURL(url), 60000);
        }
      });
    } finally { busy = false; chrome.wbArtboardHeld = false; locked.forEach(node => { node.inert = false; }); }
  }
  $('shoot').addEventListener('click', () => act(() => review(false)), { signal });
  $('handoff').addEventListener('click', () => act(() => review(true)), { signal });
  void json('/_workbench/handoff').then(value => { if (!disposed) $('handoffControl').hidden = !value.available; }).catch(() => {});
  window.addEventListener('wb-say', event => say((event as CustomEvent).detail.message), { signal });
  window.addEventListener('pagehide', () => { disposed = true; sequence++; connection.abort(); clearTimeout(toastTimer); clearTimeout(parkTimer); publisher.dispose(); controller.dispose(); previews.clear(); catalog.clear(); }, { once: true });
  return {
    async show(next: Selection) {
      const request = ++sequence; await ready; if (disposed || request !== sequence) return;
      if (busy) throw new Error('Wait for the canvas capture to finish.');
      selection = next;
      const declared = !next.docs ? next.artboards : undefined;
      const key = JSON.stringify([next.src, next.state, next.lens, next.docs ? next.example : null, declared]);
      const size = next.docs ? available() : dimensions(next.size, available(), { width: 1024, height: 768 });
      if (group !== key) {
        group = key; previews.clear();
        expanding = true;
        try {
          const boards = controller.snapshot().artboards;
          const retained = !declared?.length && boards.length === 1 && !boards[0]!.target.artboard && boards[0]!.view;
          if (retained) {
            // One renderer owns the ordinary page/lens sessions. Navigating it
            // preserves Storybook's channel and the renderer's bounded cache.
            await controller.command(boards[0]!.id, 'navigate', { src: next.src, state: next.state, lens: next.lens, example: next.example });
          } else {
            for (const board of [...boards]) {
              if (declared?.length && !board.target.artboard) {
                const element = controller.runtime(board.id)!.element;
                element.style.display = 'none'; element.inert = true;
                controller.park(board.id); parkedOrdinary = board.id;
                clearTimeout(parkTimer); parkTimer = setTimeout(expireOrdinary, 15 * 60 * 1000);
              } else controller.close(board.id);
            }
            if (declared?.length) {
              for (const artboard of declared) controller.create({ space, src: next.src, state: next.state, lens: next.lens, example: next.example, artboard: artboard.id }, { width: artboard.width, height: artboard.height });
              controller.choose(controller.snapshot().artboards[0]!.id);
            } else if (parkedOrdinary) {
              const id = parkedOrdinary; parkedOrdinary = undefined; clearTimeout(parkTimer);
              const element = controller.runtime(id)!.element;
              element.style.display = ''; element.inert = false;
              controller.resume(id);
              await controller.command(id, 'navigate', { src: next.src, state: next.state, lens: next.lens, example: next.example });
            } else controller.create({ space, src: next.src, state: next.state, lens: next.lens, example: next.example }, size);
          }
        } catch (error) {
          if (disposed || request !== sequence) return;
          group = '';
          throw error;
        } finally { if (request === sequence) expanding = false; }
        if (disposed || request !== sequence) return;
        canvas.fitAll();
      } else {
        // Repeated routes keep the selected artboard and its live inputs.
      }
      lastSelected = selected() ? identity(selected()!) : '';
      const board = selected();
      if (board?.view?.actions && board.view.actions.checked !== next.actions) await command('actions', next.actions);
      if (board?.view?.ready) await command('preview', { command: 'inspect' });
      activityChanged();
    },
    resize(size: ResolvedSize) { return command('size', dimensions(size, available(), selected()?.size || { width: 1024, height: 768 })); },
    selectedSize: () => selected()?.size,
    selectedArtboard: () => selected()?.target.artboard,
    stories: catalog.stories,
    refresh() { catalog.clear(); return Promise.all([...controller.snapshot().artboards.map(b => b.id), ...(parkedOrdinary ? [parkedOrdinary] : [])].map(id => controller.command(id, 'refresh'))); },
    clear() {
      expanding = true;
      try {
        for (const board of [...controller.snapshot().artboards]) controller.close(board.id);
        if (parkedOrdinary) controller.close(parkedOrdinary);
        parkedOrdinary = undefined; clearTimeout(parkTimer); group = '';
      } finally { expanding = false; }
    },
  };
}
