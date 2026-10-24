// Shared runtime used verbatim in development and portable browser exports.

// The project's environment (from workbench.config.ts) around a preview's
// own: its providers wrap outside, and its hooks run first and clean up last.
export function combine(outer, inner) {
  if (!outer || !Object.keys(outer).length) return inner || {};
  if (!inner || !Object.keys(inner).length) return outer;
  const both = [outer, inner];
  const has = name => both.some(environment => environment[name]);
  const staged = name => async (...args) => {
    const stops = [];
    for (const environment of both) {
      const stop = await environment[name]?.(...args);
      if (typeof stop === 'function') stops.push(stop);
    }
    if (stops.length) return async () => { for (const stop of stops.reverse()) await stop(); };
  };
  // Hooks a custom adapter defines for itself come through as they are, the
  // preview's ahead of the project's.
  const combined = { ...outer, ...inner };
  if (has('setup')) combined.setup = staged('setup');
  if (has('mount')) combined.mount = staged('mount');
  if (has('ready')) combined.ready = async context => { for (const environment of both) await environment.ready?.(context); };
  if (has('configure')) combined.configure = async (app, context) => { for (const environment of both) await environment.configure?.(app, context); };
  if (has('wrap')) combined.wrap = (tree, context) => {
    if (inner.wrap) tree = inner.wrap(tree, context);
    return outer.wrap ? outer.wrap(tree, context) : tree;
  };
  return combined;
}

