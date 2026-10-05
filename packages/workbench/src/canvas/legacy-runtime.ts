/** Migration adapter: existing single-view renderer is owned by one iframe.
 * Explicit shell/annotation ports preserve its compiler, native and capture APIs.
 * Only this adapter knows the legacy window surface. */
import type { ViewInfo } from './model.ts';
import { validDimensions } from '../sizes/browser/geometry.ts';
interface Shell {
  problem(): string | null;
  read(): Omit<ViewInfo, 'reference' | 'problem' | 'payload'> | null;
  navigate(value: unknown): void; size(width: number, height: number): void;
  pickState(id: string): void; lens(key: string): void; reload(): void; refresh(): void;
}
interface Annotations {
  toolName(): string;
  tool(name: string): void; undo(): void; clear(): void;
  payload(): Record<string, unknown>;
  capture(): Promise<{ blob: Blob; payload: Record<string, unknown>; at: number; annotations: unknown[] }>;
  clearCaptured(annotations: unknown[]): void;
}
interface LegacyWindow extends Window {
  wbArtboardHeld?: boolean;
  wbArtboardShell: Shell; wbArtboardAnnotations: Annotations;
  wbView(): unknown; wbReference: { text(value: unknown): string | null };
}
const params = new URLSearchParams(location.search);
const channel = params.get('artboard-runtime');
const origin = params.get('canvas-origin');
if (channel && origin && window.parent !== window) start(window as unknown as LegacyWindow, channel, origin);
function start(legacy: LegacyWindow, channel: string, origin: string) {
  let capturedAnnotations: unknown[] = [], capturing = false, held = false;
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
    const info: ViewInfo = { ...shell, payload, tool: legacy.wbArtboardAnnotations.toolName(), reference: legacy.wbReference.text(legacy.wbView()) || '', problem };
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
        case 'tool': if (!['pointer','draw','arrow','line','rect','ellipse','text','comment'].includes(data.value)) throw new Error('Unknown annotation tool.'); legacy.wbArtboardAnnotations.tool(data.value); break;
        case 'undo': legacy.wbArtboardAnnotations.undo(); break;
        case 'clear': legacy.wbArtboardAnnotations.clear(); break;
        case 'clear-captured': legacy.wbArtboardAnnotations.clearCaptured(capturedAnnotations); capturedAnnotations = []; break;
        case 'capture': {
          capturing = true;
          try {
            const captured = await legacy.wbArtboardAnnotations.capture(); capturedAnnotations = captured.annotations;
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
  window.addEventListener('wb-frame-change', read);
  window.addEventListener('hashchange', read);
  window.addEventListener('focus', () => emit('focus'));
  document.addEventListener('pointerdown', () => emit('focus'), true);
  // Same-origin preview documents receive clicks without bubbling to the shell.
  const watched = new WeakSet<Document>();
  function watch() {
    for (const frame of document.querySelectorAll<HTMLIFrameElement>('#artboardContent iframe')) {
      try {
        const doc = frame.contentDocument; if (!doc || watched.has(doc)) continue;
        doc.addEventListener('pointerdown', () => emit('focus'), true); watched.add(doc);
      } catch { /* Cross-origin focus is detected by the shell blur below. */ }
    }
  }
  window.addEventListener('blur', () => { if (document.activeElement?.tagName === 'IFRAME') emit('focus'); });
  const timer = setInterval(() => { read(); watch(); }, 500);
  window.addEventListener('pagehide', () => { clearInterval(timer); window.removeEventListener('message', command); }, { once: true });
  read();
}
