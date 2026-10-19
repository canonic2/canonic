import { createStoryCatalog, fetchStoryCatalog } from './story-catalog.js';

export function createRuntime(board, callbacks) {
  const element = document.createElement('iframe');
  element.title = board.view?.label || board.target.label || board.target.src;
  element.allow = 'clipboard-read; clipboard-write';
  // Publish content only after the requested destination reports readiness.
  element.style.visibility = 'hidden';
  const channel = crypto.randomUUID();
  const url = new URL('content.html', board.target.space.url);
  url.searchParams.set('artboard-runtime', channel);
  url.searchParams.set('canvas-origin', location.origin);
  url.searchParams.set('canvas-host', window.parent === window ? 'browser' : 'editor');
  if (board.target.artboard) url.searchParams.set('artboard', board.target.artboard);
  url.hash = `${board.target.src}${board.target.state ? ':' + board.target.state : ''}${board.target.example ? '!' + board.target.example : ''}~${board.target.lens || 'design'}`;
  element.src = url.href;
  let disposed = false;
  // Standalone support hosts can omit the shared canvas port.
  const localCatalog = callbacks.stories
    ? null
    : createStoryCatalog((implementation, title) =>
        fetchStoryCatalog(implementation, title, board.target.space.url),
      );
  const pending = new Map();
  function onMessage(event) {
    if (disposed || event.source !== element.contentWindow || event.origin !== url.origin) return;
    const data = event.data;
    if (
      data?.type === 'wb-story-catalog-request' &&
      data.channel === channel &&
      typeof data.id === 'string' &&
      typeof data.implementation === 'string' &&
      typeof data.title === 'string'
    ) {
      const reply = (value, error) => {
        if (!disposed)
          element.contentWindow?.postMessage(
            { type: 'wb-story-catalog-response', channel, id: data.id, value, error },
            url.origin,
          );
      };
      void Promise.resolve()
        .then(() => {
          return (callbacks.stories || localCatalog.stories)(data.implementation, data.title);
        })
        .then(
          (value) => reply(value),
          (error) => reply(undefined, String(error.message || error)),
        );
      return;
    }
    if (!data || data.channel !== channel || data.type !== 'wb-artboard') {
      if (data?.type?.startsWith('wb-')) callbacks.host(data);
      return;
    }
    if (data.event === 'view' && validInfo(data.value)) {
      element.title = data.value.label;
      if (data.value.ready) element.style.visibility = '';
      callbacks.view(data.value);
    }
    if (data.event === 'focus') callbacks.focus();
    if (data.event === 'preview') callbacks.host({ type: 'wb-runtime-preview', value: data.value });
    if (data.event === 'camera') callbacks.host({ type: 'wb-runtime-camera', value: data.value });
    if (data.event === 'response' && typeof data.id === 'string') {
      const job = pending.get(data.id);
      if (!job) return;
      pending.delete(data.id);
      clearTimeout(job.timer);
      if (typeof data.error === 'string') job.reject(new Error(data.error));
      else job.resolve(data.value);
    }
  }
  window.addEventListener('message', onMessage);
  function request(command, value) {
    if (disposed) return Promise.reject(new Error('Artboard closed.'));
    const id = crypto.randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`${command} timed out for ${board.target.src}.`));
      }, 30000);
      pending.set(id, { resolve, reject, timer });
      element.contentWindow?.postMessage(
        { type: 'wb-artboard-command', channel, id, command, value },
        url.origin,
      );
    });
  }
  return {
    element,
    request,
    async capture() {
      const result = await request('capture');
      if (
        !result ||
        typeof result !== 'object' ||
        !('blob' in result) ||
        !(result.blob instanceof Blob) ||
        !('payload' in result) ||
        typeof result.payload !== 'object'
      )
        throw new Error('Invalid artboard capture response.');
      return result;
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      window.removeEventListener('message', onMessage);
      for (const job of pending.values()) {
        clearTimeout(job.timer);
        job.reject(new Error('Artboard closed.'));
      }
      pending.clear();
      localCatalog?.clear();
      element.src = 'about:blank';
      element.remove();
    },
  };
}
function validInfo(value) {
  if (!value || typeof value !== 'object') return false;
  const v = value;
  return (
    typeof v.src === 'string' &&
    typeof v.label === 'string' &&
    typeof v.hash === 'string' &&
    typeof v.reference === 'string' &&
    typeof v.ready === 'boolean' &&
    Array.isArray(v.states) &&
    Array.isArray(v.lenses) &&
    !!v.payload &&
    typeof v.payload === 'object'
  );
}
