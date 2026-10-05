/* The minimal page Chromium keeps warm for native captures. The preview is an
   ordinary same-origin iframe, so the browser paints custom elements, shadow
   DOM and SVG exactly as it does in the visible workbench. */

(function () {
  var frame = document.getElementById('captureFrame');
  var annotations = document.getElementById('captureAnnotations');
  var current = null;
  var revision = null;
  var prepared = null;
  var mirrorRevision = null;
  var mirrorLoading = null;
  var mirrorDocument = null;

  function mirrorLoaded(snapshot, documentKey) {
    if (!mirrorLoading || mirrorDocument !== documentKey) {
      current = null; revision = null; mirrorRevision = null;
      mirrorDocument = documentKey;
      mirrorLoading = new Promise(function (resolve) {
        frame.addEventListener('load', resolve, { once: true });
        frame.setAttribute('sandbox', 'allow-same-origin');
        frame.srcdoc = '<!doctype html><html><head></head><body></body></html>';
      });
    }
    return mirrorLoading.then(function () {
      if (mirrorRevision === snapshot.revision) return;
      return window.wbDOMMirror.apply(frame.contentDocument, snapshot).then(function () {
        mirrorRevision = snapshot.revision;
      }).catch(function (error) {
        mirrorRevision = null;
        throw error;
      });
    });
  }

  /* A revision names one load of the visible preview; the same revision reuses
     the document here. A request without one, an API caller's, always loads
     afresh so the shot shows what the page serves now. */
  function frameLoaded(url, nextRevision) {
    if (mirrorLoading) {
      mirrorLoading = null; mirrorRevision = null;
      frame.removeAttribute('sandbox'); frame.removeAttribute('srcdoc');
    }
    if (nextRevision && current === url && revision === nextRevision && frame.contentDocument && frame.contentDocument.readyState === 'complete') {
      try {
        if (frame.contentWindow.location.href === url) return Promise.resolve();
      } catch (error) {}
    }

    current = url;
    revision = nextRevision;
    return new Promise(function (resolve, reject) {
      function clean() {
        frame.removeEventListener('load', loaded);
        frame.removeEventListener('error', failed);
      }
      function loaded() {
        clean();
        resolve();
      }
      function failed() {
        clean();
        reject(new Error('The preview failed to load for capture'));
      }
      frame.addEventListener('load', loaded, { once: true });
      frame.addEventListener('error', failed, { once: true });
      frame.src = url;
    });
  }

  function mediaReady(doc) {
    var win = doc.defaultView;
    var images = Array.prototype.slice.call(doc.images || []).filter(function (image) {
      var box = image.getBoundingClientRect();
      return box.right > 0 && box.bottom > 0 && box.left < win.innerWidth && box.top < win.innerHeight;
    });
    return Promise.all(
      images.map(function (image) {
        if (image.decode) return image.decode().catch(function () {});
        if (image.complete) return Promise.resolve();
        return new Promise(function (resolve) {
          image.addEventListener('load', resolve, { once: true });
          image.addEventListener('error', resolve, { once: true });
        });
      })
    );
  }

  function paint(win) {
    return new Promise(function (resolve) {
      win.requestAnimationFrame(function () {
        win.requestAnimationFrame(resolve);
      });
    });
  }

  function scrollImmediately(win, x, y) {
    var root = win.document.documentElement;
    var value = root.style.getPropertyValue('scroll-behavior');
    var priority = root.style.getPropertyPriority('scroll-behavior');
    root.style.setProperty('scroll-behavior', 'auto', 'important');
    win.scrollTo(x, y);
    if (value) root.style.setProperty('scroll-behavior', value, priority);
    else root.style.removeProperty('scroll-behavior');
  }

  function settle(payload) {
    var win = frame.contentWindow;
    var doc = frame.contentDocument;
    var scroll = payload.scroll || { x: 0, y: 0 };
    scrollImmediately(win, Number(scroll.x) || 0, Number(scroll.y) || 0);

    var fonts = doc.fonts && doc.fonts.ready ? doc.fonts.ready.catch(function () {}) : Promise.resolve();
    return previewReady(win).then(function () { return Promise.all([fonts, mediaReady(doc)]); })
      .then(function () {
        scrollImmediately(win, Number(scroll.x) || 0, Number(scroll.y) || 0);
        return paint(win);
      })
      .then(function () {
        return paint(window);
      });
  }

  function previewReady(win) {
    if (!win.__workbenchOptions) return Promise.resolve();
    var deadline = Date.now() + 8000;
    return new Promise(function (resolve, reject) {
      function check() {
        if (win.__workbenchError) { reject(new Error(win.__workbenchError)); return; }
        if (win.__workbenchReady) { resolve(); return; }
        if (Date.now() >= deadline) { reject(new Error('Workbench preview did not finish rendering before capture')); return; }
        window.setTimeout(check, 50);
      }
      check();
    });
  }

  /* The element under each annotation, read from the mirrored document: a lens's
     page is another origin the workbench can't read, but its copy here can. */
  function targets(anchors) {
    if (!Array.isArray(anchors) || !anchors.length || !window.wbDescribe) return undefined;
    return anchors.map(function (point) {
      if (!point) return null;
      try { return window.wbDescribe.at(frame.contentDocument, Number(point.x), Number(point.y)); }
      catch (_) { return null; }
    });
  }

  function result(payload) {
    var win = frame.contentWindow;
    var requested = payload.scroll || { x: 0, y: 0 };
    return {
      ready: true,
      scroll: {
        requestedX: Number(requested.x) || 0,
        requestedY: Number(requested.y) || 0,
        appliedX: win.scrollX || win.pageXOffset || 0,
        appliedY: win.scrollY || win.pageYOffset || 0,
      },
      targets: targets(payload.anchors),
    };
  }

  window.wbCapture = {
    prepare: function (payload) {
      if (!payload || !payload.url) return Promise.reject(new Error('Capture has no preview URL'));
      var key = JSON.stringify([payload.url, payload.revision, payload.width, payload.height, payload.scroll, payload.annotations, payload.mirror && payload.mirror.revision]);
      try {
        if (payload.revision && prepared === key &&
            (payload.mirror ? mirrorRevision === payload.mirror.revision : frame.contentWindow.location.href === payload.url) &&
            frame.contentDocument.readyState === 'complete') {
          return Promise.resolve(result(payload));
        }
      } catch (_) {}
      prepared = null;
      return (payload.mirror ? mirrorLoaded(payload.mirror, JSON.stringify([payload.url, payload.revision])) : frameLoaded(payload.url, payload.revision)).then(function () {
        annotations.innerHTML = payload.annotations || '';
        return settle(payload);
      }).then(function () {
        prepared = key;
        return result(payload);
      });
    },
  };
})();
