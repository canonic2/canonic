/* Actions on/off — the preview's half
   -----------------------------------
   The workbench decides whether a preview is live; this decides what that
   means inside the page. It has to be the page's job: off file:// the preview
   is a foreign origin, so the shell can't reach in and stop anything. The flag
   rides in on the URL instead and every page polices itself.

     ?actions=off   links don't navigate, forms don't submit
     ?actions=on    the page behaves normally
     no parameter   normal — a file opened straight off disk is never
                    mysteriously inert

   Off is the workbench's default, so the shell always sends the parameter
   explicitly rather than relying on the absent case.

   Off means off, silently. A blocked click does nothing and says nothing —
   the top bar already shows why.

   What this does not touch: hover, press, and focus styling, pickers, code
   boxes, disclosure, links to a spot on the same page — everything that only
   changes how the page looks or what it holds. That is the design. Only the
   things that would take you off the page are stopped.

   A TypeScript preview is a mock, not a site: on, its links and forms still
   never reach the browser. The preview runtime claims them through `follow`
   and either opens another preview or logs them in the Actions panel — see
   preview/browser.js.

   Nothing in a project points at this file. The extension's server puts it
   into its compatibility bundle, first in <head>, ahead of the page's own scripts
   — see `withPreviewScripts` in server.js.
*/
(function () {
  var flag = /[?&]actions=(on|off)/.exec(location.search);
  var on = !flag || flag[1] === 'on';
  var actions = window.wbPreviewActions = {
    configure: function (search) {
      var next = /[?&]actions=(on|off)/.exec(search);
      on = !next || next[1] === 'on';
      document.documentElement.setAttribute('data-wb-actions', on ? 'on' : 'off');
      silenceForms();
    },
    apply: silenceForms,
    on: function () { return on; },
    /* Set by a preview while it is mounted: follow(href, kind, element)
       takes a link ('link') or a form ('submit') the page is about to leave
       by, and returns true once it has dealt with it. */
    follow: null,
  };

  /* A link to a spot on this page — "#tour" — moves within the page, so
     neither switch position stops it. It is scrolled to by hand: a preview
     host page carries a <base>, against which "#tour" would resolve to a
     different document altogether. A bare "#" is the placeholder link of a
     design page and goes nowhere. */
  function inPage(e, href) {
    if (!href || href.charAt(0) !== '#' || href.length < 2) return false;
    var id;
    try { id = decodeURIComponent(href.slice(1)); } catch (error) { id = href.slice(1); }
    var target = document.getElementById(id) || document.getElementsByName(id)[0];
    e.preventDefault();
    if (target) target.scrollIntoView();
    return true;
  }

  /* A live page sits one frame deeper than the workbench shell. Relay editor
     shortcuts now; navigation being on or off has no bearing on the editor. */
  if (window.wbKeys) window.wbKeys.relay(window.parent, {
    enabled: function () {
      try { return !!window.parent.wbEmbedded; } catch (error) { return false; }
    },
  });

  document.documentElement.setAttribute('data-wb-actions', on ? 'on' : 'off');

  /* Live destinations inside the project still belong to the workbench. Hand
     them to the parent so it can load them in its spare iframe and avoid the
     unstyled flash of native iframe navigation.

     This page has no idea where the project root is, which pages are in the
     sidebar, or what any of them are called — and shouldn't: that is the
     config's business, and this file is copied into projects that arrange
     themselves however they like. So the test here is only "could this be one
     of ours" — a page next door rather than another site — and the address is
     handed over whole for the workbench to recognise or ignore.

     Anything that isn't a candidate keeps its browser behaviour: other sites,
     same-page anchors, mailto:, downloads. postMessage works across file://
     origins; the parent validates both the sender and the destination. */
  function candidate(href) {
    if (!href || href.charAt(0) === '#') return null;
    var url;
    try {
      url = new URL(href, location.href);
    } catch (error) {
      return null;
    }
    if (url.protocol !== location.protocol) return null;
    /* file:// has no meaningful origin — every local file reads as null — so
       the scheme is the whole test there, and the parent does the rest. */
    if (url.protocol !== 'file:' && url.origin !== location.origin) return null;
    if (!/\.html?$/i.test(url.pathname)) return null;

    var state = url.searchParams.get('state') || '';
    return {
      href: url.href,
      state: /^[a-z0-9-]+$/.test(state) ? state : null,
    };
  }

  function handOff(e, target) {
    if (!target || window.parent === window) return;
    e.preventDefault();
    window.parent.postMessage({
      type: 'wb-preview-navigate',
      href: target.href,
      state: target.state,
    }, '*');
  }

  /* These pages are design, not a product: nothing here validates input, and
     nothing should stand between a click and the next page. Constraint
     validation would do exactly that — it runs before the submit event, so a
     single `required` on a field is enough to stop a flow dead, with a native
     browser bubble as the only clue. novalidate turns it off at the source,
     for live previews and blocked ones alike. requestSubmit(), which a custom
     submit button uses, honours it too. */
  function silenceForms() {
    Array.prototype.forEach.call(document.forms, function (form) {
      form.noValidate = true;
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', silenceForms);
  } else {
    silenceForms();
  }

  {
    document.addEventListener(
      'click',
      function (e) {
        var path = e.composedPath ? e.composedPath() : [e.target];
        for (var i = 0; i < path.length; i++) {
          var el = path[i];
          if (el.tagName !== 'A' || !el.hasAttribute('href')) continue;
          var href = el.getAttribute('href');
          if (inPage(e, href)) return;
          if (!on) { e.preventDefault(); return; }
          if (actions.follow && actions.follow(href, 'link', el)) { e.preventDefault(); return; }
          handOff(e, candidate(href));
          return;
        }
      },
      true
    );

    /* A form that names a real page is a step in a flow — "Send code" lands
       on Verify code — and it has to travel the same road as a link. Left to
       the browser it wouldn't travel at all: these forms POST, and a POST to
       a file:// URL goes nowhere. */
    document.addEventListener(
      'submit',
      function (e) {
        if (!on) { e.preventDefault(); return; }
        var form = e.target;
        if (!form || form.tagName !== 'FORM') return;
        if (actions.follow && actions.follow(form.getAttribute('action') || '', 'submit', form)) { e.preventDefault(); return; }
        handOff(e, candidate(form.getAttribute('action')));
      },
      true
    );
  }
})();
