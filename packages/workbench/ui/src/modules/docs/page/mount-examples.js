/* Mounts a docs page's examples, each into its own panel, through the lens's
   adapter and environment, the way a preview mounts its source. One
   example's failure stays in its panel. The page is ready once every
   example has mounted, the environment's ready hook has run, and fonts and
   visible images have loaded. */

/** What a lens's compiled bundle exports. */

const text = (value) => {
  try {
    return typeof value === 'string' ? value : JSON.stringify(value);
  } catch {
    return String(value);
  }
};

export async function mountExamples(root, bundle, options) {
  const controller = new AbortController();
  const cleanups = [];
  const results = [];
  const abort = () => controller.abort();
  options.signal?.addEventListener('abort', abort, { once: true });
  if (options.signal?.aborted) abort();
  let disposed = false;
  async function dispose() {
    if (disposed) return;
    disposed = true;
    controller.abort();
    options.signal?.removeEventListener('abort', abort);
    let failure;
    for (const cleanup of cleanups.reverse()) {
      try {
        await cleanup();
      } catch (error) {
        failure ??= error;
      }
    }
    if (failure) throw failure;
  }

  function context(id, figure) {
    return {
      id,
      state: options.state,
      inputs: {},
      fixtures: {},
      globals: {},
      signal: controller.signal,
      error: (error) => {
        if (controller.signal.aborted) return;
        if (figure) fail(figure, id, error);
        else options.report('error', { message: String(error?.message ?? error) });
      },
      action: (name, ...values) => {
        if (!controller.signal.aborted)
          options.report('action', { name, values: values.map(text), example: id });
      },
      navigate: (to) => {
        if (!controller.signal.aborted) options.report('navigate', { to, example: id });
      },
    };
  }

  function fail(figure, id, error) {
    const message = String(error?.stack ?? error);
    figure.dataset.status = 'error';
    const stage = figure.querySelector('[data-wb-example-stage]');
    const shown = Object.assign(document.createElement('pre'), {
      className: 'wb-docs-example-error',
      textContent: message,
    });
    if (stage) stage.replaceWith(shown);
    else figure.prepend(shown);
    const result = results.find((entry) => entry.id === id);
    if (result)
      Object.assign(result, {
        status: 'error',
        message: String(error?.message ?? error),
      });
    options.report('error', { example: id, message: String(error?.message ?? error) });
  }

  try {
    const page = context(options.page, null);
    const stop = await bundle.environment?.setup?.(page);
    if (typeof stop === 'function') cleanups.push(stop);

    const figures = Array.from(root.querySelectorAll('[data-wb-example][data-status="ready"]'));
    for (const figure of figures) {
      if (controller.signal.aborted) break;
      const id = figure.dataset.wbExample ?? '';
      const stage = figure.querySelector('[data-wb-example-stage]');
      results.push({ id, status: 'mounted' });
      try {
        if (!stage) throw new Error('The panel for “' + id + '” has nowhere to mount.');
        if (
          !Object.prototype.hasOwnProperty.call(bundle.examples, id) ||
          bundle.examples[id] === undefined
        ) {
          throw new Error('The lens no longer exports “' + id + '”. Reload the page.');
        }
        const unmount = await bundle.adapter.mount(
          stage,
          bundle.examples[id],
          context(id, figure),
          bundle.environment,
        );
        if (typeof unmount === 'function') cleanups.push(unmount);
      } catch (error) {
        if (!controller.signal.aborted) fail(figure, id, error);
      }
    }

    if (controller.signal.aborted) return { results, dispose };
    options.phase?.('examples-mounted');
    await bundle.environment?.ready?.(page);
    options.phase?.('readiness-environment');
    if (document.fonts) await untilAborted(document.fonts.ready, controller.signal);
    options.phase?.('readiness-fonts');
    await visibleImages(3000, controller.signal);
    options.phase?.('readiness-images');

    return { results, dispose };
  } catch (error) {
    await dispose();
    throw error;
  }
}

function untilAborted(work, signal) {
  if (signal.aborted) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const done = () => {
      signal.removeEventListener('abort', done);
      resolve();
    };
    signal.addEventListener('abort', done, { once: true });
    Promise.resolve(work).then(done, (error) => {
      signal.removeEventListener('abort', done);
      reject(error);
    });
  });
}

/* Images in view, or eagerly loading, up to `limit` milliseconds; lazy images
   below the fold load as they would on any page. */
function visibleImages(limit, signal) {
  const pending = Array.from(document.images).filter((image) => {
    if (image.complete) return false;
    if (image.loading !== 'lazy') return true;
    const rect = image.getBoundingClientRect();
    return (
      rect.bottom > 0 &&
      rect.right > 0 &&
      rect.top < window.innerHeight &&
      rect.left < window.innerWidth
    );
  });
  if (!pending.length || signal.aborted) return Promise.resolve();
  return new Promise((resolve) => {
    const remaining = new Set(pending);
    const finish = () => {
      clearTimeout(timer);
      signal.removeEventListener('abort', finish);
      for (const image of pending) {
        image.removeEventListener('load', loaded);
        image.removeEventListener('error', loaded);
      }
      resolve();
    };
    const loaded = (event) => {
      remaining.delete(event.currentTarget);
      if (!remaining.size) finish();
    };
    const timer = setTimeout(finish, limit);
    signal.addEventListener('abort', finish, { once: true });
    for (const image of pending) {
      image.addEventListener('load', loaded, { once: true });
      image.addEventListener('error', loaded, { once: true });
    }
  });
}
