let revision = 0;
function loaded(element, context) {
  return new Promise((resolve, reject) => {
    const finish = error => {
      clearTimeout(timer);
      element.removeEventListener('load', ready);
      element.removeEventListener('error', failed);
      context.signal.removeEventListener('abort', aborted);
      if (error) reject(error); else resolve();
    };
    const ready = () => finish();
    const failed = () => finish(new Error('Could not load HTML resource: ' + (element.src || element.href)));
    const aborted = () => finish();
    const timer = setTimeout(() => finish(new Error('HTML resource load timed out: ' + (element.src || element.href))), 5000);
    element.addEventListener('load', ready, { once: true });
    element.addEventListener('error', failed, { once: true });
    context.signal.addEventListener('abort', aborted, { once: true });
    if (context.signal.aborted) aborted();
  });
}
export async function mount(canvas, source, context, environment) {
  if (typeof source === 'string') {
    const parsed = new DOMParser().parseFromString(source, 'text/html');
    const attributes = [];
    for (const [target, incoming] of [[document.documentElement, parsed.documentElement], [document.body, parsed.body]]) {
      for (const attribute of incoming.attributes) {
        attributes.push([target, attribute.name, target.getAttribute(attribute.name)]);
        target.setAttribute(attribute.name, attribute.value);
      }
    }
    const restore = () => attributes.forEach(([target, name, value]) => {
      if (value === null) target.removeAttribute(name); else target.setAttribute(name, value);
    });
    canvas.innerHTML = parsed.head.innerHTML + parsed.body.innerHTML;
    window.wbPageStates?.apply(context.state);
    window.wbPreviewActions?.apply();
    try {
      await Promise.all(Array.from(canvas.querySelectorAll('link[rel="stylesheet"]'))
        .filter(link => !link.sheet && !link.disabled).map(link => loaded(link, context)));
      const scripts = Array.from(canvas.querySelectorAll('script'));
      for (const script of scripts) {
        if (context.signal.aborted) return restore;
        const live = document.createElement('script');
        for (const attribute of script.attributes) live.setAttribute(attribute.name, attribute.value);
        live.textContent = script.textContent;
        if (live.src && live.type === 'module') {
          const url = new URL(live.src);
          url.searchParams.set('workbench-render', String(++revision));
          live.src = url.href;
        }
        const loading = live.src || live.type === 'module' ? loaded(live, context) : Promise.resolve();
        script.replaceWith(live);
        await loading;
      }
      if (context.signal.aborted) return restore;
      const cleanup = await environment?.mount?.(canvas, context);
      return async () => { try { if (typeof cleanup === 'function') await cleanup(); } finally { restore(); } };
    } catch (error) { restore(); throw error; }
  }
  if (typeof source !== 'function') throw new Error('An HTML JavaScript entry must export a mount function.');
  return source(canvas, context);
}
