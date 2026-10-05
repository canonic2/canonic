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
    const target = typeof to === 'string' ? { preview: to, state: null } : { preview: to?.preview || preview.id, state: to?.state || null };
    if (parent === window) rendered.action('navigate', target.preview + (target.state ? '?state=' + target.state : ''));
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
      navigate: to => navigate(rendered, to),
    };
    window.workbench = context;
    window.__workbenchRequests?.activate([spec.requests, preview.requests], rendered, rendered.action, rendered.signal);
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
