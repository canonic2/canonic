/** Opt-in browser diagnostics. No host imports or timing/network work while disabled. */
export function enabled() {
  return (
    typeof document !== 'undefined' &&
    (document.querySelector?.('meta[name="workbench-trace"]')?.content === 'true' ||
      (typeof location !== 'undefined' && new URLSearchParams(location.search).has('wbTrace')))
  );
}

export function begin(identity, context, owner = 'canvas') {
  if (!enabled()) return null;
  const now = performance.timeOrigin + performance.now();
  return {
    selection: context?.selection || crypto.randomUUID(),
    started: context?.started || now,
    previous: now,
    owner,
    page: identity?.src || identity?.page,
    state: identity?.state || 'default',
    lens: identity?.lens?.key || identity?.lens || 'design',
  };
}

export function phase(trace, name, extra = {}) {
  if (!trace) return;
  const now = performance.timeOrigin + performance.now();
  const details = {
    selection: trace.selection,
    owner: trace.owner,
    page: trace.page,
    state: trace.state,
    lens: trace.lens,
    phase: name,
    durationMs: now - trace.previous,
    elapsedMs: now - trace.started,
    ...extra,
  };
  trace.previous = now;
  void fetch('/_workbench/log', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ level: 'info', event: 'preview.browser.phase', details }),
  }).catch(() => {
    /* Diagnostics never interrupt navigation. */
  });
}

export function address(value, trace) {
  if (!trace) return value;
  const url = new URL(value, location.href);
  if (url.origin !== location.origin) return value;
  url.searchParams.set('wbTrace', trace.selection);
  if (trace.page) url.searchParams.set('wbPage', trace.page);
  url.searchParams.set('wbState', trace.state);
  url.searchParams.set('wbLens', trace.lens);
  return url.href;
}

export function documentTrace(identity) {
  if (!enabled()) return null;
  const params = new URLSearchParams(location.search);
  identity = {
    ...identity,
    src: params.get('wbPage') || identity?.src || identity?.page,
    state: params.get('wbState') || identity?.state,
    lens: params.get('wbLens') || identity?.lens,
  };
  return begin(
    identity,
    { selection: new URLSearchParams(location.search).get('wbTrace') },
    'document',
  );
}
