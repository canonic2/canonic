/* Install this script in an external preview (Storybook, for example) to let
   Canonic capture the document the reviewer is actually interacting with.
   It answers only a loopback Canonic parent and transfers inert DOM data. */
(function () {
  var scriptUrl = document.currentScript && document.currentScript.src;
  var CONNECT = 'canonic:preview:connect';
  var READY = 'canonic:preview:ready';
  var SNAPSHOT = 'canonic:preview:snapshot';
  var FLUSH = 'canonic:preview:flush';
  var DISCONNECT = 'canonic:preview:disconnect';
  var source = null;
  var parentOrigin = null;
  var scheduled = false;

  function loopback(origin) {
    try {
      var url = new URL(origin);
      return url.protocol === 'http:' && (url.hostname === '127.0.0.1' || url.hostname === 'localhost' || url.hostname === '[::1]');
    } catch (_) { return false; }
  }

  function answer(body) {
    if (parentOrigin) parent.postMessage(body, parentOrigin);
  }

  function publish(requestId) {
    scheduled = false;
    if (!source || !parentOrigin) return;
    try {
      var incremental = source.read();
      answer({
        type: SNAPSHOT,
        version: 1,
        snapshot: source.full(incremental),
        scroll: { x: window.scrollX || window.pageXOffset || 0, y: window.scrollY || window.pageYOffset || 0 },
        requestId: requestId,
      });
    } catch (error) {
      answer({ type: SNAPSHOT, version: 1, requestId: requestId, error: String(error && error.message || error) });
    }
  }

  function changed() {
    if (scheduled) return;
    scheduled = true;
    window.requestAnimationFrame(publish);
  }

  function connect(origin) {
    parentOrigin = origin;
    if (!source) source = window.wbDOMMirror.create(document, changed);
    publish();
  }

  function receive(event) {
    if (event.source !== parent || !event.data || event.data.version !== 1 || !loopback(event.origin)) return;
    if (event.data.type === CONNECT) connect(event.origin);
    if (event.data.type === FLUSH && event.origin === parentOrigin) publish(event.data.requestId);
    if (event.data.type === DISCONNECT && event.origin === parentOrigin) {
      if (source) source.stop();
      source = null;
      parentOrigin = null;
    }
  }

  function ready() {
    window.addEventListener('message', receive);
    var referrer;
    try { referrer = new URL(document.referrer); } catch (_) { return; }
    if (loopback(referrer.origin)) parent.postMessage({ type: READY, version: 1 }, referrer.origin);
  }

  if (window.wbDOMMirror) ready();
  else if (scriptUrl) {
    var dependency = document.createElement('script');
    dependency.src = new URL('dom-mirror.js', scriptUrl).toString();
    dependency.addEventListener('load', ready, { once: true });
    document.head.appendChild(dependency);
  }
})();
