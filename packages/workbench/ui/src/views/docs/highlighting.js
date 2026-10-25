/* Live code starts as readable text. Colouring is independent of page readiness,
   tied to its exact nodes/text, and cancelled when its document is disposed. */
export function createHighlighting({
  colour,
  frame = nextFrame,
  report = () => {},
  document = globalThis.document,
} = {}) {
  const pending = new Set();
  const controllers = new Set();
  let disposed = false;
  let fencesRequest;
  let worker;
  let colours;
  const sources = new WeakMap();

  async function tokenize(blocks, signal) {
    if (colour) return colour(blocks, { signal });
    if (!worker) {
      const { createWorkerColouring } = await import('./highlight-worker-client.js');
      if (disposed || signal.aborted) return [];
      worker ||= createWorkerColouring();
    }
    return worker.colour(blocks, { signal });
  }

  async function applyCode(element, block, signal) {
    if (!colours) {
      const { createCodeColours } = await import('./code-colours.js');
      if (disposed || signal.aborted) return;
      colours ||= createCodeColours(document);
    }
    if (!disposed && !signal.aborted) colours.apply(element, block.text, block.html);
  }

  async function afterLoad(signal) {
    if (document.readyState !== 'complete' && !signal.aborted) {
      await new Promise((resolve) => {
        const done = () => {
          document.defaultView.removeEventListener('load', done);
          signal.removeEventListener('abort', done);
          resolve();
        };
        document.defaultView.addEventListener('load', done, { once: true });
        signal.addEventListener('abort', done, { once: true });
      });
    }
    // A worker created during document load can delay that event and the
    // enclosing frame's promotion. Let the plain document paint first.
    await frame();
    await frame();
  }

  function run(blocks, apply) {
    if (disposed || !blocks.length) return { task: Promise.resolve(), cancel() {} };
    const controller = new AbortController();
    controllers.add(controller);
    const timeout = setTimeout(() => controller.abort(), 5000);
    let onAbort;
    const aborted = new Promise((resolve) => {
      onAbort = resolve;
      controller.signal.addEventListener('abort', onAbort, { once: true });
    });
    const work = (async () => {
      await afterLoad(controller.signal);
      if (controller.signal.aborted) return;
      const result = await tokenize(blocks, controller.signal);
      if (!disposed && !controller.signal.aborted) await apply(result, controller.signal);
    })();
    const task = Promise.race([work, aborted])
      .catch(() => {
        if (!disposed && !controller.signal.aborted) report('highlight-failed');
        // Plain, escaped code remains usable on failure, timeout or cancellation.
      })
      .finally(() => {
        clearTimeout(timeout);
        controller.signal.removeEventListener('abort', onAbort);
        controllers.delete(controller);
        pending.delete(task);
      });
    pending.add(task);
    return { task, cancel: () => controller.abort() };
  }

  return {
    fences(root) {
      fencesRequest?.cancel();
      const nodes = Array.from(root.querySelectorAll('code[data-wb-code]'));
      if (!nodes.length) return;
      const blocks = nodes.map((node) => ({
        text: node.textContent,
        language: node.dataset.wbCode,
      }));
      fencesRequest = run(blocks, async (result, signal) => {
        let started = performance.now();
        for (const [index, node] of nodes.entries()) {
          if (disposed || signal.aborted) return;
          const block = result[index];
          if (block?.text === blocks[index].text && block.language === node.dataset.wbCode)
            await applyCode(node, block, signal);
          if (performance.now() - started >= 8) {
            await frame();
            started = performance.now();
          }
        }
        await frame();
        report('highlight-fences');
      });
    },
    source(element, language) {
      sources.get(element)?.cancel();
      const text = element.textContent;
      const task = run([{ text, language }], async (result, signal) => {
        if (result[0]?.text === text) await applyCode(element, result[0], signal);
        await frame();
        report('highlight-source');
      });
      sources.set(element, task);
    },
    async settled() {
      while (pending.size) await Promise.all(Array.from(pending));
    },
    reset() {
      for (const controller of controllers) controller.abort();
      worker?.reset();
      colours?.reset();
    },
    dispose() {
      disposed = true;
      for (const controller of controllers) controller.abort();
      worker?.dispose();
      colours?.dispose();
    },
  };
}

function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}
