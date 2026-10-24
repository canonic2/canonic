/* The address bar
   ---------------
   The selection lives in the URL hash, so a reload or a copied link lands on
   the same page, in the same scenario, at the same size, through the same
   lens:

     #pages/sign-in.html:error@laptop~staging
        └ src        └ scenario └ size └ lens

     #docs/card.md!with-custom-style~native
        └ src      └ example         └ lens

   Every part after the src is optional. The default scenario and the design lens
   are both addressed by leaving them out, so the hash for a plain page reads
   as it always did: `#pages/sign-in.html@fit`. A docs page fills the canvas,
   so it has no size; it may name one of its examples instead.

   Parsed from the right: the lens is after the last `~`, the size after the
   last `@`, the example after the last `!`, the scenario after the first `:`. No
   src holds `:`, `!`, or `~` — config.js refuses one that does — so the split
   is unambiguous. Nothing here is validated: the shell checks the size
   against the page’s sizes, the scenario against the page, the lens against the
   config. */
(function () {
  function parse(hash) {
    var raw = String(hash || '');
    if (raw.charAt(0) === '#') raw = raw.slice(1);
    try {
      raw = decodeURIComponent(raw);
    } catch (e) {
      /* An address somebody mangled by hand; read it as it is. */
    }

    var lens = null;
    var size = null;
    var example = null;
    var scenario = null;

    var tilde = raw.lastIndexOf('~');
    if (tilde > -1) {
      lens = raw.slice(tilde + 1) || null;
      raw = raw.slice(0, tilde);
    }
    var at = raw.lastIndexOf('@');
    if (at > -1) {
      size = raw.slice(at + 1) || null;
      raw = raw.slice(0, at);
    }
    var bang = raw.lastIndexOf('!');
    if (bang > -1) {
      example = raw.slice(bang + 1) || null;
      raw = raw.slice(0, bang);
    }
    var colon = raw.indexOf(':');
    if (colon > -1) {
      scenario = raw.slice(colon + 1) || null;
      raw = raw.slice(0, colon);
    }

    return { src: raw, scenario: scenario, example: example, size: size, lens: lens };
  }

  /* An artboard page always writes its size; a docs page, which has none,
     leaves it out. The scenario, example, and lens appear only when they say
     something. No src, no address. */
  function write(target) {
    if (!target || !target.src) return '';
    return target.src +
      (target.scenario ? ':' + target.scenario : '') +
      (target.example ? '!' + target.example : '') +
      (target.size ? '@' + target.size : '') +
      (target.lens ? '~' + target.lens : '');
  }

  window.wbAddress = { parse: parse, write: write };
})();