export async function boot(preview, sources, adapter, environment, options) {
  let cleanup = [];
  let controller;
  let pending = Promise.resolve();
  const initial = new URL(location.href).searchParams.get('scenario') || Object.keys(preview.scenarios || {})[0] || 'default';
  const scenarios = preview.scenarios && Object.keys(preview.scenarios).length ? preview.scenarios : { default: {} };
  // An instance shows one artboard and iteration of whichever scenario it
  // renders; both are looked up in that scenario, so a scenario that does not
  // declare them renders an error naming them.
  const artboardId = options?.artboard || new URL(location.href).searchParams.get('artboard') || null;
  const iterationId = options?.iteration || new URL(location.href).searchParams.get('iteration') || null;
  const own = (map, key) => !!map && Object.prototype.hasOwnProperty.call(map, key);
  const position = iterationId ? { iteration: iterationId } : {};
  let selected = initial;
  let context;
  let actions = [];
  let loadStarted;
  let phaseStarted;
  let stopped = false;
  let lastOverrides = {};
  const listeners = [];
  let stopping;
  const tracing = options?.development && new URL(location.href).searchParams.has('wbTrace');
  function listen(name, handler) {
    window.addEventListener(name, handler);
    listeners.push([name, handler]);
  }
  function shutdown() {
    if (stopping) return stopping;
    stopped = true;
    controller?.abort();
    listeners.forEach(([name, handler, target = window]) => target.removeEventListener(name, handler, target !== window));
    if (window.wbPreviewActions?.follow === follow) window.wbPreviewActions.follow = null;
    window.__workbenchRequests?.deactivate();
    stopping = (async () => {
      await pending.catch(() => {});
      await dispose();
      canvas.remove();
      if (window.workbench === context) window.workbench = null;
    })();
    return stopping;
  }
  window.__workbenchStop = shutdown;
  function diagnostic(phase, details = {}) {
    if (!tracing) return;
    const trace = new URL(location.href).searchParams.get('wbTrace');
    if (!options?.development || !trace) return;
    const now = Date.now();
    try {
      fetch('/_workbench/log', { method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ level: 'info', event: 'preview.browser.phase',
          details: { id: preview.id, owner: 'preview-runtime', page: new URL(location.href).searchParams.get('wbPage') || decodeURIComponent(location.pathname.slice(1)), selection: trace, lens: new URL(location.href).searchParams.get('wbLens') || 'design', scenario: selected, ...position, phase, elapsedMs: now - loadStarted,
            phaseMs: now - phaseStarted, ...details } }), keepalive: true }).catch(() => {});
    } catch (_) { /* Loading must not depend on diagnostics. */ }
    phaseStarted = now;
  }
  const canvas = document.createElement('main');
  canvas.id = 'workbench-preview';
  document.body.append(canvas);
  async function whileActive(promise, signal) {
    if (signal.aborted) return;
    let aborted;
    const cancelled = new Promise(resolve => {
      aborted = resolve;
      signal.addEventListener('abort', aborted, { once: true });
    });
    try { return await Promise.race([promise, cancelled]); }
    finally { signal.removeEventListener('abort', aborted); }
  }
  const text = value => { try { return typeof value === 'string' ? value : JSON.stringify(value); } catch (_) { return String(value); } };
  function report(type, detail) {
    window.dispatchEvent(new CustomEvent('workbench:' + type, { detail }));
    if (parent !== window) parent.postMessage({ type: 'workbench-preview', event: type, id: preview.id, ...detail }, location.origin);
  }
  // A preview is a mock of its page, so it never leaves through the browser.
  // With actions on, a link or form whose address is a key of `links` opens
  // that preview on the canvas; anything else — another route, another site, a
  // download — is logged as an action instead. With actions off, actions.js
  // has already stopped it.
  const address = href => {
    try { const url = new URL(href, document.baseURI); return url.origin + url.pathname + url.search; } catch (_) { return null; }
  };
  function linked(href) {
    const where = address(href);
    if (!where) return null;
    for (const [key, to] of Object.entries(preview.links || {})) if (address(key) === where) return to;
    return null;
  }
  function navigate(rendered, to) {
    if (rendered.signal.aborted || window.wbPreviewActions?.on?.() === false) return;
    const target = typeof to === 'string' ? { preview: to, scenario: null } : { preview: to?.preview || preview.id, scenario: to?.scenario || null };
    if (parent === window) rendered.action('navigate', target.preview + (target.scenario ? '?scenario=' + target.scenario : ''));
    else report('navigate', target);
  }
  function follow(href, kind, element) {
    const rendered = context;
    if (stopped || !rendered || rendered.signal.aborted) return false;
    const to = linked(href);
    if (to) navigate(rendered, to);
    else if (kind === 'submit') {
      const fields = Object.fromEntries(Array.from(new FormData(element), ([name, value]) => [name, typeof value === 'string' ? value : value.name]));
      rendered.action('submit', ...(href ? [href] : []), fields);
    } else rendered.action('navigate', href);
    return true;
  }
  if (window.wbPreviewActions) window.wbPreviewActions.follow = follow;
  else {
    // A portable export has no actions.js and no switch: it behaves as on.
    const claim = (event, href, kind, element) => { if (follow(href, kind, element)) event.preventDefault(); };
    const listen = (name, handler) => { document.addEventListener(name, handler, true); listeners.push([name, handler, document]); };
    listen('click', event => {
      const link = event.composedPath().find(element => element.tagName === 'A' && element.hasAttribute('href'));
      const href = link?.getAttribute('href');
      if (link && !href.startsWith('#')) claim(event, href, 'link', link);
    });
    listen('submit', event => { if (event.target.tagName === 'FORM') claim(event, event.target.getAttribute('action') || '', 'submit', event.target); });
  }
  async function dispose() {
    controller?.abort();
    const stopping = cleanup;
    cleanup = [];
    let failure;
    for (const stop of stopping.reverse()) { try { await stop(); } catch (error) { failure ||= error; } }
    if (failure) throw failure;
  }
  async function fail(error) {
    diagnostic('error', { message: String(error?.message || error) });
    window.__workbenchError = String(error?.stack || error);
    window.__workbenchReady = false;
    await dispose().catch(() => {});
    canvas.replaceChildren(Object.assign(document.createElement('pre'), { textContent: window.__workbenchError }));
    report('error', { scenario: selected, ...position, message: String(error?.message || error) });
  }
  async function render(scenario, overrides = {}) {
    if (stopped) return;
    if (!Object.prototype.hasOwnProperty.call(scenarios, scenario)) throw new Error('Unknown preview scenario: ' + scenario);
    window.__workbenchReady = false;
    window.__workbenchError = null;
    loadStarted = phaseStarted = Date.now();
    diagnostic('started');
    await dispose();
    canvas.replaceChildren();
    selected = scenario;
    lastOverrides = overrides;
    const spec = scenarios[scenario];
    if (artboardId && !own(spec.artboards, artboardId)) throw new Error('Unknown preview artboard: ' + artboardId);
    const artboard = artboardId ? spec.artboards[artboardId] : null;
    // A declared artboard holds its own iterations; a scenario without
    // declared artboards holds those of its one implicit artboard.
    const iterations = artboard ? artboard.iterations : spec.iterations;
    if (iterationId && !own(iterations, iterationId)) throw new Error('Unknown preview iteration: ' + iterationId);
    const iteration = iterationId ? iterations[iterationId] : null;
    controller = new AbortController();
    const rendered = context = {
      id: preview.id, scenario, ...(artboardId ? { artboard: artboardId } : {}), ...position,
      inputs: structuredClone({ ...preview.inputs, ...spec.inputs, ...artboard?.inputs, ...iteration?.inputs, ...overrides }),
      fixtures: structuredClone({ ...preview.fixtures, ...spec.fixtures, ...iteration?.fixtures }),
      globals: structuredClone({ ...preview.globals, ...spec.globals, ...iteration?.globals }),
      signal: controller.signal,
      error: error => {
        if (context !== rendered || rendered.signal.aborted) return;
        window.__workbenchError = String(error?.stack || error);
        window.__workbenchReady = false;
        controller.abort();
        pending = pending.then(() => { if (context === rendered) return fail(error); }).catch(fail);
      },
      action: (name, ...values) => {
        if (rendered.signal.aborted) return;
        const entry = { name, values: values.map(text) };
        actions.push(entry);
        actions = actions.slice(-30);
        report('action', { ...entry, scenario, ...position });
      },
      navigate: to => navigate(rendered, to),
    };
    window.workbench = context;
    // Mocks match from the most specific level: iteration, scenario, preview.
    window.__workbenchRequests?.activate([...(iteration ? [iteration.requests] : []), spec.requests, preview.requests], rendered, rendered.action, rendered.signal);
    document.documentElement.dataset.wbScenario = scenario;
    if (iterationId) document.documentElement.dataset.wbIteration = iterationId;
    else delete document.documentElement.dataset.wbIteration;
    for (const setup of [environment?.setup, preview.setup, spec.setup, iteration?.setup]) {
      if (setup) {
        const stop = await setup(context);
        if (rendered.signal.aborted) { if (typeof stop === 'function') await stop(); return; }
        if (typeof stop === 'function') cleanup.push(stop);
      }
    }
    // An iteration with its own source has a key naming its artboard when the
    // artboard is declared; then a declared artboard's own render (Astro
    // pre-renders one as `<scenario>+<artboard>/`), the scenario's source and
    // the default.
    const artboardKey = artboardId && scenario + '+' + artboardId + '/';
    const iterationKey = iterationId && (artboardKey || scenario + '+') + iterationId;
    const key = [iterationKey, artboardKey, scenario].find(name => name && own(sources, name));
    const source = key ? sources[key] : sources.default;
    diagnostic('setup.completed');
    if (source === undefined) throw new Error('The selected source export does not exist for ' + preview.id + ' — ' + (iterationKey || scenario));
    const stop = await adapter.mount(canvas, source, context, environment);
    if (rendered.signal.aborted) { if (typeof stop === 'function') await stop(); return; }
    if (typeof stop === 'function') cleanup.push(stop);
    window.wbPreviewActions?.configure(location.search || new URL(location.href).search);
    diagnostic('mount.completed');
    // Workbench mounts into a hidden spare iframe. Browsers can suspend its
    // animation frames until promotion, which itself waits for readiness.
    // Yield to paint when available, but never make hidden-frame readiness
    // depend on a paint that cannot happen yet.
    if (preview.play || spec.play || iteration?.play || environment?.ready || preview.ready || spec.ready || iteration?.ready) {
      await new Promise(resolve => {
        const timer = setTimeout(resolve, 100);
        requestAnimationFrame(() => requestAnimationFrame(() => { clearTimeout(timer); resolve(); }));
      });
    }
    for (const play of [preview.play, spec.play, iteration?.play]) if (play) await play({ ...context, canvas });
    for (const ready of [environment?.ready, preview.ready, spec.ready, iteration?.ready]) if (ready) await ready(context);
    diagnostic('hooks.completed');
    // Fonts the page's text starts loading count too, so this waits on the
    // font set's ready promise rather than a status read once.
    const fontStatus = document.fonts?.status || 'unavailable';
    if (document.fonts) await whileActive(document.fonts.ready, rendered.signal);
    if (rendered.signal.aborted) return;
    diagnostic('fonts.completed', { fontStatus });
    const images = Array.from(document.images).filter(image => {
      if (image.loading !== 'lazy' || image.complete) return true;
      const rect = image.getBoundingClientRect();
      return rect.bottom > 0 && rect.right > 0 && rect.top < window.innerHeight && rect.left < window.innerWidth;
    });
    const pendingImages = images.filter(image => !image.complete).length;
    await whileActive(Promise.race([Promise.all(images.map(image => image.complete ? Promise.resolve() :
      new Promise(resolve => { image.addEventListener('load', resolve, { once: true }); image.addEventListener('error', resolve, { once: true }); }))),
      new Promise(resolve => setTimeout(resolve, 3000))]), rendered.signal);
    if (rendered.signal.aborted) return;
    diagnostic('images.completed', { pendingImages, deferredImages: document.images.length - images.length, stillPending: images.filter(image => !image.complete).length });
    window.__workbenchReady = true;
    diagnostic('ready');
    report('ready', { scenario, ...position, inputs: context.inputs, controls: preview.controls || {}, docs: preview.docs || '', actions });
  }
  listen('message', event => {
    if (stopped) return;
    if (event.source !== parent || event.origin !== location.origin || event.data?.type !== 'workbench-preview-command') return;
    const data = event.data;
    if (data.command === 'inspect') {
      if (context && window.__workbenchReady) report('ready', { scenario: selected, ...position, inputs: context.inputs, controls: preview.controls || {}, docs: preview.docs || '', actions });
      return;
    }
    pending = pending.then(() => {
      if (data.command === 'reset') actions = [];
      // A scenario command comes from the address: no scenario there is the first one.
      if (data.command === 'scenario') return render(data.scenario || Object.keys(scenarios)[0], {});
      return render(data.scenario || selected, data.inputs || {});
    }).catch(fail);
  });
  listen('pagehide', () => { void shutdown().catch(() => {}); });
  listen('error', event => { if (event.message) context?.error(event.error || new Error(event.message)); });
  listen('unhandledrejection', event => context?.error(event.reason));
  // A hot update to a source that does not refresh itself re-renders the
  // selected scenario with its inputs, without reloading the document. Edits
  // arrive pushed from the server; nothing here polls.
  window.__workbenchRerender = next => {
    if (stopped) return;
    if (next) Object.assign(sources, next);
    pending = pending.then(() => render(selected, lastOverrides)).catch(fail);
  };
  pending = Promise.resolve().then(() => render(initial)).catch(fail);
  await pending;
  return { dispose: shutdown };
}
