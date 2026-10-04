/* The address bar
   ---------------
   The selection lives in the URL hash, so a reload or a copied link lands on
   the same page, in the same state, at the same width, through the same
   lens:

     #pages/sign-in.html:error@1512~staging
        └ src        └ state └ width └ lens

     #docs/card.md!with-custom-style~native
        └ src      └ example         └ lens

   Every part after the src is optional. The default state and the design lens
   are both addressed by leaving them out, so the hash for a plain page reads
   as it always did: `#pages/sign-in.html@fit`. A docs page fills the canvas,
   so it has no width; it may name one of its examples instead.

   Parsed from the right: the lens is after the last `~`, the width after the
   last `@`, the example after the last `!`, the state after the first `:`. No
   src holds `:`, `!`, or `~` — config.js refuses one that does — so the split
   is unambiguous. Nothing here is validated: the shell checks the width
   against its buttons, the state against the page, the lens against the
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
    var width = null;
    var example = null;
    var state = null;

    var tilde = raw.lastIndexOf('~');
    if (tilde > -1) {
      lens = raw.slice(tilde + 1) || null;
      raw = raw.slice(0, tilde);
    }
    var at = raw.lastIndexOf('@');
    if (at > -1) {
      width = raw.slice(at + 1) || null;
      raw = raw.slice(0, at);
    }
    var bang = raw.lastIndexOf('!');
    if (bang > -1) {
      example = raw.slice(bang + 1) || null;
      raw = raw.slice(0, bang);
    }
    var colon = raw.indexOf(':');
    if (colon > -1) {
      state = raw.slice(colon + 1) || null;
      raw = raw.slice(0, colon);
    }

    return { src: raw, state: state, example: example, width: width, lens: lens };
  }

  /* An artboard page always writes its width; a docs page, which has none,
     leaves it out. The state, example, and lens appear only when they say
     something. No src, no address. */
  function write(target) {
    if (!target || !target.src) return '';
    return target.src +
      (target.state ? ':' + target.state : '') +
      (target.example ? '!' + target.example : '') +
      (target.width ? '@' + target.width : '') +
      (target.lens ? '~' + target.lens : '');
  }

  window.wbAddress = { parse: parse, write: write };
})();
