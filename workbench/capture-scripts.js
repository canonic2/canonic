/* Scripts the capture helper runs inside a page: the markup overlay and
   scroll it is shot with, the element under each mark for a handoff, and the
   settling gate and in-place Storybook switch used by export references. */

var OVERLAY_ID = '__wb_markup';

function overlayScript(payload) {
  return '(function () {' +
    'var s = ' + JSON.stringify(payload.scroll || null) + ';' +
    'if (s) {' +
    '  var r = document.documentElement, v = r.style.getPropertyValue("scroll-behavior"), p = r.style.getPropertyPriority("scroll-behavior");' +
    '  r.style.setProperty("scroll-behavior", "auto", "important");' +
    '  window.scrollTo(s.x, s.y);' +
    '  if (v) r.style.setProperty("scroll-behavior", v, p); else r.style.removeProperty("scroll-behavior");' +
    '}' +
    'var old = document.getElementById(' + JSON.stringify(OVERLAY_ID) + ');' +
    'if (old) old.remove();' +
    'var o = document.createElement("div");' +
    'o.id = ' + JSON.stringify(OVERLAY_ID) + ';' +
    'o.className = "wb wb-markup-layer";' +
    'o.style.cssText = "position:fixed;inset:0;overflow:hidden;pointer-events:none;z-index:2147483647";' +
    'o.innerHTML = ' + JSON.stringify('<style>' + (payload.css || '') + '</style>' + (payload.markup || '')) + ';' +
    'document.documentElement.appendChild(o);' +
    'var fonts = document.fonts && document.fonts.ready ? document.fonts.ready : Promise.resolve();' +
    'return fonts.catch(function () {}).then(function () {' +
    '  return new Promise(function (done) {' +
    '    requestAnimationFrame(function () { requestAnimationFrame(function () {' +
    '      done({ scroll: { requestedX: s ? Number(s.x) || 0 : 0, requestedY: s ? Number(s.y) || 0 : 0,' +
    '        appliedX: window.scrollX || window.pageXOffset || 0, appliedY: window.scrollY || window.pageYOffset || 0 } });' +
    '    }); });' +
    '  });' +
    '});' +
    '})()';
}

var REMOVE_OVERLAY = '(function () { var o = document.getElementById(' + JSON.stringify(OVERLAY_ID) + '); if (o) o.remove(); return true; })()';

function describeScript(anchors) {
  return '(function () {' +
    'var at = ' + JSON.stringify(anchors || []) + ';' +
    'return at.map(function (p) {' +
    '  if (!p || typeof window.__wbDescribeAt !== "function") return null;' +
    '  try { return window.__wbDescribeAt(p.x, p.y); } catch (e) { return null; }' +
    '});' +
    '})()';
}

/* A load event only means Storybook's preview shell arrived. Its story is
   rendered later, while #storybook-root is hidden behind .sb-loader. Wait for
   that hand-off and then for a short quiet window so async components, fonts,
   and visible images have painted before pixels are read. */
function settleScript(options) {
  options = options || {};
  var imageTimeout = Number.isFinite(options.imageTimeout) ? Math.max(0, options.imageTimeout) : 2000;
  var quiet = options.quiet !== false;
  var quietScript = quiet ? `
    var changed = Date.now();
    var observer = new MutationObserver(function () { changed = Date.now(); });
    observer.observe(document.documentElement, { attributes: true, childList: true, characterData: true, subtree: true });
    var quietDeadline = Date.now() + 1500;
    while (Date.now() - changed < 250 && Date.now() < quietDeadline) await sleep(50);
    observer.disconnect();` : '';
  return `(async function () {
    function sleep(ms) { return new Promise(function (resolve) { setTimeout(resolve, ms); }); }
    function visible(node) {
      if (!node || node.hidden) return false;
      var style = getComputedStyle(node);
      if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0) return false;
      var box = node.getBoundingClientRect();
      return box.width > 0 && box.height > 0;
    }
    var storybook = location.pathname.replace(/\\/+$/, '').endsWith('/iframe.html') || location.pathname === '/iframe.html';
    if (/\\.workbench\\.tsx?$/.test(location.pathname) || window.__workbenchOptions) {
      var previewDeadline = Date.now() + 8000;
      while (!window.__workbenchReady) {
        if (window.__workbenchError) throw new Error(window.__workbenchError);
        if (Date.now() >= previewDeadline) throw new Error('Workbench preview did not finish rendering before capture');
        await sleep(50);
      }
    }
    if (storybook) {
      var deadline = Date.now() + 8000;
      while (true) {
        var loader = Array.from(document.querySelectorAll('.sb-loader, .sb-preparing-story, .sb-preparing-docs')).some(visible);
        var failure = Array.from(document.querySelectorAll('.sb-errordisplay, .sb-nopreview')).some(visible);
        var main = document.body.classList.contains('sb-show-main');
        if (failure || (main && !loader)) break;
        if (Date.now() >= deadline) throw new Error('Storybook did not finish rendering before capture');
        await sleep(50);
      }
    }
    if (document.fonts && document.fonts.ready) await document.fonts.ready.catch(function () {});
    var images = Promise.all(Array.from(document.images).filter(function (image) {
      return visible(image) && !image.complete;
    }).map(function (image) {
      return image.decode ? image.decode().catch(function () {}) : Promise.resolve();
    }));
    await Promise.race([images, sleep(${imageTimeout})]);
    ${quietScript}
    await new Promise(function (resolve) { requestAnimationFrame(function () { requestAnimationFrame(resolve); }); });
    return { storybook: storybook };
  })()`;
}

function storybookPreview(value) {
  try {
    var url = new URL(value);
    if (url.pathname.replace(/\/+$/, '') !== '/iframe.html' && !url.pathname.endsWith('/iframe.html')) return null;
    return { document: url.origin + url.pathname, id: url.searchParams.get('id') || '' };
  } catch (_) { return null; }
}

function storybookSwitchScript(url) {
  var story = storybookPreview(url);
  if (!story) return 'false';
  return `(function () {
    var root = document.getElementById('storybook-root');
    var docs = document.getElementById('storybook-docs');
    if (root) root.hidden = true;
    if (docs) docs.hidden = true;
    document.body.classList.remove('sb-show-main');
    document.body.classList.add('sb-show-preparing-story');
    history.replaceState(null, '', ${JSON.stringify(url)});
    window.postMessage(JSON.stringify({ key: 'storybook-channel', event: {
      type: 'setCurrentStory', args: [{ storyId: ${JSON.stringify(story.id)}, viewMode: 'story' }],
      from: 'canonic-export'
    } }), location.origin);
    return true;
  })()`;
}

module.exports = {
  overlayScript: overlayScript,
  removeOverlayScript: REMOVE_OVERLAY,
  describeScript: describeScript,
  settleScript: settleScript,
  storybookSwitchScript: storybookSwitchScript,
};
