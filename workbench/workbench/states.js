/* Page states — the preview's half
   --------------------------------
   A page is rarely one picture. Sign in has the empty form you land on and
   the one that comes back saying the password was wrong; a list has rows and
   has nothing yet. Those are states of the same page, and the workbench lists
   them under it the way Storybook lists stories under a component.

   The workbench decides which state is showing; this decides what that means
   inside the page. It has to be the page's job for the same reason as
   actions.js: off file:// the preview is a foreign origin, so the shell can't
   reach in. The state rides in on the URL instead.

     ?state=error   the page renders its "error" state
     no parameter   "default" — the page as authored

   The state is declared twice: once in workbench.yaml, which is what
   puts the row in the sidebar, and once in the page, which is what the row
   shows. The ids have to match.

   Three ways for a page to answer, smallest first:

   1. CSS, off the root attribute. This file sets data-wb-state on <html>, so
      any rule can key off it:

        html[data-wb-state="error"] .auth-title { color: var(--color-red); }

   2. Markup that only exists in some states:

        <p class="auth-alert" data-wb-state-only="error">…</p>
        <p class="auth-hint"  data-wb-state-not="error locked">…</p>

      Space-separated ids. "only" keeps the element in those states and
      removes it everywhere else; "not" removes it in those states. Removed,
      not hidden — an element that isn't in the state shouldn't take up space
      or turn up in a screenshot.

   3. Attributes applied in one state, for the times the difference is a value
      or a flag on an element that's there either way:

        <input data-wb-set-error="value=user@example.com">
        <button data-wb-set-sending="disabled">

      Semicolons between them, name=value or a bare name for a boolean.

   Like actions.js, this is part of the compatibility bundle put into the page
   by the extension's server rather than referenced by the project.

   It runs at DOMContentLoaded, so custom elements (loaded with defer) are
   already upgraded and take the attributes normally.
*/
(function () {
  /* Ids are kebab-case: they go in a URL, in a filename for the screenshot,
     and — below — straight into a selector. Anything else is somebody's
     typo, and the default state is the safe thing to show. */
  var flag = /[?&]state=([^&]*)/.exec(location.search);
  var raw = flag ? decodeURIComponent(flag[1]) : '';
  var state = /^[a-z0-9-]+$/.test(raw) ? raw : 'default';

  document.documentElement.setAttribute('data-wb-state', state);

  function ids(value) {
    return (value || '').split(/\s+/).filter(Boolean);
  }

  function each(selector, fn) {
    Array.prototype.forEach.call(document.querySelectorAll(selector), fn);
  }

  function apply(next) {
    if (typeof next === 'string') state = /^[a-z0-9-]+$/.test(next) ? next : 'default';
    document.documentElement.setAttribute('data-wb-state', state);
    each('[data-wb-state-only]', function (el) {
      if (ids(el.getAttribute('data-wb-state-only')).indexOf(state) === -1) el.remove();
    });

    each('[data-wb-state-not]', function (el) {
      if (ids(el.getAttribute('data-wb-state-not')).indexOf(state) > -1) el.remove();
    });

    /* Only the active state's attribute is read — the others stay in the
       markup as documentation of what this element does elsewhere. */
    each('[data-wb-set-' + state + ']', function (el) {
      el.getAttribute('data-wb-set-' + state).split(';').forEach(function (pair) {
        var text = pair.trim();
        if (!text) return;
        var eq = text.indexOf('=');
        var name = (eq === -1 ? text : text.slice(0, eq)).trim();
        if (name) el.setAttribute(name, eq === -1 ? '' : text.slice(eq + 1).trim());
      });
    });
  }
  window.wbPageStates = { apply: apply };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', apply);
  } else {
    apply();
  }
})();
