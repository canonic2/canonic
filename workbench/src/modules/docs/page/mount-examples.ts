/* Mounts a docs page's examples, each into its own panel, through the lens's
   adapter and environment, the way a preview mounts its source. One
   example's failure stays in its panel. The page is ready once every
   example has mounted, the environment's ready hook has run, and fonts and
   visible images have loaded. */

type Cleanup = void | (() => void | Promise<void>);

export interface ExampleContext {
  id: string;
  state: string;
  inputs: Record<string, unknown>;
  fixtures: Record<string, unknown>;
  globals: Record<string, unknown>;
  signal: AbortSignal;
  error(error: unknown): void;
  action(name: string, ...values: unknown[]): void;
  navigate(to: unknown): void;
}

interface Environment {
  setup?(context: ExampleContext): Cleanup | Promise<Cleanup>;
  ready?(context: ExampleContext): void | Promise<void>;
  [hook: string]: unknown;
}

/** What a lens's compiled bundle exports. */
export interface ExamplesBundle {
  adapter: { mount(canvas: HTMLElement, source: unknown, context: ExampleContext, environment?: unknown): Cleanup | Promise<Cleanup> };
  environment: Environment;
  examples: Record<string, unknown>;
}

export interface ExampleResult {
  id: string;
  status: 'mounted' | 'error';
  message?: string;
}

export interface MountOptions {
  page: string;
  state: string;
  /** Sends an event to the canvas: `action`, `navigate`, or `error`. */
  report(type: string, detail: Record<string, unknown>): void;
}

export interface MountedExamples {
  results: ExampleResult[];
  dispose(): Promise<void>;
}

const text = (value: unknown): string => {
  try { return typeof value === 'string' ? value : JSON.stringify(value); } catch { return String(value); }
};

export async function mountExamples(root: ParentNode, bundle: ExamplesBundle, options: MountOptions): Promise<MountedExamples> {
  const controller = new AbortController();
  const cleanups: (() => void | Promise<void>)[] = [];
  const results: ExampleResult[] = [];

  function context(id: string, figure: HTMLElement | null): ExampleContext {
    return {
      id, state: options.state, inputs: {}, fixtures: {}, globals: {},
      signal: controller.signal,
      error: error => { if (figure) fail(figure, id, error); else options.report('error', { message: String((error as Error)?.message ?? error) }); },
      action: (name, ...values) => {
        if (!controller.signal.aborted) options.report('action', { name, values: values.map(text), example: id });
      },
      navigate: to => { if (!controller.signal.aborted) options.report('navigate', { to, example: id }); },
    };
  }

  function fail(figure: HTMLElement, id: string, error: unknown): void {
    const message = String((error as Error)?.stack ?? error);
    figure.dataset.status = 'error';
    const stage = figure.querySelector('[data-wb-example-stage]');
    const shown = Object.assign(document.createElement('pre'), { className: 'wb-docs-example-error', textContent: message });
    if (stage) stage.replaceWith(shown); else figure.prepend(shown);
    const result = results.find(entry => entry.id === id);
    if (result) Object.assign(result, { status: 'error', message: String((error as Error)?.message ?? error) });
    options.report('error', { example: id, message: String((error as Error)?.message ?? error) });
  }

  const page = context(options.page, null);
  const stop = await bundle.environment?.setup?.(page);
  if (typeof stop === 'function') cleanups.push(stop);

  const figures = Array.from(root.querySelectorAll<HTMLElement>('[data-wb-example][data-status="ready"]'));
  for (const figure of figures) {
    if (controller.signal.aborted) break;
    const id = figure.dataset.wbExample ?? '';
    const stage = figure.querySelector<HTMLElement>('[data-wb-example-stage]');
    results.push({ id, status: 'mounted' });
    try {
      if (!stage) throw new Error('The panel for “' + id + '” has nowhere to mount.');
      if (!Object.prototype.hasOwnProperty.call(bundle.examples, id) || bundle.examples[id] === undefined) {
        throw new Error('The lens no longer exports “' + id + '”. Reload the page.');
      }
      const unmount = await bundle.adapter.mount(stage, bundle.examples[id], context(id, figure), bundle.environment);
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
      let failure: unknown;
      for (const cleanup of cleanups.reverse()) {
        try { await cleanup(); } catch (error) { failure ??= error; }
      }
      if (failure) throw failure;
    },
  };
}

/* Images in view, or eagerly loading, up to `limit` milliseconds; lazy images
   below the fold load as they would on any page. */
function visibleImages(limit: number): Promise<void> {
  const pending = Array.from(document.images).filter(image => {
    if (image.complete) return false;
    if (image.loading !== 'lazy') return true;
    const rect = image.getBoundingClientRect();
    return rect.bottom > 0 && rect.right > 0 && rect.top < window.innerHeight && rect.left < window.innerWidth;
  });
  const loaded = Promise.all(pending.map(image => new Promise<void>(resolve => {
    image.addEventListener('load', () => resolve(), { once: true });
    image.addEventListener('error', () => resolve(), { once: true });
  })));
  return Promise.race([loaded.then(() => undefined), new Promise<void>(resolve => setTimeout(resolve, limit))]);
}
