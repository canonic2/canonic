/* Keep one prepare in flight and collapse changes to the latest visible view.
   A heartbeat checks readiness; transient failures retry without disabling
   native capture for the rest of the workbench session. */
(function (root) {
  function create(options) {
    var timer;
    var busy = false;
    var again = false;
    var stopped = false;
    var paused = 0;
    var failures = 0;
    var retrying = false;
    function schedule(delay) {
      if (stopped) return;
      if (paused) { again = true; return; }
      if (busy) { again = true; return; }
      // Continuous mutations must not postpone preparation indefinitely.
      if (timer && !retrying && typeof delay !== 'number') return;
      clearTimeout(timer);
      retrying = false;
      timer = setTimeout(run, typeof delay === 'number' ? delay : 80);
    }
    function run() {
      timer = null;
      if (stopped) return;
      busy = true;
      again = false;
      var requested = false;
      Promise.resolve().then(function () {
        if (paused || stopped) return;
        var request = options.read();
        if (!request) return;
        requested = true;
        return options.prepare(request);
      }).then(function () {
        failures = 0;
        if (requested && options.ready) options.ready();
      }, function (error) {
        failures++;
        if (options.failed) options.failed(error);
      }).finally(function () {
        busy = false;
        if (again) schedule();
        else if (failures) {
          schedule(Math.min(30000, 1000 * Math.pow(2, failures - 1)));
          retrying = true;
        }
      });
    }
    /* The heartbeat catches a renderer that came back after nothing changed
       here. A change reschedules at once; a pending retry keeps its backoff. */
    var heartbeat = setInterval(function () { if (!timer) schedule(); }, options.heartbeat || 5000);
    return {
      schedule: schedule,
      pause: function () { paused++; clearTimeout(timer); timer = null; },
      resume: function () { if (paused) paused--; if (!paused) schedule(0); },
      stop: function () { stopped = true; clearTimeout(timer); clearInterval(heartbeat); },
    };
  }
  if (typeof module === 'object' && module.exports) module.exports = { create: create };
  else root.wbCaptureSync = { create: create };
})(typeof window === 'undefined' ? null : window);
