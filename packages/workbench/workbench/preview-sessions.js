/* Retention policy only: the canvas owns frames, readiness and presentation. */
(function (root) {
  function create(options) {
    options = options || {};
    var now = options.now || Date.now;
    var later = options.setTimeout || setTimeout;
    var cancel = options.clearTimeout || clearTimeout;
    var ttl = options.ttl || 15 * 60 * 1000;
    var limit = options.limit === undefined ? 3 : options.limit;
    var entries = new Map();
    var active = null;
    var timer = null;
    var order = 0;
    function discard(entry) {
      entries.delete(entry.key);
      if (active === entry) active = null;
      options.dispose(entry.frame);
    }
    function trim() {
      cancel(timer); timer = null;
      var idle = Array.from(entries.values()).filter(function (entry) { return entry !== active && !entry.protected; });
      idle.sort(function (a, b) { return a.order - b.order; });
      idle.slice().forEach(function (entry) { if (now() - entry.parkedAt >= ttl) { discard(entry); idle.splice(idle.indexOf(entry), 1); } });
      while (idle.length > limit) discard(idle.shift());
      if (idle.length) timer = later(trim, Math.max(1, Math.min.apply(null, idle.map(function (entry) { return entry.parkedAt + ttl - now(); }))));
    }
    function find(frame) { return Array.from(entries.values()).find(function (entry) { return entry.frame === frame; }); }
    return {
      get: function (key) { trim(); return entries.get(key) || null; },
      has: function (frame) { return !!find(frame); },
      add: function (key, frame) {
        var previous = entries.get(key);
        if (previous && previous.frame !== frame) discard(previous);
        var entry = { key: key, frame: frame, ready: false, protected: true, parkedAt: now(), order: ++order };
        entries.set(key, entry);
        return entry;
      },
      protect: function (frame, value) { var entry = find(frame); if (entry) entry.protected = value; trim(); },
      activate: function (frame) {
        var next = find(frame);
        if (!next) return;
        if (active && active !== next) { active.parkedAt = now(); active.order = ++order; }
        next.ready = true; next.protected = false; active = next;
        trim();
      },
      park: function () { if (active) { active.parkedAt = now(); active.order = ++order; active = null; } trim(); },
      // A forced reload keeps the outgoing document visible until replacement.
      forget: function (frame) { var entry = find(frame); if (entry) entries.delete(entry.key); if (active === entry) active = null; trim(); },
      remove: function (frame) { var entry = find(frame); if (entry) discard(entry); trim(); },
      clearInactive: function () { Array.from(entries.values()).forEach(function (entry) { if (entry !== active) discard(entry); }); trim(); },
      close: function () { cancel(timer); timer = null; Array.from(entries.values()).forEach(discard); },
    };
  }
  if (typeof module === 'object' && module.exports) module.exports = { create: create };
  else root.wbPreviewSessions = { create: create };
})(typeof window === 'undefined' ? null : window);
