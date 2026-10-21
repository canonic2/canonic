// Loaded lazily by the shipping highlighter in candidate documents.
export function localRequest(variant) {
  let worker;
  let sequence = 0;
  let disposed = false;
  let snapshot = [];
  const pending = new Map();
  const metrics = (window.__labHighlightMetrics = []);
  const escape = (text) =>
    text.replace(
      /[&<>"']/g,
      (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c],
    );
  const terminate = () => {
    disposed = true;
    worker?.terminate();
    worker = null;
    for (const job of pending.values()) job.reject(new Error('Worker disposed'));
    pending.clear();
  };
  addEventListener('pagehide', terminate, { once: true });
  async function colour(blocks, signal) {
    if (disposed || signal?.aborted) throw new DOMException('Aborted', 'AbortError');
    if (variant === 'D') return blocks.map((block) => ({ ...block, html: escape(block.text) }));
    if (!worker) {
      worker = new Worker('/_workbench/ui/src/lab/worker.js');
      worker.onmessage = ({ data }) => {
        const job = pending.get(data.id);
        if (!job) return;
        pending.delete(data.id);
        job.detach();
        metrics.push({
          ...data,
          blocks: undefined,
          startedAbs: performance.timeOrigin + job.started,
          totalWorkerMs: performance.now() - job.started,
        });
        data.error ? job.reject(Error(data.error)) : job.resolve(data.blocks);
      };
      worker.onerror = (event) => {
        metrics.push({ error: event.message });
        terminate();
      };
    }
    const id = ++sequence;
    return new Promise((resolve, reject) => {
      const abort = () => {
        pending.delete(id);
        reject(new DOMException('Aborted', 'AbortError'));
      };
      if (signal?.aborted) {
        abort();
        return;
      }
      signal?.addEventListener('abort', abort, { once: true });
      pending.set(id, {
        resolve,
        reject,
        started: performance.now(),
        detach: () => signal?.removeEventListener('abort', abort),
      });
      worker.postMessage({ id, variant, blocks });
    });
  }
  const request = async (value, options) => {
    if (disposed || options?.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
    const url = new URL(value, location.href);
    let body;
    if (url.pathname.endsWith('/highlight')) body = { blocks: snapshot };
    else {
      url.searchParams.set('plain', '1');
      const response = await fetch(url, options);
      if (!response.ok) return response;
      body = await response.json();
      body.blocks = [{ text: body.text, language: body.file?.split('.').pop() }];
    }
    const blocks = await colour(body.blocks, options?.signal);
    return {
      ok: true,
      json: async () =>
        url.pathname.endsWith('/highlight') ? { blocks } : { ...body, html: blocks[0].html },
    };
  };
  return {
    request,
    fences(root) {
      snapshot = [...root.querySelectorAll('code[data-wb-code]')].map((node) => ({
        text: node.textContent,
        language: node.dataset.wbCode,
      }));
    },
  };
}
