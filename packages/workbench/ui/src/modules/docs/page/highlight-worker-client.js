// One lazy worker per docs document. Cancelling CPU work terminates that worker;
// the next request can start fresh, without queuing behind an obsolete job.
export function createWorkerColouring({
  createWorker = () =>
    new Worker(new URL('./highlight-worker.js', import.meta.url), { type: 'module' }),
} = {}) {
  let worker;
  let sequence = 0;
  let disposed = false;
  const pending = new Map();
  const aborted = () => new DOMException('Highlighting cancelled', 'AbortError');
  function stop(error) {
    const previous = worker;
    worker = undefined;
    if (previous) {
      previous.onmessage = previous.onerror = previous.onmessageerror = null;
      previous.terminate();
    }
    for (const job of pending.values()) {
      job.detach();
      job.reject(error);
    }
    pending.clear();
  }
  return {
    colour(blocks, { signal }) {
      if (disposed || signal.aborted) return Promise.reject(aborted());
      if (!worker) {
        try {
          worker = createWorker();
        } catch (error) {
          return Promise.reject(error);
        }
        worker.onmessage = ({ data }) => {
          const job = pending.get(data.id);
          if (!job) return;
          pending.delete(data.id);
          job.detach();
          if (data.error) job.reject(new Error(data.error));
          else if (!Array.isArray(data.blocks))
            job.reject(new Error('Invalid highlighting response'));
          else job.resolve(data.blocks);
        };
        worker.onerror = (event) => {
          event.preventDefault();
          stop(new Error(event.message || 'Highlighting worker failed'));
        };
        worker.onmessageerror = () => stop(new Error('Could not read highlighting response'));
      }
      const id = ++sequence;
      return new Promise((resolve, reject) => {
        const cancel = () => stop(aborted());
        signal.addEventListener('abort', cancel, { once: true });
        pending.set(id, {
          resolve,
          reject,
          detach: () => signal.removeEventListener('abort', cancel),
        });
        try {
          worker.postMessage({ id, blocks });
        } catch (error) {
          stop(error);
        }
      });
    },
    reset() {
      stop(aborted());
    },
    dispose() {
      disposed = true;
      stop(aborted());
    },
  };
}
