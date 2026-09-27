/* What is under a point
   ---------------------
   The handoff names the element under each mark, so "make this full width"
   has a referent that isn't a coordinate. Two places ask: the markup layer,
   reading straight into a same-origin preview, and the browser that opens
   an implementation's page, where the server puts this file into the page
   and asks it by DevTools. One file, so both name things the same way.

   Coordinates are the page's own viewport CSS pixels. */
(function () {
  /* A custom element keeps its real markup in a shadow root, and
     elementFromPoint stops at the host. Keep descending to the thing that
     was actually pointed at. */
  function elementAt(doc, x, y) {
    var el = doc.elementFromPoint(x, y);
    while (el && el.shadowRoot) {
      var deeper = el.shadowRoot.elementFromPoint(x, y);
      if (!deeper || deeper === el) break;
      el = deeper;
    }
    return el;
  }

  /* Named the way it would be written in a stylesheet, plus its words —
     enough for an agent to find it in the source without a coordinate. */
  function describe(el) {
    if (!el || (el.ownerDocument && el === el.ownerDocument.body)) return null;
    var name = el.tagName.toLowerCase();
    if (el.id) name += '#' + el.id;
    else if (el.classList.length) name += '.' + Array.prototype.slice.call(el.classList).join('.');

    var text = (el.textContent || '').replace(/\s+/g, ' ').trim();
    if (!text) return name;
    return name + ' “' + (text.length > 60 ? text.slice(0, 60) + '…' : text) + '”';
  }

  function at(doc, x, y) {
    return describe(elementAt(doc, x, y));
  }

  window.wbDescribe = { at: at, describe: describe, elementAt: elementAt };
})();
