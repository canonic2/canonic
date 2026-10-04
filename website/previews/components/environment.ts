// Around every component example. Base.astro marks <html> with `js` before
// the page renders, and the site's CSS shows Copy buttons and captions only
// then; the docs page has no Base, so this marks it the same way. The docs
// sidebar's search answers with the sample guides, or fails inside
// [data-search="unavailable"], whichever sidebar's input has focus.
import { searchIndex } from './fixtures';

export function setup() {
  document.documentElement.classList.add('js');
  const original = window.fetch;
  window.fetch = async (input, init) => {
    const url = new URL(input instanceof Request ? input.url : String(input), location.href);
    if (!url.pathname.endsWith('/workbench/docs/search.json')) return original(input, init);
    if (document.activeElement?.closest('[data-search="unavailable"]')) return new Response('', { status: 503 });
    return Response.json(searchIndex);
  };
  return () => { window.fetch = original; };
}
