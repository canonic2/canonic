// Shared runtime used verbatim in development and portable browser exports.
export async function boot(preview, sources, adapter, environment, options) {
  let cleanup = [];
  let controller;
  let pending = Promise.resolve();
  const initial = new URL(location.href).searchParams.get('state') || Object.keys(preview.states || {})[0] || 'default';
  const states = preview.states && Object.keys(preview.states).length ? preview.states : { default: {} };
  let selected = initial;
  let context;
  let actions = [];
  let loadStarted;
  let phaseStarted;
  let stopped = false;
  let revisionTimer;
  const listeners = [];
  let stopping;
  function listen(name, handler) {
    window.addEventListener(name, handler);
    listeners.push([name, handler]);
  }
  function shutdown() {
    if (stopping) return stopping;
    stopped = true;
    controller?.abort();
    clearInterval(revisionTimer);
    listeners.forEach(([name, handler]) => window.removeEventListener(name, handler));
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
    if (!options?.development) return;
    const now = Date.now();
    try {
      fetch('/_workbench/log', { method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ level: 'info', event: 'preview.browser.phase',
          details: { id: preview.id, state: selected, phase, elapsedMs: now - loadStarted,
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
    report('error', { state: selected, message: String(error?.message || error) });
  }
  async function render(state, overrides = {}) {
    if (stopped) return;
    if (!Object.prototype.hasOwnProperty.call(states, state)) throw new Error('Unknown preview state: ' + state);
    window.__workbenchReady = false;
    window.__workbenchError = null;
    loadStarted = phaseStarted = Date.now();
    diagnostic('started');
    await dispose();
    canvas.replaceChildren();
    selected = state;
    const spec = states[state];
    controller = new AbortController();
    const rendered = context = {
      id: preview.id, state,
      inputs: structuredClone({ ...preview.inputs, ...spec.inputs, ...overrides }),
      fixtures: structuredClone({ ...preview.fixtures, ...spec.fixtures }),
      globals: structuredClone({ ...preview.globals, ...spec.globals }),
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
        report('action', { ...entry, state });
      },
    };
    window.workbench = context;
    document.documentElement.dataset.wbState = state;
    for (const setup of [environment?.setup, preview.setup, spec.setup]) {
      if (setup) {
        const stop = await setup(context);
        if (rendered.signal.aborted) { if (typeof stop === 'function') await stop(); return; }
        if (typeof stop === 'function') cleanup.push(stop);
      }
    }
    const source = Object.prototype.hasOwnProperty.call(sources, state) ? sources[state] : sources.default;
    diagnostic('setup.completed');
    if (source === undefined) throw new Error('The selected source export does not exist for ' + preview.id + ' — ' + state);
    const stop = await adapter.mount(canvas, source, context, environment);
    if (rendered.signal.aborted) { if (typeof stop === 'function') await stop(); return; }
    if (typeof stop === 'function') cleanup.push(stop);
    window.wbPreviewActions?.configure(location.search || new URL(location.href).search);
    diagnostic('mount.completed');
    // Workbench mounts into a hidden spare iframe. Browsers can suspend its
    // animation frames until promotion, which itself waits for readiness.
    // Yield to paint when available, but never make hidden-frame readiness
    // depend on a paint that cannot happen yet.
    if (preview.play || spec.play || environment?.ready || preview.ready || spec.ready) {
      await new Promise(resolve => {
        const timer = setTimeout(resolve, 100);
        requestAnimationFrame(() => requestAnimationFrame(() => { clearTimeout(timer); resolve(); }));
      });
    }
    for (const play of [preview.play, spec.play]) if (play) await play({ ...context, canvas });
    for (const ready of [environment?.ready, preview.ready, spec.ready]) if (ready) await ready(context);
    diagnostic('fonts.waiting', { fontStatus: document.fonts?.status || 'unavailable' });
    if (document.fonts) await whileActive(document.fonts.ready, rendered.signal);
    if (rendered.signal.aborted) return;
    const images = Array.from(document.images).filter(image => {
      if (image.loading !== 'lazy' || image.complete) return true;
      const rect = image.getBoundingClientRect();
      return rect.bottom > 0 && rect.right > 0 && rect.top < window.innerHeight && rect.left < window.innerWidth;
    });
    diagnostic('images.waiting', { pendingImages: images.filter(image => !image.complete).length,
      deferredImages: document.images.length - images.length });
    await whileActive(Promise.race([Promise.all(images.map(image => image.complete ? Promise.resolve() :
      new Promise(resolve => { image.addEventListener('load', resolve, { once: true }); image.addEventListener('error', resolve, { once: true }); }))),
      new Promise(resolve => setTimeout(resolve, 3000))]), rendered.signal);
    if (rendered.signal.aborted) return;
    window.__workbenchReady = true;
    diagnostic('ready', { pendingImages: images.filter(image => !image.complete).length });
    report('ready', { state, inputs: context.inputs, controls: preview.controls || {}, docs: preview.docs || '', actions });
  }
  listen('message', event => {
    if (stopped) return;
    if (event.source !== parent || event.origin !== location.origin || event.data?.type !== 'workbench-preview-command') return;
    const data = event.data;
    if (data.command === 'inspect') {
      if (context && window.__workbenchReady) report('ready', { state: selected, inputs: context.inputs, controls: preview.controls || {}, docs: preview.docs || '', actions });
      return;
    }
    pending = pending.then(() => {
      if (data.command === 'reset') actions = [];
      return render(data.state || selected, data.inputs || {});
    }).catch(fail);
  });
  listen('pagehide', () => { void shutdown().catch(() => {}); });
  listen('error', event => { if (event.message) context?.error(event.error || new Error(event.message)); });
  listen('unhandledrejection', event => context?.error(event.reason));
  pending = Promise.resolve().then(() => render(initial)).catch(fail);
  await pending;
  if (options?.development && !stopped) {
    let revision = options.revision;
    revisionTimer = setInterval(async () => {
      try {
        const answer = await fetch(options.revisionUrl, { cache: 'no-store' });
        const data = await answer.json();
        if (!stopped && revision !== data.revision) {
          clearInterval(revisionTimer);
          if (window.__workbenchHost) window.wbPreviewHost.load(location.href);
          else location.reload();
        }
      } catch (_) { /* The server can restart while the editor remains open. */ }
    }, 1000);
  }
  return { dispose: shutdown };
}
