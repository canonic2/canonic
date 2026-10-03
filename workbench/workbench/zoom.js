/* Canvas zoom and pan
   -------------------
   The frame sits on the stage the way a frame sits on a Figma canvas: it keeps
   its real size — 1512 × 982 is still 1512 × 982 to the page inside — and the
   canvas scales and moves it. One transform on the frame shell does both, so
   the page lays out at its true width whatever the zoom, and the markup layer,
   which lives in the same shell, scales with it.

     ⌘/Ctrl + wheel, pinch   zoom at the pointer
     wheel, Shift + wheel    pan
     Space + drag, middle    pan
     ⌘ =  ⌘ -                zoom in, out (by powers of two, as Figma does)
     ⌘ 0  Shift 0            100%
     Shift 1                 zoom to fit

   Until the canvas is zoomed or panned by hand it stays fitted: a new frame
   size, or a resized editor, fits again. Fitting never magnifies past 100%.

   Wheel and keys over a preview served by the workbench are read from inside
   its document, which is the same origin. A lens on another origin keeps its
   own wheel; Storybook's key channel still carries the shortcuts (see
   workbench.js). */
(function (root) {
  var MIN = 0.1;
  var MAX = 8;
  /* Room around a fitted frame: its label above, the floating tool bar below. */
  var INSET = { top: 40, right: 32, bottom: 80, left: 32 };
  /* At least this much of the frame stays on screen however it is panned. */
  var KEEP = 48;

  function clamp(z) {
    return Math.min(MAX, Math.max(MIN, z));
  }

  /* Figma steps by powers of two: 69% zooms in to 100%, then 200%. */
  function stepIn(z) {
    return clamp(Math.pow(2, Math.floor(Math.log2(z) + 1e-6) + 1));
  }

  function stepOut(z) {
    return clamp(Math.pow(2, Math.ceil(Math.log2(z) - 1e-6) - 1));
  }

  /* A mouse notch reports ~100px, a pinch a few px per event. Capping the
     delta keeps a notch to ~22% while a pinch stays smooth. */
  function wheelFactor(e) {
    var dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
    return Math.exp(-Math.max(-25, Math.min(25, dy)) * 0.01);
  }

  /* The zoom shortcut a key event asks for, or null. Physical codes keep it
     working on layouts where = or 0 sit elsewhere. */
  function command(e) {
    var mod = e.metaKey || e.ctrlKey;
    var key = String(e.key || '');
    var code = String(e.code || '');
    if (e.altKey) return null;
    if (mod) {
      if (key === '=' || key === '+' || code === 'Equal' || code === 'NumpadAdd') return 'in';
      if (key === '-' || key === '_' || code === 'Minus' || code === 'NumpadSubtract') return 'out';
      if (key === '0' || code === 'Digit0' || code === 'Numpad0') return 'actual';
      return null;
    }
    if (e.shiftKey && (code === 'Digit0' || key === ')')) return 'actual';
    if (e.shiftKey && (code === 'Digit1' || key === '!')) return 'fit';
    return null;
  }

  function typing(e) {
    var el = e.target;
    if (!el || el.nodeType !== 1) return false;
    var tag = String(el.tagName || '').toUpperCase();
    return !!el.isContentEditable || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
  }

  if (!root.document) {
    root.wbZoomRules = { stepIn: stepIn, stepOut: stepOut, command: command, clamp: clamp, wheelFactor: wheelFactor };
    return;
  }

  var document = root.document;
  var shell = document.querySelector('.wb');
  var stage = document.getElementById('stage');
  var frameShell = document.getElementById('frameShell');
  var label = document.getElementById('frameLabel');
  var sizeText = document.getElementById('frameSize');
  var control = document.getElementById('zoomControl');
  var percent = document.getElementById('zoomValue');
  var menu = document.getElementById('zoomMenu');

  var z = 1;
  var x = 0;
  var y = 0;
  var fitted = true; /* follows the stage until zoomed or panned by hand */

  function view() {
    return { w: stage.clientWidth, h: stage.clientHeight };
  }

  /* The Fit width mode is the stage itself, less the room the canvas keeps
     around a frame — the frame is still a frame, not the whole window. */
  function sizeFitMode() {
    if (stage.dataset.width !== 'fit') return;
    var v = view();
    frameShell.style.width = Math.max(320, v.w - INSET.left - INSET.right) + 'px';
    frameShell.style.height = Math.max(320, v.h - INSET.top - INSET.bottom) + 'px';
  }

  function apply() {
    var w = frameShell.offsetWidth;
    var h = frameShell.offsetHeight;
    var v = view();
    if (w && h && v.w && v.h) {
      x = Math.min(v.w - KEEP, Math.max(KEEP - w * z, x));
      y = Math.min(v.h - KEEP, Math.max(KEEP - h * z, y));
    }
    frameShell.style.transform = 'translate(' + x + 'px, ' + y + 'px) scale(' + z + ')';
    shell.style.setProperty('--wb-zoom', String(z));
    stage.style.backgroundPosition = x + 'px ' + y + 'px';
    label.hidden = frameShell.hidden || !w;
    label.style.transform = 'translate(' + x + 'px, ' + (y - 24) + 'px)';
    label.style.width = Math.max(0, w * z) + 'px';
    sizeText.textContent = Math.round(w) + ' × ' + Math.round(h);
    percent.textContent = Math.round(z * 100) + '%';
  }

  function fit() {
    fitted = true;
    sizeFitMode();
    var w = frameShell.offsetWidth;
    var h = frameShell.offsetHeight;
    var v = view();
    if (!w || !h || !v.w || !v.h) return apply();
    var aw = Math.max(1, v.w - INSET.left - INSET.right);
    var ah = Math.max(1, v.h - INSET.top - INSET.bottom);
    z = clamp(Math.min(1, aw / w, ah / h));
    x = INSET.left + (aw - w * z) / 2;
    y = INSET.top + (ah - h * z) / 2;
    apply();
  }

  /* Zoom about a point on the stage, keeping what is under it in place. */
  function zoomAt(next, cx, cy) {
    next = clamp(next);
    var wx = (cx - x) / z;
    var wy = (cy - y) / z;
    z = next;
    x = cx - wx * z;
    y = cy - wy * z;
    fitted = false;
    apply();
  }

  function zoomCentered(next) {
    var v = view();
    zoomAt(next, v.w / 2, v.h / 2);
  }

  function panBy(dx, dy) {
    x += dx;
    y += dy;
    fitted = false;
    apply();
  }

  function run(name) {
    closeMenu();
    if (name === 'in') zoomCentered(stepIn(z));
    else if (name === 'out') zoomCentered(stepOut(z));
    else if (name === 'actual') zoomCentered(1);
    else if (name === 'fit') fit();
    else if (typeof name === 'number') zoomCentered(name);
  }

  /* --------------------------------------------------------------- input */

  function stagePoint(clientX, clientY) {
    var r = stage.getBoundingClientRect();
    return { x: clientX - r.left, y: clientY - r.top };
  }

  function onWheel(e, offset) {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      var p = stagePoint(offset.x + e.clientX * offset.scale, offset.y + e.clientY * offset.scale);
      zoomAt(z * wheelFactor(e), p.x, p.y);
      return;
    }
    if (offset.inside) return; /* the page scrolls itself */
    e.preventDefault();
    var dx = e.deltaX;
    var dy = e.deltaY;
    if (e.shiftKey && !dx) {
      dx = dy;
      dy = 0;
    }
    if (e.deltaMode === 1) {
      dx *= 16;
      dy *= 16;
    }
    panBy(-dx, -dy);
  }

  stage.addEventListener('wheel', function (e) {
    onWheel(e, { x: 0, y: 0, scale: 1, inside: false });
  }, { passive: false });

  function onKey(e) {
    var name = command(e);
    if (!name) return false;
    if (!(e.metaKey || e.ctrlKey) && typing(e)) return false;
    if (e.preventDefault) e.preventDefault();
    if (e.type && e.type !== 'keydown') return true;
    run(name);
    return true;
  }

  document.addEventListener('keydown', onKey, true);

  /* The preview's own document: wheel and keys land there, not here, while
     the pointer or the focus is in the page. */
  function watch(frame) {
    var win;
    try {
      win = frame.contentWindow;
      if (!win || !win.document) return;
    } catch (e) {
      return;
    }
    if (win.__wbZoomWatched) return;
    win.__wbZoomWatched = true;
    win.addEventListener('wheel', function (e) {
      if (!frame.classList.contains('is-active')) return;
      var r = frame.getBoundingClientRect();
      onWheel(e, { x: r.left, y: r.top, scale: r.width / (frame.offsetWidth || r.width || 1), inside: true });
    }, { passive: false });
    win.addEventListener('keydown', onKey, true);
  }

  Array.prototype.forEach.call(document.querySelectorAll('.wb-frame iframe'), function (frame) {
    frame.addEventListener('load', function () { watch(frame); });
    watch(frame);
  });

  /* Space held, or the middle button, drags the canvas. While space is down
     the preview and the marks stop taking the pointer, so the drag can start
     anywhere on the stage. */
  var spaceDown = false;
  var panning = null;

  document.addEventListener('keydown', function (e) {
    if (e.code !== 'Space' || e.repeat || typing(e) || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.target && e.target.closest && e.target.closest('button, a, [role="menu"]')) return;
    spaceDown = true;
    stage.classList.add('is-space');
    e.preventDefault();
  });

  document.addEventListener('keyup', function (e) {
    if (e.code !== 'Space') return;
    spaceDown = false;
    stage.classList.remove('is-space');
  });

  root.addEventListener('blur', function () {
    spaceDown = false;
    stage.classList.remove('is-space');
  });

  stage.addEventListener('pointerdown', function (e) {
    if (!(spaceDown && e.button === 0) && e.button !== 1) return;
    e.preventDefault();
    panning = { id: e.pointerId, x: e.clientX, y: e.clientY };
    stage.setPointerCapture(e.pointerId);
    stage.classList.add('is-panning');
  }, true);

  stage.addEventListener('pointermove', function (e) {
    if (!panning || e.pointerId !== panning.id) return;
    panBy(e.clientX - panning.x, e.clientY - panning.y);
    panning.x = e.clientX;
    panning.y = e.clientY;
  });

  function endPan(e) {
    if (!panning || e.pointerId !== panning.id) return;
    panning = null;
    stage.classList.remove('is-panning');
  }

  stage.addEventListener('pointerup', endPan);
  stage.addEventListener('pointercancel', endPan);
  stage.addEventListener('lostpointercapture', endPan);

  /* ------------------------------------------------------------- control */

  var MENU = [
    { label: 'Zoom in', keys: '⌘ +', run: 'in' },
    { label: 'Zoom out', keys: '⌘ −', run: 'out' },
    { label: 'Zoom to fit', keys: '⇧ 1', run: 'fit' },
    { label: 'Zoom to 100%', keys: '⌘ 0', run: 'actual' },
    null,
    { label: 'Zoom to 50%', run: 0.5 },
    { label: 'Zoom to 200%', run: 2 },
  ];

  function closeMenu() {
    menu.hidden = true;
    percent.setAttribute('aria-expanded', 'false');
  }

  function openMenu() {
    menu.innerHTML = '';
    MENU.forEach(function (entry) {
      if (!entry) {
        var rule = document.createElement('span');
        rule.className = 'wb-menu-rule';
        rule.setAttribute('role', 'separator');
        menu.appendChild(rule);
        return;
      }
      var row = document.createElement('button');
      row.type = 'button';
      row.className = 'wb-menu-row wb-menu-keyed';
      row.setAttribute('role', 'menuitem');
      var text = document.createElement('span');
      text.textContent = entry.label;
      row.appendChild(text);
      if (entry.keys) {
        var keys = document.createElement('kbd');
        keys.textContent = entry.keys;
        row.appendChild(keys);
      }
      row.addEventListener('click', function () { run(entry.run); });
      menu.appendChild(row);
    });
    menu.hidden = false;
    percent.setAttribute('aria-expanded', 'true');
  }

  document.getElementById('zoomIn').addEventListener('click', function () { run('in'); });
  document.getElementById('zoomOut').addEventListener('click', function () { run('out'); });
  percent.addEventListener('click', function () {
    if (menu.hidden) openMenu(); else closeMenu();
  });
  document.addEventListener('pointerdown', function (e) {
    if (!menu.hidden && !control.contains(e.target)) closeMenu();
  });
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') closeMenu();
  });

  /* ------------------------------------------------------------- layout */

  /* A new frame size, a frame that just appeared, or a resized editor: fit
     again while the canvas is still fitted, else just keep it in bounds. */
  function relayout() {
    if (fitted) fit();
    else {
      sizeFitMode();
      apply();
    }
  }

  if (root.ResizeObserver) {
    var observer = new root.ResizeObserver(relayout);
    observer.observe(stage);
    observer.observe(frameShell);
  } else {
    root.addEventListener('resize', relayout);
  }
  new MutationObserver(relayout).observe(frameShell, { attributes: true, attributeFilter: ['hidden'] });

  root.wbZoom = {
    scale: function () { return z; },
    pan: function () { return { x: x, y: y }; },
    panTo: function (nx, ny) {
      x = nx;
      y = ny;
      fitted = false;
      apply();
    },
    /* A frame dragged to a new size keeps the zoom it was dragged at. */
    hold: function () { fitted = false; },
    fit: fit,
    update: apply,
    key: onKey,
    command: command,
  };
  root.wbZoomRules = { stepIn: stepIn, stepOut: stepOut, command: command, clamp: clamp, wheelFactor: wheelFactor };
  fit();
})(typeof window === 'undefined' ? globalThis : window);
