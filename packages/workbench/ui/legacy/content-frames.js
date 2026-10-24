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


  function warmKind(src) {
    var url = new URL(src, location.href);
    if (url.origin !== location.origin) return null;
    if (/\.workbench\.tsx?$/.test(url.pathname)) return 'preview';
    if (/\.md$/.test(url.pathname) && url.searchParams.get('lens')) return 'docs:' + url.searchParams.get('lens');
    return null;
  }

  function hostOf(target) {
    try {
      var content = target.contentWindow;
      return content && (target.wbWarmKind === 'preview' ? content.wbPreviewHost : content.wbDocsHost);
    } catch (_) { return null; }
  }

  function warmFrame(target, kind) {
    target.wbWarmHost = true;
    target.wbWarmKind = kind;
    target.src = kind === 'preview' ? new URL('preview-host.html?actions=off', location.href).href :
      new URL('/_workbench/ui/src/modules/docs/host/docs-host.html?lens=' + encodeURIComponent(kind.slice(5)), location.href).href;
  }

  function cancelReadyWait(target) {
    var waiting = target.wbReadyWait;
    if (!waiting) return;
    target.wbReadyWait = null;
    window.clearTimeout(waiting.timer);
    waiting.preview.removeEventListener('workbench:ready', waiting.ready);
  }

  var previewFrames = [frame, frameBuffer];
  var sessions = window.wbPreviewSessions.create({ dispose: disposeFrame });

  function sessionKey(url) {
    var lens = view && view.lens;
    var identity = new URL(url, location.href);
    ['wbTrace', 'wbPage', 'wbScenario', 'wbLens'].forEach(function (key) { identity.searchParams.delete(key); });
    return lens && lens.kind === 'storybook' ? 'storybook|' + lens.key + '|' + lens.url :
      (lens ? lens.key : 'design') + '|' + identity.href;
  }

  function disposeFrame(target) {
    if (target.wbDisposed) return;
    cancelReadyWait(target);
    target.wbDisposed = true;
    target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
    previewFrames = previewFrames.filter(function (candidate) { return candidate !== target; });
    function remove() { target.removeAttribute('src'); target.remove(); }
    var renderer;
    renderer = target.wbWarmHost && hostOf(target);
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
    cancelReadyWait(previous);
    previous.wbLoadSequence = (previous.wbLoadSequence || 0) + 1;
    if (previous.wbRestoring) sessions.protect(previous, false);
    else sessions.remove(previous);
  }

  function parkPreview() {
    cancelPendingFrame();
    cancelReadyWait(frame);
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
    var renderer = hostOf(target);
    if (!address || !renderer) return;
    renderer.load(address).then(function () {
      if (seq !== target.wbLoadSequence || target.wbRequestedUrl !== address) return;
      frameLoaded({ currentTarget: target, warm: true });
    }).catch(function (error) { if (seq === target.wbLoadSequence && (target === frame || target === pendingFrame)) options.failed(error, requestedView); });
  }

  function prepareFrame(target, src) {
    cancelReadyWait(target);
    target.wbContentView = view;
    target.wbRequestedUrl = src;
    target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
    var kind = warmKind(src);
    if (kind) {
      if (target.wbWarmHost && target.wbWarmKind === kind && hostOf(target)) startWarmLoad(target);
      else warmFrame(target, kind);
    } else {
      target.wbWarmHost = false;
      target.wbWarmKind = null;
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
      var renderer = retained.wbWarmHost && hostOf(retained);
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
    // fonts. A new scenario of the same preview re-renders without reloading it.
    var kind = warmKind(src);
    if (!force && frame.wbWarmHost && frame.wbWarmKind === 'preview' && frame.wbRequestedUrl && frameReady && kind === 'preview') {
      var showing = new URL(frame.wbRequestedUrl, location.href);
      var next = new URL(src, location.href);
      sessions.forget(frame);
      frame.wbSessionKey = key;
      frame.wbContentView = view;
      frame.wbRestoring = false;
      sessions.add(key, frame);
      var host = frame.contentWindow;
      // Another iteration can bring its own source, so it loads the document again.
      var sameIteration = showing.searchParams.get('iteration') === next.searchParams.get('iteration');
      if (showing.pathname === next.pathname && sameIteration && host && host.__workbenchOptions && !host.__workbenchError) {
        frame.wbRequestedUrl = src;
        frame.wbLoadSequence = (frame.wbLoadSequence || 0) + 1;
        host.history.replaceState(host.history.state, '', next.href);
        host.__workbenchReady = false;
        host.postMessage({ type: 'workbench-preview-command', command: 'scenario', scenario: next.searchParams.get('scenario') || undefined }, location.origin);
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
    if (!force && kind && kind.indexOf('docs:') === 0) {
      var idle = sessions.idleFrames();
      if (idle.length >= 3) {
        var reuse = idle.find(function (candidate) { return candidate !== frame && candidate.wbWarmKind === kind && hostOf(candidate); });
        if (reuse) {
          sessions.forget(reuse);
          pendingFrame = frameBuffer = reuse;
          reuse.wbSessionKey = key;
          reuse.wbRestoring = false;
          if (reuse.style) { reuse.style.width = ''; reuse.style.height = ''; }
          sessions.add(key, reuse);
          prepareFrame(reuse, src);
          return;
        }
      }
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
    cancelReadyWait(loaded);
    var sequence = loaded.wbLoadSequence;
    var measuredView = loaded.wbContentView;
    if (measuredView && measuredView.trace && !e.previewAttempt) {
      measuredView.phase(measuredView.trace, 'document-loaded', { retained: !!loaded.wbRestoring, warm: !!e.warm });
    }
    if (loaded.wbWarmHost && !e.warm) {
      startWarmLoad(loaded);
      return;
    }
    try {
      var preview = loaded.contentWindow;
      if (preview && (preview.__workbenchOptions || preview.__workbenchDocs) && !preview.__workbenchReady && !preview.__workbenchError) {
        var attempt = e.previewAttempt || 0;
        if (attempt < 300) {
          // Docs already signal when examples, fonts and images have settled.
          // Observe that signal instead of holding ready pixels until a poll.
          // Keep polling for older/custom renderers that only expose the flag.
          var ready = function () {
            frameLoaded({ currentTarget: loaded, previewAttempt: attempt + 1, sequence: sequence, warm: true });
          };
          loaded.wbReadyWait = { preview: preview, ready: ready, timer: window.setTimeout(ready, 50) };
          preview.addEventListener('workbench:ready', ready);
          return;
        }
        options.failed(new Error('Preview did not become ready.'), loaded.wbContentView);
        return;
      }
      if (preview && preview.__workbenchError) { options.failed(new Error(String(preview.__workbenchError)), loaded.wbContentView); return; }
    } catch (_) { /* Foreign-origin previews keep their existing load behavior. */ }
    if (loaded === pendingFrame) {
      if (measuredView && measuredView.trace) measuredView.phase(measuredView.trace, 'readiness');
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
        if (measuredView && measuredView.trace) measuredView.phase(measuredView.trace, 'frame-promotion');
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
      if (measuredView && measuredView.trace) measuredView.phase(measuredView.trace, 'readiness');
      window.requestAnimationFrame(function () {
        if (loaded !== frame || pendingFrame || sequence !== loaded.wbLoadSequence) return;
        sessions.activate(loaded);
        loaded.inert = false;
        loaded.classList.add('is-active');
        loaded.removeAttribute('aria-hidden');
        if (loaded.style) { loaded.style.width = ''; loaded.style.height = ''; }
        frameReady = true;
        if (measuredView && measuredView.trace) measuredView.phase(measuredView.trace, 'frame-promotion');
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
      // A pending page can request another viewport size before it is ready.
      // Keep the outgoing document at its displayed size during the grace period.
      if (frameReady && frame.clientWidth > 0 && frame.clientHeight > 0) {
        frame.wbFrameSize = { width: frame.clientWidth, height: frame.clientHeight };
        freezeFrameSize(frame);
      }
      showSeq++;
      cancelStorySwitch();
      cancelPendingFrame();
      cancelReadyWait(frame);
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
