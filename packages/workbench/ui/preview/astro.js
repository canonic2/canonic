import { mount as mountHtml } from './html.js';

// Reference states are rendered at build time. Input edits ask the worker to
// render again; exported previews need no server to select reference states.
export async function mount(canvas, source, context, environment) {
  let html = source.html;
  if (JSON.stringify(context.inputs) !== JSON.stringify(source.inputs)) {
    if (!source.renderUrl) throw new Error('Astro input edits require the live Workbench server. Portable previews contain the authored states.');
    const url = new URL(source.renderUrl, location.href);
    url.searchParams.set('state', context.state);
    url.searchParams.set('inputs', JSON.stringify(context.inputs));
    const response = await fetch(url, { signal: context.signal });
    const answer = await response.json();
    if (!response.ok) throw new Error(answer.error || 'Astro rendering failed');
    const document = new DOMParser().parseFromString(answer.html, 'text/html');
    // An input variant has its own assets, while the frame keeps its original
    // address and base. Resolve only packaged assets against the variant.
    for (const element of document.querySelectorAll('[src],link[href],[poster],[srcset]')) {
      for (const attribute of ['src', 'href', 'poster']) {
        const value = element.getAttribute(attribute);
        if (value?.startsWith('./assets/')) element.setAttribute(attribute, new URL(value, new URL(answer.base, location.href)).href);
      }
      const srcset = element.getAttribute('srcset');
      if (srcset) element.setAttribute('srcset', srcset.replace(/\.\/assets\/[^\s,]+/g, value => new URL(value, new URL(answer.base, location.href)).href));
    }
    html = document.documentElement.outerHTML;
  }
  return mountHtml(canvas, html, context, environment);
}
