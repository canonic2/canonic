/* The workbench side of the cooperative cross-origin preview bridge.
   A bridged preview sends inert, full DOM snapshots; the workbench still
   validates them server-side before the capture surface applies them. */
(function (root) {
  var CONNECT = 'canonic:preview:connect';
  var READY = 'canonic:preview:ready';
  var SNAPSHOT = 'canonic:preview:snapshot';
  var FLUSH = 'canonic:preview:flush';
  var DISCONNECT = 'canonic:preview:disconnect';

  function create(frame, changed, scrolled, failed) {
    var stopped = false;
    var snapshot;
    var scroll = { x: 0, y: 0 };
    var origin;
    var nextRequest = 0;
    var pending = new Map();

    try { origin = new URL(frame.src, root.location.href).origin; } catch (_) { return null; }
    if (origin === root.location.origin) return null;

    function send(type, extra) {
      if (stopped || !frame.contentWindow) return;
      frame.contentWindow.postMessage(Object.assign({ type: type, version: 1 }, extra), origin);
    }

    function receive(event) {
      if (stopped || event.source !== frame.contentWindow || event.origin !== origin || !event.data) return;
      if (event.data.type === READY) {
        send(CONNECT);
        return;
      }
      if (event.data.type !== SNAPSHOT || event.data.version !== 1) return;
      if (event.data.error) {
        var failedRequest = pending.get(event.data.requestId);
        if (failedRequest) {
          clearTimeout(failedRequest.timer);
          pending.delete(event.data.requestId);
          failedRequest.reject(new Error(String(event.data.error)));
        }
        if (failed) failed(String(event.data.error));
        return;
      }
      if (!event.data.snapshot || typeof event.data.snapshot.revision !== 'string') return;
      snapshot = event.data.snapshot;
      frame.dataset.canonicPreviewBridge = 'ready';
      scroll = {
        x: Number(event.data.scroll && event.data.scroll.x) || 0,
        y: Number(event.data.scroll && event.data.scroll.y) || 0,
      };
      if (scrolled) scrolled(scroll);
      if (changed) changed();
      var waiting = pending.get(event.data.requestId);
      if (waiting) {
        clearTimeout(waiting.timer);
        pending.delete(event.data.requestId);
        waiting.resolve();
      }
    }

    root.addEventListener('message', receive);
    send(CONNECT);
    return {
      read: function () { return snapshot; },
      full: function (value) { return value; },
      acknowledge: function () {},
      scroll: function () { return { x: scroll.x, y: scroll.y }; },
      flush: function () {
        if (stopped) return Promise.reject(new Error('Preview bridge is stopped'));
        /* A client exists for every cross-origin frame, before we know whether
           that page installed the cooperative bridge. With no first snapshot
           there is nothing to flush: let capture use its URL fallback. Once a
           page has answered, however, a lost flush is a real bridge failure and
           must not silently capture a separate, potentially signed-out session. */
        if (!snapshot) return Promise.resolve();
        var requestId = String(++nextRequest);
        return new Promise(function (resolve, reject) {
          var timer = setTimeout(function () {
            pending.delete(requestId);
            reject(new Error('Preview bridge did not answer the capture flush'));
          }, 2000);
          pending.set(requestId, { resolve: resolve, reject: reject, timer: timer });
          send(FLUSH, { requestId: requestId });
        });
      },
      stop: function () {
        send(DISCONNECT);
        stopped = true;
        delete frame.dataset.canonicPreviewBridge;
        pending.forEach(function (waiting) {
          clearTimeout(waiting.timer);
          waiting.reject(new Error('Preview bridge was replaced'));
        });
        pending.clear();
        root.removeEventListener('message', receive);
      },
    };
  }

  root.wbPreviewBridge = { create: create };
})(typeof window === 'undefined' ? globalThis : window);
