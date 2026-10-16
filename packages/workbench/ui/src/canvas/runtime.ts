import type { Artboard, ViewInfo } from './model.ts';
export interface Captured { blob: Blob; payload: Record<string, unknown>; at: number }
export interface Runtime {
  element: HTMLIFrameElement;
  request(command: string, value?: unknown): Promise<unknown>;
  capture(): Promise<Captured>;
  dispose(): void;
}
export function createRuntime(board: Artboard, callbacks: { view(info: ViewInfo): void; focus(): void; host(data: unknown): void }): Runtime {
  const element = document.createElement('iframe');
  element.title = board.target.src;
  element.allow = 'clipboard-read; clipboard-write';
  const channel = crypto.randomUUID();
  const url = new URL('index.html', board.target.space.url);
  url.searchParams.set('artboard-runtime', channel);
  url.searchParams.set('canvas-origin', location.origin);
  url.searchParams.set('canvas-host', window.parent === window ? 'browser' : 'editor');
  url.hash = `${board.target.src}${board.target.state ? ':' + board.target.state : ''}${board.target.example ? '!' + board.target.example : ''}${board.target.lens ? '~' + board.target.lens : ''}`;
  element.src = url.href;
  let disposed = false;
  const pending = new Map<string, { resolve(value: unknown): void; reject(error: Error): void; timer: ReturnType<typeof setTimeout> }>();
  function onMessage(event: MessageEvent) {
    if (disposed || event.source !== element.contentWindow || event.origin !== url.origin) return;
    const data = event.data;
    if (!data || data.channel !== channel || data.type !== 'wb-artboard') {
      if (data?.type?.startsWith('wb-')) callbacks.host(data);
      return;
    }
    if (data.event === 'view' && validInfo(data.value)) callbacks.view(data.value);
    if (data.event === 'focus') callbacks.focus();
    if (data.event === 'response' && typeof data.id === 'string') {
      const job = pending.get(data.id); if (!job) return;
      pending.delete(data.id); clearTimeout(job.timer);
      if (typeof data.error === 'string') job.reject(new Error(data.error)); else job.resolve(data.value);
    }
  }
  window.addEventListener('message', onMessage);
  function request(command: string, value?: unknown): Promise<unknown> {
    if (disposed) return Promise.reject(new Error('Artboard closed.'));
    const id = crypto.randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${command} timed out for ${board.target.src}.`)); }, 30000);
      pending.set(id, { resolve, reject, timer });
      element.contentWindow?.postMessage({ type: 'wb-artboard-command', channel, id, command, value }, url.origin);
    });
  }
  return { element, request,
    async capture() {
      const result = await request('capture');
      if (!result || typeof result !== 'object' || !('blob' in result) || !(result.blob instanceof Blob) || !('payload' in result) || typeof result.payload !== 'object') throw new Error('Invalid artboard capture response.');
      return result as Captured;
    },
    dispose() {
      if (disposed) return; disposed = true;
      window.removeEventListener('message', onMessage);
      for (const job of pending.values()) { clearTimeout(job.timer); job.reject(new Error('Artboard closed.')); }
      pending.clear(); element.src = 'about:blank'; element.remove();
    } };
}
function validInfo(value: unknown): value is ViewInfo {
  if (!value || typeof value !== 'object') return false;
  const v = value as ViewInfo;
  return typeof v.src === 'string' && typeof v.label === 'string' && typeof v.hash === 'string' && typeof v.reference === 'string'
    && typeof v.ready === 'boolean' && Array.isArray(v.states) && Array.isArray(v.lenses) && !!v.payload && typeof v.payload === 'object';
}
