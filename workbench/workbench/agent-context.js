/* Tells the server which screen this canvas shows, so agents in the editor
   can ask for it. It says exactly what Copy reference would copy, read from
   the resolved view the same way, and says nothing while that isn't ready.
   A heartbeat keeps this canvas counted while it stays open; closing it
   says so. See agent-view.js for the server's side. */
(function () {
  var PATH = '/_workbench/view';
  var HEARTBEAT_MS = 30 * 1000;
  var client = Date.now().toString(36) + Math.random().toString(36).slice(2, 10);
  var timer = null;

  function current() {
    var view = window.wbView ? window.wbView() : null;
    var text = window.wbReference ? window.wbReference.text(view) : null;
    if (!text) return null;
    return {
      text: text,
      src: view.src,
      state: view.story ? null : view.state,
      story: view.story ? view.story.id : null,
      lens: view.lens ? view.lens.key : null,
    };
  }

  function post(body) {
    try {
      fetch(PATH, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        keepalive: true,
      }).catch(function () {});
    } catch (error) { /* a file:// canvas has no server */ }
  }

  /* Several changes land together when a screen settles; send the last. */
  function report() {
    clearTimeout(timer);
    timer = setTimeout(function () {
      post({ client: client, view: current() });
    }, 150);
  }

  if (location.protocol !== 'http:' && location.protocol !== 'https:') {
    window.wbAgentContext = { report: function () {} };
    return;
  }

  window.addEventListener('wb-frame-change', report);
  window.addEventListener('hashchange', report);
  setInterval(report, HEARTBEAT_MS);
  window.addEventListener('pagehide', function () {
    var body = JSON.stringify({ client: client, closed: true });
    if (navigator.sendBeacon) navigator.sendBeacon(PATH, body);
    else post({ client: client, closed: true });
  });

  window.wbAgentContext = { report: report };
  report();
})();
