/* Page scenarios — the preview's half
   -----------------------------------
   A page is rarely one picture. Sign in has the empty form you land on and
   the one that comes back saying the password was wrong; a list has rows and
   has nothing yet. Those are scenarios of the same page, and the workbench
   lists them under it the way Storybook lists stories under a component.

   The workbench decides which scenario is showing; this decides what that
   means inside the page. It has to be the page's job for the same reason as
   actions.js: off file:// the preview is a foreign origin, so the shell can't
   reach in. The scenario rides in on the URL instead.

     ?scenario=error   the page renders its "error" scenario
     no parameter      "default" — the page as authored

   The scenario is declared twice: once in workbench.yaml, which is what
   puts the row in the sidebar, and once in the page, which is what the row
   shows. The ids have to match.

   Three ways for a page to answer, smallest first:

   1. CSS, off the root attribute. This file sets data-wb-scenario on <html>,
      so any rule can key off it:

        html[data-wb-scenario="error"] .auth-title { color: var(--color-red); }

   2. Markup that only exists in some scenarios:

        <p class="auth-alert" data-wb-scenario-only="error">…</p>
        <p class="auth-hint"  data-wb-scenario-not="error locked">…</p>

      Space-separated ids. "only" keeps the element in those scenarios and
      removes it everywhere else; "not" removes it in those scenarios. Removed,
      not hidden — an element that isn't in the scenario shouldn't take up
      space or turn up in a screenshot.

   3. Attributes applied in one scenario, for the times the difference is a
      value or a flag on an element that's there either way:

        <input data-wb-set-error="value=user@example.com">
        <button data-wb-set-sending="disabled">

      Semicolons between them, name=value or a bare name for a boolean.

   A scenario can also list iterations in workbench.yaml: other takes on its
   design, kept beside it. The selected one rides in the same way:

     ?iteration=inline-hint   the page renders that iteration
     no parameter             no iteration — the design as authored

   It sets data-wb-iteration on <html> (absent with no iteration), and
   data-wb-iteration-only / data-wb-iteration-not keep or remove elements
   exactly as the scenario attributes do. An "only" element is removed when
   no iteration is selected.

   Like actions.js, this is part of the compatibility bundle put into the page
   by the extension's server rather than referenced by the project.

   It runs at DOMContentLoaded, so custom elements (loaded with defer) are
   already upgraded and take the attributes normally.
*/
(function () {
  /* Ids are kebab-case — a lowercase letter, then lowercase letters and
     digits in single-hyphen parts, the rule every scenario, artboard and
     iteration id follows: they go in a URL, in a filename for the
     screenshot, and — below — straight into a selector. Anything else is
     somebody's typo, and the default scenario (or the authored design) is
     the safe thing to show. */
  function param(name) {
    var flag = new RegExp('[?&]' + name + '=([^&]*)').exec(location.search);
    try { return flag ? decodeURIComponent(flag[1]) : ''; } catch (_) { return ''; }
  }
  function valid(value) {
    return typeof value === 'string' && /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(value);
  }
  var scenario = valid(param('scenario')) ? param('scenario') : 'default';
  /* An invalid iteration id shows the authored design. */
  var iteration = valid(param('iteration')) ? param('iteration') : null;

  function mark() {
    document.documentElement.setAttribute('data-wb-scenario', scenario);
    if (iteration) document.documentElement.setAttribute('data-wb-iteration', iteration);
    else document.documentElement.removeAttribute('data-wb-iteration');
  }
  mark();

  function ids(value) {
    return (value || '').split(/\s+/).filter(Boolean);
  }

  function each(selector, fn) {
    Array.prototype.forEach.call(document.querySelectorAll(selector), fn);
  }

  /* apply(scenario, iteration) re-applies a render's selection; apply() with
     no scenario keeps the one from the URL. */
  function apply(next, nextIteration) {
    if (typeof next === 'string') {
      scenario = valid(next) ? next : 'default';
      iteration = valid(nextIteration) ? nextIteration : null;
    }
    mark();
    each('[data-wb-scenario-only]', function (el) {
      if (ids(el.getAttribute('data-wb-scenario-only')).indexOf(scenario) === -1) el.remove();
    });

    each('[data-wb-scenario-not]', function (el) {
      if (ids(el.getAttribute('data-wb-scenario-not')).indexOf(scenario) > -1) el.remove();
    });

    each('[data-wb-iteration-only]', function (el) {
      if (!iteration || ids(el.getAttribute('data-wb-iteration-only')).indexOf(iteration) === -1) el.remove();
    });

    each('[data-wb-iteration-not]', function (el) {
      if (iteration && ids(el.getAttribute('data-wb-iteration-not')).indexOf(iteration) > -1) el.remove();
    });

    /* Only the active scenario's attribute is read — the others stay in the
       markup as documentation of what this element does elsewhere. */
    each('[data-wb-set-' + scenario + ']', function (el) {
      el.getAttribute('data-wb-set-' + scenario).split(';').forEach(function (pair) {
        var text = pair.trim();
        if (!text) return;
        var eq = text.indexOf('=');
        var name = (eq === -1 ? text : text.slice(0, eq)).trim();
        if (name) el.setAttribute(name, eq === -1 ? '' : text.slice(eq + 1).trim());
      });
    });
  }
  window.wbPageScenarios = { apply: apply };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', apply);
  } else {
    apply();
  }
})();
