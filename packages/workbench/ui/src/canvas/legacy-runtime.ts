/** Migration adapter: existing single-view renderer is owned by one iframe.
 * Explicit shell/annotation ports preserve its compiler, native and capture APIs.
 * Only this adapter knows the legacy window surface. */
import type { Size, ViewInfo } from './model.ts';
import { validDimensions } from '../../../contracts/src/sizes/browser/geometry.ts';
import { connectStoryCatalog } from '../modules/canvas/story-catalog.ts';
interface Shell {
  problem(): string | null;
  read(): Omit<ViewInfo, 'reference' | 'problem' | 'payload'> | null;
  navigate(value: unknown): void; size(width: number, height: number): void;
  pickState(id: string): void; lens(key: string): void; reload(): void; refresh(): void;
  action(name: string, value?: unknown): void;
}
interface Annotations {
  deselect(): void;
  toolName(): string;
  tool(name: string): void; undo(): void; clear(): void;
  payload(): Record<string, unknown>;
  capture(): Promise<{ blob: Blob; payload: Record<string, unknown>; at: number; annotations: unknown[] }>;
  clearCaptured(annotations: unknown[]): void;
}
interface LegacyWindow extends Window {
  wbRequestStories?: (implementation: string, title: string) => Promise<unknown>;
  wbArtboardHeld?: boolean;
  wbArtboardShell: Shell; wbArtboardAnnotations: Annotations;
  wbView(): unknown; wbReference: { text(value: unknown): string | null };
  wbZoom: { update(): void };
}
const params = new URLSearchParams(location.search);
const channel = params.get('artboard-runtime');
const origin = params.get('canvas-origin');
if (channel && origin && window.parent !== window) start(window as unknown as LegacyWindow, channel, origin);
function start(legacy: LegacyWindow, channel: string, origin: string) {
  const catalog = connectStoryCatalog(window.parent, origin, channel);
  legacy.wbRequestStories = catalog.stories;
  window.addEventListener('pagehide', () => catalog.dispose(), { once: true });
  window.dispatchEvent(new Event('wb-artboard-ready'));
  const style = document.createElement('style');
  style.textContent = `
    .wb{display:block!important;width:100vw!important;height:100vh!important}
    .wb-topbar,.wb-sidebar,.wb-toolbar,.wb-view-controls,.wb-artboard-label{display:none!important}
    .wb-main,.wb-canvas{position:absolute!important;inset:0!important;width:100%!important;height:100%!important;padding:0!important}
    #artboard{position:absolute!important;left:0!important;top:0!important;transform:none!important;margin:0!important}
    .wb[data-canvas-mode=docs] #artboard{width:100%!important;height:100%!important}
  `;
  document.head.append(style);
  // Review belongs to the complete owning canvas. Local review proxies stay hidden.
  const reviewStyle = document.createElement('style');
  reviewStyle.textContent = '#shoot,#copyReference,#handoffControl,#annotationCount,#viewControls{display:none!important}';
  document.head.append(reviewStyle);
  const contentStyle = document.createElement('style');
  contentStyle.textContent = '.wb.is-content-fitted,.wb.is-content-fitted .wb-main,.wb.is-content-fitted .wb-canvas,.wb.is-content-fitted .wb-artboard-content{background:transparent!important;background-image:none!important;box-shadow:none!important}';
  document.head.append(contentStyle);
  for (const id of ['shoot', 'copyReference', 'handoff']) {
    const button = document.getElementById(id) as HTMLButtonElement;
    button.disabled = true; button.hidden = true;
  }
  let capturedAnnotations: unknown[] = [], capturing = false, held = false;
  let contentSize: Size | undefined;
  let observedContent: Element | null = null;
  const contentObserver = new ResizeObserver(() => read());
  let previous = '';
  const emit = (event: string, value?: unknown, id?: string, error?: string) => window.parent.postMessage({ type: 'wb-artboard', channel, event, value, id, error }, origin);
  function read() {
    const shell = legacy.wbArtboardShell.read();
    const problem = legacy.wbArtboardShell.problem();
    if (!shell) {
      if (!problem) return null;
      const src = location.hash.replace(/^#/, '').split(/[:!@~]/)[0] || 'unavailable';
      const info: ViewInfo = { src, state: null, lens: null, label: src, hash: location.hash, ready: false, problem, reference: '', payload: {}, states: [], lenses: [] };
      const serial = JSON.stringify(info); if (serial !== previous) { previous = serial; emit('view', info); }
      return info;
    }
    const payload = legacy.wbArtboardAnnotations.payload();
    const artboard = params.get('artboard');
    if (artboard) { payload.artboard = artboard; payload.label = shell.label; }
    if (!held && !capturing && shell.ready) {
      contentSize = undefined;
      try {
        const doc = document.querySelector<HTMLIFrameElement>('#artboardContent iframe.is-active')?.contentDocument;
        const content = doc?.querySelector<HTMLElement>('[data-wb-fit-content]');
        if ((content || null) !== observedContent) {
          contentObserver.disconnect(); observedContent = content || null;
          if (content) contentObserver.observe(content);
        }
        if (content) {
          const box = content.getBoundingClientRect();
          const measured = { width: Math.max(1, Math.ceil(box.width)), height: Math.max(1, Math.ceil(box.height)) };
          if (Math.abs(box.x) < 1 && Math.abs(box.y) < 1 && validDimensions(measured)) contentSize = measured;
        }
      } catch { /* External frames retain their explicitly selected dimensions. */ }
      document.querySelector('.wb')?.classList.toggle('is-content-fitted', !!contentSize);
    }
    const reference = legacy.wbReference.text(legacy.wbView()) || '';
    const info: ViewInfo = { ...shell, contentSize, payload, tool: legacy.wbArtboardAnnotations.toolName(), reference: reference + (artboard ? '\n- Artboard: ' + artboard : ''), problem };
    const status = document.getElementById('simulatorStatus');
    if (status && /failed|denied|requires|not found/i.test(status.textContent || '')) info.problem = status.textContent;
    const serial = JSON.stringify(info);
    if (serial !== previous) { previous = serial; emit('view', info); }
    return info;
  }
  async function command(event: MessageEvent) {
    const data = event.data;
    if (event.source !== window.parent || event.origin !== origin || data?.type !== 'wb-artboard-command' || data.channel !== channel || typeof data.id !== 'string') return;
    try {
      if ((held || capturing) && !['inspect','capture','unlock','clear-captured'].includes(data.command)) throw new Error('Artboard capture is in progress.');
      let result: unknown = null;
      switch (data.command) {
        case 'lock': held = true; legacy.wbArtboardHeld = true; document.getElementById('artboardContent')!.inert = true; break;
        case 'unlock': held = false; legacy.wbArtboardHeld = false; document.getElementById('artboardContent')!.inert = false; break;
        case 'navigate': legacy.wbArtboardShell.navigate(data.value); break;
        case 'size': {
          if (!validDimensions(data.value)) throw new Error('Invalid artboard size.');
          legacy.wbArtboardShell.size(data.value.width, data.value.height); break;
        }
        case 'state': legacy.wbArtboardShell.pickState(String(data.value)); break;
        case 'lens': legacy.wbArtboardShell.lens(String(data.value || '')); break;
        case 'reload': legacy.wbArtboardShell.reload(); break;
        case 'refresh': legacy.wbArtboardShell.refresh(); break;
        case 'actions': legacy.wbArtboardShell.action('actions', data.value); break;
        case 'preview': {
          const frame = document.querySelector<HTMLIFrameElement>('#artboardContent iframe.is-active');
          frame?.contentWindow?.postMessage({ type: 'workbench-preview-command', ...data.value }, location.origin);
          break;
        }
        case 'tool': if (!['pointer','draw','arrow','line','rect','ellipse','text','comment'].includes(data.value)) throw new Error('Unknown annotation tool.'); legacy.wbArtboardAnnotations.tool(data.value); break;
        case 'undo': legacy.wbArtboardAnnotations.undo(); break;
        case 'deselect': legacy.wbArtboardAnnotations.deselect(); break;
        case 'clear': legacy.wbArtboardAnnotations.clear(); break;
        case 'clear-captured': legacy.wbArtboardAnnotations.clearCaptured(capturedAnnotations); capturedAnnotations = []; break;
        case 'capture': {
          capturing = true;
          try {
            const captured = await legacy.wbArtboardAnnotations.capture(); capturedAnnotations = captured.annotations;
            const artboard = params.get('artboard');
            if (artboard) { captured.payload.artboard = artboard; captured.payload.label = legacy.wbArtboardShell.read()?.label || captured.payload.label; }
            if (captured.payload.docs) {
              const response = await fetch('/_workbench/canvas/review', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(captured.payload) });
              const result = await response.json(); if (!response.ok) throw new Error(result.error || 'Could not resolve docs review.'); captured.payload = result.payload;
            }
            result = { blob: captured.blob, payload: captured.payload, at: captured.at };
          } finally { capturing = false; }
          break;
        }
        case 'inspect': result = read(); break;
        default: throw new Error('Unknown artboard command.');
      }
      read(); emit('response', result, data.id);
    } catch (error) { emit('response', null, data.id, error instanceof Error ? error.message : String(error)); }
  }
  window.addEventListener('message', command);
  window.addEventListener('message', event => {
    const frame = document.querySelector<HTMLIFrameElement>('#artboardContent iframe.is-active');
    if (event.origin === location.origin && event.source === frame?.contentWindow && event.data?.type === 'workbench-preview') emit('preview', event.data);
  });
  window.addEventListener('wb-frame-change', read);
  window.addEventListener('hashchange', read);
  window.addEventListener('focus', () => emit('focus'));
  document.addEventListener('pointerdown', () => emit('focus'), true);
  // Same-origin preview documents receive clicks without bubbling to the shell.
  const watched = new WeakSet<Document>();
  function watchDocument(doc: Document) {
    if (watched.has(doc)) return;
    watched.add(doc);
    doc.addEventListener('pointerdown', () => emit('focus'), true);
    doc.addEventListener('wheel', event => {
      if (!event.ctrlKey && !event.metaKey) return;
      event.preventDefault(); emit('camera', { wheel: event.deltaY, x: event.clientX, y: event.clientY });
    }, { passive: false });
    doc.addEventListener('keydown', event => {
      const node = event.composedPath()[0] as HTMLElement;
      if (node?.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(node?.tagName)) return;
      const key = (event.metaKey || event.ctrlKey) && !event.altKey
        ? event.key === '0' ? '100' : ['=', '+'].includes(event.key) ? 'in' : event.key === '-' ? 'out' : null
        : event.shiftKey && event.key === '1' ? 'fit-all' : event.shiftKey && event.key === '2' ? 'fit-selected' : event.code === 'Space' ? 'pan-start' : null;
      if (key) { event.preventDefault(); event.stopImmediatePropagation(); emit('camera', { key }); }
    }, true);
    doc.addEventListener('keyup', event => { if (event.code === 'Space') emit('camera', { key: 'pan-stop' }); }, true);
  }
  function watch() {
    for (const frame of document.querySelectorAll<HTMLIFrameElement>('#artboardContent iframe')) {
      try {
        const doc = frame.contentDocument; if (!doc) continue;
        watchDocument(doc);
      } catch { /* Cross-origin focus is detected by the shell blur below. */ }
    }
  }
  window.addEventListener('blur', () => { if (document.hasFocus() && document.activeElement?.tagName === 'IFRAME') emit('focus'); });
  const timer = setInterval(() => { read(); watch(); }, 500);
  watchDocument(document);
  window.addEventListener('pagehide', () => { clearInterval(timer); contentObserver.disconnect(); window.removeEventListener('message', command); }, { once: true });
  read();
}
