/* Content frame lifecycle extracted from workbench.js. No application controls,
   canvas, camera or page-list initialization. The owner supplies the current view
   and receives readiness; project mounts and retained documents stay unchanged. */
(function () {
  function create(options) {
  var artboardContent = options.viewport;
  var artboard = options.surface;
  var frame = options.frame;
  var frameBuffer = options.buffer;
  var pendingFrame = null;
  var frameReady = false;
  var view = null;
  var showSeq = 0;
  function changed() { options.changed(); }
  function revealExample() { options.reveal(frame); }
  var pendingStorySwitch = null;
  var renderedStory = null;
  var reportedStorySeq = null;

  function cancelStorySwitch() {
    if (!pendingStorySwitch) return;
    clearTimeout(pendingStorySwitch.timer);
    if (pendingStorySwitch.frame !== pendingFrame) sessions.protect(pendingStorySwitch.frame, false);
    pendingStorySwitch = null;
  }

  function fallBackStorySwitch(switching) {
    if (pendingStorySwitch !== switching || switching.seq !== showSeq) return;
    pendingStorySwitch = null;
    loadPreview(switching.url, true);
  }

  function reportRenderedStory() {
    if (!renderedStory || reportedStorySeq === showSeq || !frameReady || pendingFrame ||
        renderedStory.frame !== frame || renderedStory.seq !== showSeq || !view ||
        !view.story || view.story.id !== renderedStory.id) return;
    reportedStorySeq = showSeq;
    changed();
  }

  /* Keep Storybook's loaded preview warm. The channel does not promise that
     the preview accepted a message, so navigate only if no acknowledgement
     arrives. Once accepted, wait for its render event before telling capture
     that the new story is ready. */
  function reuseStoryFrame(previous, lens, storyId, seq) {
    var cached = sessions.get(sessionKey(window.wbLenses.storyUrl(lens, storyId)));
    var target = cached && cached.ready ? cached.frame :
      frameReady && !pendingFrame && previous && previous.lens && previous.lens.kind === 'storybook' && previous.lens.url === lens.url ? frame : null;
    if (!target) return false;
    target.wbContentView = view;
    target.wbRequestedUrl = window.wbLenses.storyUrl(lens, storyId);
    if (target !== frame || !frameReady) {
      cancelPendingFrame();
      pendingFrame = target;
      target.wbRestoring = true;
      target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
      sessions.protect(target, true);
    }
    if (target.wbStoryId === storyId) {
      renderedStory = { frame: target, id: storyId, seq: seq };
      if (pendingFrame === target) frameLoaded({ currentTarget: target });
      else { reportRenderedStory(); window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } })); }
      return true;
    }
    if (!window.wbLenses.selectStory(target, lens, storyId)) return false;
    target.wbStoryId = null;
    var switching = { frame: target, lens: lens, id: storyId, seq: seq,
      url: window.wbLenses.storyUrl(lens, storyId), timer: null };
    switching.timer = setTimeout(function () { fallBackStorySwitch(switching); }, 1500);
    pendingStorySwitch = switching;
    return true;
  }

  function storySwitchEvent(message) {
    var switching = pendingStorySwitch;
    if (!view || !view.story || !view.lens || view.lens.kind !== 'storybook') return;
    var target = pendingFrame || frame;
    var event = window.wbLenses.storybookEvent(message, target, view.lens);
    if (!event || event.id !== view.story.id) return;
    if (switching && switching.seq === showSeq && event.type === 'currentStoryWasSet') {
      clearTimeout(switching.timer);
      switching.timer = setTimeout(function () { fallBackStorySwitch(switching); }, 15000);
    } else if (event.type === 'storyRendered') {
      if (switching && switching.seq === showSeq) cancelStorySwitch();
      var wasReported = reportedStorySeq === showSeq;
      renderedStory = { frame: target, id: event.id, seq: showSeq };
      target.wbStoryId = event.id;
      if (target === pendingFrame && target.wbRestoring) frameLoaded({ currentTarget: target });
      reportRenderedStory();
      if (!wasReported && reportedStorySeq === showSeq && target === frame && !pendingFrame) {
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      }
    }
  }


  function warmFrame(target) {
    target.wbWarmHost = true;
    target.src = new URL('preview-host.html?actions=off', location.href).href;
  }

  var previewFrames = [frame, frameBuffer];
  var sessions = window.wbPreviewSessions.create({ dispose: disposeFrame });

  function sessionKey(url) {
    var lens = view && view.lens;
    return lens && lens.kind === 'storybook' ? 'storybook|' + lens.key + '|' + lens.url :
      (lens ? lens.key : 'design') + '|' + new URL(url, location.href).href;
  }

  function disposeFrame(target) {
    if (target.wbDisposed) return;
    target.wbDisposed = true;
    target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
    previewFrames = previewFrames.filter(function (candidate) { return candidate !== target; });
    function remove() { target.removeAttribute('src'); target.remove(); }
    var renderer;
    try { renderer = target.wbWarmHost && target.contentWindow && target.contentWindow.wbPreviewHost; } catch (_) {}
    if (renderer) renderer.reset().then(remove, function (error) { console.warn('[workbench] preview cleanup failed', error); remove(); });
    else remove();
  }

  function spareFrame() {
    var available = previewFrames.find(function (candidate) { return candidate !== frame && candidate !== pendingFrame && !sessions.has(candidate); });
    if (available) return available;
    var target = document.createElement('iframe');
    target.title = 'Preview loading';
    target.setAttribute('allow', 'clipboard-read; clipboard-write');
    target.setAttribute('aria-hidden', 'true');
    target.inert = true;
    target.addEventListener('load', frameLoaded);
    artboardContent.appendChild(target);
    previewFrames.push(target);
    return target;
  }

  function cancelPendingFrame() {
    if (!pendingFrame) return;
    var previous = pendingFrame;
    pendingFrame = null;
    previous.wbLoadSequence = (previous.wbLoadSequence || 0) + 1;
    if (previous.wbRestoring) sessions.protect(previous, false);
    else sessions.remove(previous);
  }

  function parkPreview() {
    cancelPendingFrame();
    sessions.park();
    frame.classList.remove('is-active');
    frame.setAttribute('aria-hidden', 'true');
    frame.inert = true;
    freezeFrameSize(frame);
    frameReady = false;
  }

  function freezeFrameSize(target) {
    if (target.style && target.wbFrameSize && target.wbFrameSize.width) {
      target.style.width = target.wbFrameSize.width + 'px';
      target.style.height = target.wbFrameSize.height + 'px';
    }
  }

  function startWarmLoad(target) {
    var address = target.wbRequestedUrl;
    var seq = target.wbLoadSequence;
    var requestedView = target.wbContentView;
    var renderer = target.contentWindow && target.contentWindow.wbPreviewHost;
    if (!address || !renderer) return;
    renderer.load(address).then(function () {
      if (seq !== target.wbLoadSequence || target.wbRequestedUrl !== address) return;
      frameLoaded({ currentTarget: target, warm: true });
    }).catch(function (error) { if (seq === target.wbLoadSequence && (target === frame || target === pendingFrame)) options.failed(error, requestedView); });
  }

  function prepareFrame(target, src) {
    target.wbContentView = view;
    target.wbRequestedUrl = src;
    target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
    if (/\.workbench\.tsx?(?:[?#]|$)/.test(src) && new URL(src, location.href).origin === location.origin) {
      if (target.wbWarmHost && target.contentWindow && target.contentWindow.wbPreviewHost) startWarmLoad(target);
      else warmFrame(target);
    } else {
      target.wbWarmHost = false;
      target.src = src;
    }
  }

  function loadPreview(src, force) {
    cancelPendingFrame();
    if (previewFrames.indexOf(frame) === -1) frame = spareFrame();
    var key = sessionKey(src);
    var cached = sessions.get(key);
    if (force && cached) {
      if (cached.frame === frame) sessions.forget(frame);
      else sessions.remove(cached.frame);
      cached = null;
    }
    if (cached && cached.ready) {
      var retained = cached.frame;
      retained.wbContentView = view;
      if (retained.style) { retained.style.width = ''; retained.style.height = ''; }
      retained.wbRestoring = true;
      retained.wbRequestedUrl = src;
      retained.wbLoadSequence = (retained.wbLoadSequence || 0) + 1;
      sessions.protect(retained, true);
      pendingFrame = retained;
      var sequence = retained.wbLoadSequence;
      var renderer = retained.wbWarmHost && retained.contentWindow && retained.contentWindow.wbPreviewHost;
      Promise.resolve(renderer ? renderer.resume(src) : null).then(function () {
        if (pendingFrame === retained && sequence === retained.wbLoadSequence) frameLoaded({ currentTarget: retained, warm: true });
      }, function (error) {
        if (pendingFrame !== retained || sequence !== retained.wbLoadSequence) return;
        console.warn('[workbench] retained preview could not resume', error);
        loadPreview(src, true);
      });
      return;
    }
    // The warm document: the visible warm host changes page in place, as an
    // application's router would, keeping loaded dependencies, stylesheets and
    // fonts. A new state of the same preview re-renders without reloading it.
    if (!force && frame.wbWarmHost && frame.wbRequestedUrl && frameReady && /\.workbench\.tsx?(?:[?#]|$)/.test(src) &&
        new URL(src, location.href).origin === location.origin) {
      var showing = new URL(frame.wbRequestedUrl, location.href);
      var next = new URL(src, location.href);
      sessions.forget(frame);
      frame.wbSessionKey = key;
      frame.wbContentView = view;
      frame.wbRestoring = false;
      sessions.add(key, frame);
      var host = frame.contentWindow;
      if (showing.pathname === next.pathname && host && host.__workbenchOptions && !host.__workbenchError) {
        frame.wbRequestedUrl = src;
        frame.wbLoadSequence = (frame.wbLoadSequence || 0) + 1;
        host.history.replaceState(host.history.state, '', next.href);
        host.__workbenchReady = false;
        host.postMessage({ type: 'workbench-preview-command', command: 'state', state: next.searchParams.get('state') || undefined }, location.origin);
        frameLoaded({ currentTarget: frame, warm: true });
        return;
      }
      prepareFrame(frame, src);
      return;
    }
    if (!frame.getAttribute('src') || (frame.wbWarmHost && !frame.wbRequestedUrl && !frameReady)) {
      artboardContent.classList.add('is-loading');
      frameReady = false;
      frame.wbSessionKey = key;
      frame.wbRestoring = false;
      sessions.add(key, frame);
      prepareFrame(frame, src);
      return;
    }
    pendingFrame = frameBuffer = spareFrame();
    pendingFrame.wbSessionKey = key;
    pendingFrame.wbRestoring = false;
    sessions.add(key, pendingFrame);
    prepareFrame(pendingFrame, src);
  }

  function frameLoaded(e) {
    var loaded = e.currentTarget;
    if (previewFrames.indexOf(loaded) === -1 || (e.sequence !== undefined && e.sequence !== loaded.wbLoadSequence)) return;
    var sequence = loaded.wbLoadSequence;
    if (loaded.wbWarmHost && !e.warm) {
      startWarmLoad(loaded);
      return;
    }
    try {
      var preview = loaded.contentWindow;
      if (preview && (preview.__workbenchOptions || preview.__workbenchDocs) && !preview.__workbenchReady && !preview.__workbenchError) {
        var attempt = e.previewAttempt || 0;
        if (attempt < 300) {
          window.setTimeout(function () { frameLoaded({ currentTarget: loaded, previewAttempt: attempt + 1, sequence: sequence, warm: true }); }, 50);
          return;
        }
        options.failed(new Error('Preview did not become ready.'), loaded.wbContentView);
        return;
      }
      if (preview && preview.__workbenchError) { options.failed(new Error(String(preview.__workbenchError)), loaded.wbContentView); return; }
    } catch (_) { /* Foreign-origin previews keep their existing load behavior. */ }
    if (loaded === pendingFrame) {
      window.requestAnimationFrame(function () {
        if (loaded !== pendingFrame || sequence !== loaded.wbLoadSequence) return;
        var previous = frame;
        previous.classList.remove('is-active');
        previous.setAttribute('aria-hidden', 'true');
        loaded.classList.add('is-active');
        loaded.removeAttribute('aria-hidden');
        previous.inert = true;
        loaded.inert = false;
        freezeFrameSize(previous);
        if (loaded.style) { loaded.style.width = ''; loaded.style.height = ''; }
        loaded.title = 'Preview';
        previous.title = 'Preview loading';
        frame = loaded;
        frameBuffer = previous;
        pendingFrame = null;
        frameReady = true;
        // A resize can start the spare frame before the initial frame has
        // loaded. Whichever wins must reveal the preview and enable prepare.
        artboardContent.classList.remove('is-loading');
        artboard.hidden = false;
        loaded.wbRestoring = false;
        var outgoing = previous.wbSessionKey && sessions.get(previous.wbSessionKey);
        if (previous !== loaded && outgoing && !outgoing.ready) sessions.remove(previous);
        sessions.activate(loaded);
        if (previous !== loaded && !sessions.has(previous)) disposeFrame(previous);
        reportRenderedStory();
        revealExample();
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      });
      return;
    }
    if (loaded === frame && !pendingFrame) {
      window.requestAnimationFrame(function () {
        if (loaded !== frame || pendingFrame || sequence !== loaded.wbLoadSequence) return;
        sessions.activate(loaded);
        loaded.inert = false;
        loaded.classList.add('is-active');
        loaded.removeAttribute('aria-hidden');
        frameReady = true;
        artboardContent.classList.remove('is-loading');
        artboard.hidden = false;
        reportRenderedStory();
        revealExample();
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      });
    }
  }

  frame.addEventListener('load', frameLoaded);
  frameBuffer.addEventListener('load', frameLoaded);

  return {
    show: function (next, force) {
      var previous = view;
      showSeq++;
      cancelStorySwitch();
      cancelPendingFrame();
      renderedStory = null;
      reportedStorySeq = null;
      view = next;
      if (view.lens && view.lens.kind === 'storybook' && !force && reuseStoryFrame(previous, view.lens, view.story.id, showSeq)) return;
      loadPreview(view.url, force);
    },
    suspend: function () { showSeq++; cancelStorySwitch(); parkPreview(); view = null; },
    ready: function () {
      if (!view || !frameReady || pendingFrame || pendingStorySwitch || frame.wbRequestedUrl !== view.url) return false;
      return !(view.lens && view.lens.kind === 'storybook') || !!(renderedStory && renderedStory.frame === frame && renderedStory.seq === showSeq && view.story && renderedStory.id === view.story.id);
    },
    frame: function () { return frame; },
    storyEvent: storySwitchEvent,
    close: function () { showSeq++; cancelStorySwitch(); cancelPendingFrame(); sessions.close(); }
  };
  }
  window.wbContentFrames = { create: create };
})();
