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

  function context(id, figure) {
    return {
      id,
      state: options.state,
      inputs: {},
      fixtures: {},
      globals: {},
      signal: controller.signal,
      error: (error) => {
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
      fail(figure, id, error);
    }
  }

  await bundle.environment?.ready?.(page);
  if (document.fonts) await document.fonts.ready;
  await visibleImages(3000);

  return {
    results,
    async dispose() {
      controller.abort();
      let failure;
      for (const cleanup of cleanups.reverse()) {
        try {
          await cleanup();
        } catch (error) {
          failure ??= error;
        }
      }
      if (failure) throw failure;
    },
  };
}

/* Images in view, or eagerly loading, up to `limit` milliseconds; lazy images
   below the fold load as they would on any page. */
function visibleImages(limit) {
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
  const loaded = Promise.all(
    pending.map(
      (image) =>
        new Promise((resolve) => {
          image.addEventListener('load', () => resolve(), { once: true });
          image.addEventListener('error', () => resolve(), { once: true });
        }),
    ),
  );
  return Promise.race([
    loaded.then(() => undefined),
    new Promise((resolve) => setTimeout(resolve, limit)),
  ]);
}
