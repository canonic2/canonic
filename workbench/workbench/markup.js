/* Markup layer
   ------------
   Draw over whatever the canvas is showing — scribble, line, arrow,
   rectangle, circle, text — then move, resize, and save the frame as a JPEG.

   These things shape this:
   - A warm background renderer mirrors the local preview's live DOM and this
     overlay continuously. Capture flushes pending state, then reads its native
     surface. The app runs only in the visible preview; no computed-style walk
     or foreignObject rendering is needed after an interaction.
   - The DOM renderer is retained only for standalone file:// use. Hosted
     capture failures are reported instead of starting an expensive fallback.
   - The camera asks the capture server for image bytes and downloads them.
     A handoff persists its image under the workspace's .canonic/.handoffs/
     folder and copies the prompt that points to it.
   - Marks live in an overlay sharing the frame's box but sitting outside its
     clip, so they move with the frame while a note can hang past its edge
     onto the stage — and taking the caret can never scroll the frame.
     Screenshots crop to the frame's box either way.
   - A mark keeps its geometry, not just its attributes, so it can be redrawn
     at a new position or size. Every one is <g class="wb-mark"> holding a fat
     transparent copy for hit-testing and the visible copy on top — a 3px
     outline is too thin to grab otherwise.
   - The layer itself never takes clicks; only marks and handles do. So Select
     mode leaves the preview underneath fully clickable, and clicking inside
     an empty rectangle hits the page, not the rectangle.

   One colour, one weight, outlines only. This annotates a design; it isn't a
   drawing app.

   Marks are cleared when the canvas loads a different file — a note belongs
   to the screen it was drawn on.
*/

(function () {
  var NS = 'http://www.w3.org/2000/svg';
  var SHAPE_ICONS = { line: 'minus', rect: 'square', ellipse: 'circle' };
  var SHAPE_NAMES = { line: 'Line', rect: 'Rectangle', ellipse: 'Circle' };
  var TAGS = { draw: 'path', line: 'line', arrow: 'line', rect: 'rect', ellipse: 'ellipse' };
  var MIN_DRAG = 3; /* below this a drag is a stray click, not a mark */
  var SAMPLE = 2; /* scribble points closer together than this are dropped */
  var PAD = 4; /* breathing room between a note and its selection box */
  var MIN_TEXT = 32; /* a note never narrows past this */

  /* Which edges of the box a handle drags. The opposite ones stay put. */
  var EDGES = {
    nw: { x: 'left', y: 'top' },
    n: { x: null, y: 'top' },
    ne: { x: 'right', y: 'top' },
    e: { x: 'right', y: null },
    se: { x: 'right', y: 'bottom' },
    s: { x: null, y: 'bottom' },
    sw: { x: 'left', y: 'bottom' },
    w: { x: 'left', y: null },
  };

  var shell = document.querySelector('.wb');
  var stage = document.getElementById('stage');
  var frameShell = document.getElementById('frameShell');
  var frameWrap = document.getElementById('frameWrap');
  var frame = document.getElementById('frame');
  var layer = document.getElementById('markupLayer');
  var svg = document.getElementById('markupSvg');
  var toast = document.getElementById('toast');
  var shapeButton = document.getElementById('shapeTool');
  var shapeCaret = document.getElementById('shapeCaret');
  var shapeMenu = document.getElementById('shapeMenu');
  var handoffControl = document.getElementById('handoffControl');
  var handoffButton = document.getElementById('handoff');
  var toolButtons = slice(document.querySelectorAll('.wb-tool'));
  var menuItems = slice(shapeMenu.querySelectorAll('.wb-menu-item'));

  /* The workbench double-buffers page navigation. Follow whichever iframe it
     promotes so inspection, screenshots, and handoff use the visible page. */
  var tool = 'pointer';
  var shape = shapeButton.dataset.tool; /* the shape the shape button draws */
  var marks = []; /* creation order — that's paint order and undo order */
  var selected = null;

  function slice(list) {
    return Array.prototype.slice.call(list);
  }

  function make(name, attrs) {
    var el = document.createElementNS(NS, name);
    for (var key in attrs) el.setAttribute(key, attrs[key]);
    return el;
  }

  function point(e) {
    var r = layer.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  /* ----------------------------------------------------------- the tools */

  function setTool(name) {
    tool = name;
    layer.classList.toggle('is-drawing', name !== 'pointer');
    layer.dataset.tool = name;
    toolButtons.forEach(function (b) {
      b.setAttribute('aria-pressed', String(b.dataset.tool === name));
    });
    /* What's held stays held through a tool change — picking up the arrow
       doesn't drop the box you were adjusting. */
  }

  function setShape(name) {
    shape = name;
    shapeButton.dataset.tool = name;
    shapeButton.title = SHAPE_NAMES[name];
    shapeButton.setAttribute('aria-label', SHAPE_NAMES[name]);
    shapeButton.innerHTML = '';
    shapeButton.appendChild(window.wbIcon(SHAPE_ICONS[name]));
    menuItems.forEach(function (b) {
      b.setAttribute('aria-current', String(b.dataset.shape === name));
    });
    setTool(name);
  }

  function show(menu, caret, open) {
    menu.hidden = !open;
    caret.setAttribute('aria-expanded', String(open));
  }

  function openMenu(open) {
    show(shapeMenu, shapeCaret, open);
  }

  function closeMenus() {
    openMenu(false);
  }

  /* ---------------------------------------------------------- the marks */

  /* The preview scrolls inside its iframe, while this layer deliberately
     sits beside that iframe so comments can overhang its clipped edge. Keep
     the layer in viewport coordinates and move every mark by the iframe's
     scroll delta. The two boxes remain siblings, so scrolling and overflow
     are both possible instead of one being traded for the other. */
  var watchedWindow = null;
  var previewScroll = { x: 0, y: 0 };
  var liveMirror = null;
  var bridgedScroll = null;
  var previewRevision = '';
  var revisionNumber = 0;

  function dirtyPreview(e) {
    if (e.isTrusted) scheduleNativePrepare();
  }

  function scrollOf(target) {
    if (target === frame && bridgedScroll) return { x: bridgedScroll.x, y: bridgedScroll.y };
    try {
      var win = target && target.contentWindow;
      return {
        x: win ? win.scrollX || win.pageXOffset || 0 : 0,
        y: win ? win.scrollY || win.pageYOffset || 0 : 0,
      };
    } catch (e) {
      /* A foreign preview supplies this through the cooperative bridge when
         installed. Until its first snapshot arrives, the viewport is at 0. */
      return { x: 0, y: 0 };
    }
  }

  function moveMark(mark, dx, dy) {
    var g = mark.geom;
    if (mark.type === 'line' || mark.type === 'arrow') {
      g.a = { x: g.a.x + dx, y: g.a.y + dy };
      g.b = { x: g.b.x + dx, y: g.b.y + dy };
    } else if (mark.type === 'text' || mark.type === 'comment') {
      g.x += dx;
      g.y += dy;
    } else {
      g.box.x += dx;
      g.box.y += dy;
    }
    render(mark);
  }

  function applyPreviewScroll(next) {
    var dx = previewScroll.x - next.x;
    var dy = previewScroll.y - next.y;
    previewScroll = next;
    if (!dx && !dy) return;

    marks.forEach(function (mark) {
      moveMark(mark, dx, dy);
    });
    drawSelection();
    scheduleNativePrepare();
  }

  function syncPreviewScroll() {
    applyPreviewScroll(scrollOf(frame));
  }

  function watchFrame(next) {
    if (frame) frame.removeEventListener('load', activeFrameLoaded);
    if (watchedWindow) {
      try {
        watchedWindow.removeEventListener('scroll', syncPreviewScroll);
        watchedWindow.removeEventListener('click', dirtyPreview, true);
        watchedWindow.removeEventListener('input', dirtyPreview, true);
        watchedWindow.removeEventListener('change', dirtyPreview, true);
        watchedWindow.removeEventListener('keydown', dirtyPreview, true);
      } catch (e) {}
    }

    frame = next;
    frame.addEventListener('load', activeFrameLoaded);
    watchedWindow = null;
    if (liveMirror) liveMirror.stop();
    liveMirror = null;
    bridgedScroll = null;
    previewRevision = Date.now() + '-' + (++revisionNumber);
    previewScroll = scrollOf(frame);
    try {
      watchedWindow = frame.contentWindow;
      if (frame.contentDocument && frame.contentDocument.documentElement && window.wbDOMMirror) {
        liveMirror = window.wbDOMMirror.create(frame.contentDocument, scheduleNativePrepare);
      }
      watchedWindow.addEventListener('scroll', syncPreviewScroll, { passive: true });
      watchedWindow.addEventListener('click', dirtyPreview, true);
      watchedWindow.addEventListener('input', dirtyPreview, true);
      watchedWindow.addEventListener('change', dirtyPreview, true);
      watchedWindow.addEventListener('keydown', dirtyPreview, true);
    } catch (e) {
      watchedWindow = null;
    }
    if (!liveMirror && window.wbPreviewBridge) {
      liveMirror = window.wbPreviewBridge.create(
        frame,
        scheduleNativePrepare,
        function (nextScroll) {
          bridgedScroll = nextScroll;
          applyPreviewScroll(nextScroll);
        },
        function (message) { console.info('[workbench] preview bridge could not mirror the page:', message); }
      );
    }
    scheduleNativePrepare();
  }

  /* The first iframe is already in place when markup.js runs. Later page
     changes promote the double buffer and announce its new active iframe. */
  watchFrame(frame);
  function activeFrameLoaded() {
    if (frame === this) watchFrame(this);
  }
  window.addEventListener('wb-frame-change', function (e) {
    watchFrame(e.detail.frame);
  });

  /* Geometry per type, all in layer pixels:
       line, arrow   { a: {x, y}, b: {x, y} }
       rect, ellipse { box: {x, y, w, h} }
       draw          { d, base: {x, y, w, h}, box: {x, y, w, h} }
       text, comment { x, y }
     Everything else — attributes, transforms — is derived in render(). */

  function render(mark) {
    var g = mark.geom;

    if (mark.type === 'text' || mark.type === 'comment') {
      /* A comment's point lands exactly where it was placed; the paper sits
         just to its right so it doesn't cover the thing being discussed. */
      mark.el.style.left = (mark.type === 'comment' ? g.x + 12 : g.x) + 'px';
      mark.el.style.top = (mark.type === 'comment' ? g.y - 18 : g.y) + 'px';
      /* Width is only set once the note has been pulled to one; until then it
         sizes to its words, capped by the stylesheet. */
      mark.el.style.width = g.w ? g.w + 'px' : '';
      mark.el.style.maxWidth = g.w ? 'none' : '';
      return;
    }

    mark.nodes.forEach(function (el) {
      if (mark.type === 'line' || mark.type === 'arrow') {
        el.setAttribute('x1', g.a.x);
        el.setAttribute('y1', g.a.y);
        el.setAttribute('x2', g.b.x);
        el.setAttribute('y2', g.b.y);
      } else if (mark.type === 'rect') {
        el.setAttribute('x', g.box.x);
        el.setAttribute('y', g.box.y);
        el.setAttribute('width', g.box.w);
        el.setAttribute('height', g.box.h);
      } else if (mark.type === 'ellipse') {
        el.setAttribute('cx', g.box.x + g.box.w / 2);
        el.setAttribute('cy', g.box.y + g.box.h / 2);
        el.setAttribute('rx', g.box.w / 2);
        el.setAttribute('ry', g.box.h / 2);
      } else {
        /* A scribble keeps its path and gets mapped from the box it was drawn
           in onto the box it's been dragged to. Mid-stroke there's no box yet
           — the path itself is being appended to. A flat one doesn't scale on
           its flat axis, either; there's nothing to scale. */
        if (!g.base) return;
        var sx = g.base.w ? g.box.w / g.base.w : 1;
        var sy = g.base.h ? g.box.h / g.base.h : 1;
        el.setAttribute(
          'transform',
          'translate(' + g.box.x + ' ' + g.box.y + ') scale(' + sx + ' ' + sy + ') ' +
            'translate(' + -g.base.x + ' ' + -g.base.y + ')'
        );
      }
    });
  }

  /* The box a selection is drawn around. Text measures itself. */
  function boxOf(mark) {
    if (mark.type === 'text' || mark.type === 'comment') {
      return {
        x: mark.el.offsetLeft,
        y: mark.el.offsetTop,
        w: mark.el.offsetWidth,
        h: mark.el.offsetHeight,
      };
    }
    return mark.geom.box;
  }

  function addMark(type, geom) {
    var group = make('g', { class: 'wb-mark' });
    var hit = make(TAGS[type], { class: 'wb-hit' });
    var shown = make(TAGS[type], {});
    if (type === 'arrow') shown.setAttribute('marker-end', 'url(#wb-arrowhead)');
    if (type === 'draw') {
      hit.setAttribute('d', geom.d);
      shown.setAttribute('d', geom.d);
    }
    group.appendChild(hit);
    group.appendChild(shown);
    /* Always under the selection chrome. */
    svg.insertBefore(group, sel);

    var mark = { type: type, el: group, nodes: [hit, shown], geom: geom };
    marks.push(mark);
    render(mark);
    return mark;
  }

  function addText(p) {
    var el = document.createElement('div');
    el.className = 'wb-markup-text';
    el.contentEditable = 'true';
    el.spellcheck = false;
    layer.appendChild(el);

    var mark = { type: 'text', el: el, nodes: [], geom: { x: p.x, y: p.y - 12 } };
    marks.push(mark);
    render(mark);

    /* An empty note is a mis-click and drops itself. Otherwise the note stops
       being editable, so a later click picks it up to move instead of putting
       a caret in it — double-click gets back in. */
    el.addEventListener('blur', function () {
      el.contentEditable = 'false';
      if (!el.textContent.trim()) remove(mark);
    });

    el.addEventListener('dblclick', function () {
      if (tool !== 'pointer') return;
      /* Editing, not handling — the box would go stale as the words grow. */
      select(null);
      el.contentEditable = 'true';
      el.focus({ preventScroll: true });
    });

    el.addEventListener('keydown', function (e) {
      /* Escape and ⌘Z belong to the note while it has the caret. */
      if (e.key === 'Escape') {
        e.stopPropagation();
        el.blur();
      }
    });

    el.focus({ preventScroll: true });
    return mark;
  }

  function addComment(p) {
    var el = document.createElement('div');
    el.className = 'wb-markup-comment';

    var body = document.createElement('div');
    body.className = 'wb-comment-body';
    body.contentEditable = 'true';
    body.spellcheck = true;
    body.dataset.placeholder = 'Add a comment…';

    el.appendChild(body);
    layer.appendChild(el);

    var mark = { type: 'comment', el: el, nodes: [], geom: { x: p.x, y: p.y } };
    marks.push(mark);
    render(mark);

    body.addEventListener('blur', function () {
      body.contentEditable = 'false';
      if (!body.textContent.trim()) remove(mark);
    });

    body.addEventListener('keydown', function (e) {
      if (e.key === 'Escape') {
        e.stopPropagation();
        body.blur();
      }
    });

    el.addEventListener('dblclick', function () {
      if (tool !== 'pointer') return;
      select(null);
      body.contentEditable = 'true';
      body.focus({ preventScroll: true });
    });

    body.focus({ preventScroll: true });
    return mark;
  }

  function markOf(node) {
    for (var i = 0; i < marks.length; i++) {
      if (marks[i].el === node) return marks[i];
    }
    return null;
  }

  function remove(mark) {
    var at = marks.indexOf(mark);
    if (at > -1) marks.splice(at, 1);
    mark.el.remove();
    if (selected === mark) select(null);
  }

  function undo() {
    var mark = marks[marks.length - 1];
    if (mark) remove(mark);
  }

  function clearAll() {
    while (marks.length) remove(marks[marks.length - 1]);
  }

  /* ------------------------------------------------------- selection */

  var sel = make('g', { class: 'wb-sel' });
  var selBox = make('rect', { class: 'wb-sel-box' });
  var handles = {};
  sel.appendChild(selBox);
  ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w', 'a', 'b'].forEach(function (name) {
    handles[name] = make('circle', { class: 'wb-handle', 'data-handle': name, r: 6 });
    sel.appendChild(handles[name]);
  });
  svg.appendChild(sel);

  function placeHandle(name, x, y) {
    handles[name].setAttribute('cx', x);
    handles[name].setAttribute('cy', y);
    handles[name].style.display = '';
  }

  function hideHandles() {
    for (var name in handles) handles[name].style.display = 'none';
  }

  function select(mark) {
    selected = mark;
    /* While something is held, the layer catches the next click on empty
       space so it can drop it — that click doesn't reach the preview. */
    layer.classList.toggle('has-selection', !!mark);
    drawSelection();
  }

  function drawSelection() {
    if (!selected) {
      sel.style.display = 'none';
      return;
    }

    sel.style.display = '';
    hideHandles();

    /* A line is two ends, not a box — dragging its corners would be a lie. */
    if (selected.type === 'line' || selected.type === 'arrow') {
      selBox.style.display = 'none';
      placeHandle('a', selected.geom.a.x, selected.geom.a.y);
      placeHandle('b', selected.geom.b.x, selected.geom.b.y);
      return;
    }

    var box = boxOf(selected);

    /* A note is only as tall as its words, so it stretches sideways: a box
       around the text and a grip on each side. A comment keeps its compact
       card width and only needs the selection frame. */
    if (selected.type === 'text' || selected.type === 'comment') {
      selBox.style.display = '';
      selBox.setAttribute('x', box.x - PAD);
      selBox.setAttribute('y', box.y - PAD);
      selBox.setAttribute('width', box.w + PAD * 2);
      selBox.setAttribute('height', box.h + PAD * 2);
      if (selected.type === 'text') {
        placeHandle('w', box.x - PAD, box.y + box.h / 2);
        placeHandle('e', box.x + box.w + PAD, box.y + box.h / 2);
      }
      return;
    }

    /* Shapes wear their own outline, so the grips sit straight on the box
       with no second frame around it. */
    selBox.style.display = 'none';
    var mx = box.x + box.w / 2;
    var my = box.y + box.h / 2;
    placeHandle('nw', box.x, box.y);
    placeHandle('n', mx, box.y);
    placeHandle('ne', box.x + box.w, box.y);
    placeHandle('e', box.x + box.w, my);
    placeHandle('se', box.x + box.w, box.y + box.h);
    placeHandle('s', mx, box.y + box.h);
    placeHandle('sw', box.x, box.y + box.h);
    placeHandle('w', box.x, my);
  }

  /* ---------------------------------------------------------- gestures */

  /* One shape for every drag. Capture is taken on whatever was pressed —
     that keeps the drag alive over the iframe, which would otherwise swallow
     the moves. */
  function drag(e, onMove, onEnd) {
    var owner = e.target;
    owner.setPointerCapture(e.pointerId);

    function move(ev) {
      onMove(point(ev), ev);
    }

    function up(ev) {
      owner.removeEventListener('pointermove', move);
      owner.removeEventListener('pointerup', up);
      owner.removeEventListener('pointercancel', up);
      if (owner.hasPointerCapture(ev.pointerId)) owner.releasePointerCapture(ev.pointerId);
      if (onEnd) onEnd(point(ev), ev);
    }

    owner.addEventListener('pointermove', move);
    owner.addEventListener('pointerup', up);
    owner.addEventListener('pointercancel', up);
  }

  /* Shift constrains, the way it does everywhere else: 45° off the other end
     for a line or an arrow, a square or a circle for the box shapes. */
  function constrain(from, to, kind, shifted) {
    if (!shifted) return to;
    var dx = to.x - from.x;
    var dy = to.y - from.y;

    if (kind === 'line' || kind === 'arrow') {
      var step = Math.PI / 4;
      var angle = Math.round(Math.atan2(dy, dx) / step) * step;
      var length = Math.sqrt(dx * dx + dy * dy);
      return { x: from.x + Math.cos(angle) * length, y: from.y + Math.sin(angle) * length };
    }

    var side = Math.max(Math.abs(dx), Math.abs(dy));
    return { x: from.x + (dx < 0 ? -side : side), y: from.y + (dy < 0 ? -side : side) };
  }

  function boxBetween(a, b) {
    return {
      x: Math.min(a.x, b.x),
      y: Math.min(a.y, b.y),
      w: Math.abs(b.x - a.x),
      h: Math.abs(b.y - a.y),
    };
  }

  /* A tool stays held once its mark is down, so a run of arrows or boxes is
     one pick rather than one each — and a stray click that draws nothing
     doesn't put the tool away either. Select, or Escape, hands it back.
     The new mark comes up held, so it can be nudged or resized straight away:
     grips and marks are live whichever tool is up, and empty space is where
     the held tool draws. */
  function finish(mark) {
    select(mark);
  }

  function startDraw(e) {
    var a = point(e);
    /* Starting a mark drops whatever was held — including when the drag turns
       out to be a click that draws nothing, which is how you let go of a mark
       without putting the tool away. */
    select(null);

    if (tool === 'text') {
      addText(a);
      return;
    }

    if (tool === 'comment') {
      addComment(a);
      return;
    }

    if (tool === 'draw') {
      var points = [a];
      var d = 'M' + a.x + ' ' + a.y;
      var scribble = addMark('draw', { d: d, base: null, box: null });

      drag(
        e,
        function (p) {
          var last = points[points.length - 1];
          if (Math.abs(p.x - last.x) < SAMPLE && Math.abs(p.y - last.y) < SAMPLE) return;
          points.push(p);
          d += ' L' + p.x + ' ' + p.y;
          scribble.nodes.forEach(function (el) {
            el.setAttribute('d', d);
          });
        },
        function () {
          if (points.length < 2) {
            remove(scribble);
            return;
          }
          /* The box it was drawn in is the box it gets scaled from later. */
          var bbox = scribble.nodes[1].getBBox();
          scribble.geom.d = d;
          scribble.geom.base = { x: bbox.x, y: bbox.y, w: bbox.width, h: bbox.height };
          scribble.geom.box = { x: bbox.x, y: bbox.y, w: bbox.width, h: bbox.height };
          render(scribble);
          finish(scribble);
        }
      );
      return;
    }

    var kind = tool;
    var mark =
      kind === 'line' || kind === 'arrow'
        ? addMark(kind, { a: a, b: a })
        : addMark(kind, { box: boxBetween(a, a) });

    drag(
      e,
      function (p, ev) {
        var b = constrain(a, p, kind, ev.shiftKey);
        if (kind === 'line' || kind === 'arrow') mark.geom.b = b;
        else mark.geom.box = boxBetween(a, b);
        render(mark);
      },
      function (p, ev) {
        var b = constrain(a, p, kind, ev.shiftKey);
        if (Math.abs(b.x - a.x) < MIN_DRAG && Math.abs(b.y - a.y) < MIN_DRAG) {
          remove(mark);
          return;
        }
        finish(mark);
      }
    );
  }

  function startMove(e, mark) {
    var origin = point(e);
    var g = mark.geom;
    var start =
      mark.type === 'line' || mark.type === 'arrow'
        ? { a: { x: g.a.x, y: g.a.y }, b: { x: g.b.x, y: g.b.y } }
        : mark.type === 'text' || mark.type === 'comment'
        ? { x: g.x, y: g.y }
        : { box: { x: g.box.x, y: g.box.y } };

    drag(e, function (p) {
      var dx = p.x - origin.x;
      var dy = p.y - origin.y;
      if (mark.type === 'line' || mark.type === 'arrow') {
        g.a = { x: start.a.x + dx, y: start.a.y + dy };
        g.b = { x: start.b.x + dx, y: start.b.y + dy };
      } else if (mark.type === 'text' || mark.type === 'comment') {
        g.x = start.x + dx;
        g.y = start.y + dy;
      } else {
        g.box.x = start.box.x + dx;
        g.box.y = start.box.y + dy;
      }
      render(mark);
      drawSelection();
    });
  }

  function startResize(e, name) {
    var mark = selected;
    var g = mark.geom;

    if (name === 'a' || name === 'b') {
      var other = name === 'a' ? g.b : g.a;
      drag(e, function (p, ev) {
        g[name] = constrain(other, p, mark.type, ev.shiftKey);
        render(mark);
        drawSelection();
      });
      return;
    }

    /* A note has no height of its own — the words set it. Its grips move the
       left or right edge and let the text rewrap. */
    if (mark.type === 'text') {
      var startBox = boxOf(mark);
      var right = startBox.x + startBox.w;
      drag(e, function (p) {
        if (name === 'e') {
          g.w = Math.max(MIN_TEXT, p.x - startBox.x);
        } else {
          g.w = Math.max(MIN_TEXT, right - p.x);
          g.x = right - g.w;
        }
        render(mark);
        drawSelection();
      });
      return;
    }

    var edges = EDGES[name];
    var start = { x: g.box.x, y: g.box.y, w: g.box.w, h: g.box.h };

    drag(e, function (p, ev) {
      var x0 = start.x;
      var x1 = start.x + start.w;
      var y0 = start.y;
      var y1 = start.y + start.h;
      var q = p;

      /* Shift squares off the corner that isn't moving. A side grip has no
         second axis to square against, so it ignores it. */
      if (edges.x && edges.y && ev.shiftKey) {
        q = constrain(
          { x: edges.x === 'left' ? x1 : x0, y: edges.y === 'top' ? y1 : y0 },
          p,
          mark.type,
          true
        );
      }

      if (edges.x === 'left') x0 = q.x;
      else if (edges.x === 'right') x1 = q.x;
      if (edges.y === 'top') y0 = q.y;
      else if (edges.y === 'bottom') y1 = q.y;

      g.box = boxBetween({ x: x0, y: y0 }, { x: x1, y: y1 });
      render(mark);
      drawSelection();
    });
  }

  /* Grips first, then marks, then the held tool. A mark that's already down
     can be picked up and adjusted without putting the tool away — only empty
     space draws. */
  layer.addEventListener('pointerdown', function (e) {
    if (e.button !== 0) return;

    var handle = e.target.closest('[data-handle]');
    if (handle) {
      e.preventDefault();
      startResize(e, handle.dataset.handle);
      return;
    }

    var node = e.target.closest('.wb-mark, .wb-markup-text, .wb-markup-comment');
    if (node) {
      /* A note with the caret in it is being typed, not dragged. */
      if (node.isContentEditable || (node.closest('.wb-markup-comment') && node.closest('.wb-markup-comment').querySelector('[contenteditable="true"]'))) return;
      e.preventDefault();
      select(markOf(node));
      startMove(e, selected);
      return;
    }

    if (tool !== 'pointer') {
      e.preventDefault();
      startDraw(e);
      return;
    }

    /* Empty space with something held: that click drops it. */
    select(null);
  });

  /* ------------------------------------------------------------- capture */

  var toastTimer = null;
  var toastCopy = '';

  function say(message, copy) {
    toastCopy = copy || '';
    toast.textContent = message;
    toast.classList.toggle('is-copyable', !!toastCopy);
    if (toastCopy) {
      toast.setAttribute('role', 'button');
      toast.setAttribute('tabindex', '0');
      toast.setAttribute('title', 'Copy ' + toastCopy);
    } else {
      toast.setAttribute('role', 'status');
      toast.removeAttribute('tabindex');
      toast.removeAttribute('title');
    }
    toast.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () {
      toast.hidden = true;
    }, 2400);
  }

  /* Browser failures join the extension's persistent session log without
     carrying page HTML, handoff prompts or image data. This request is best
     effort: losing diagnostics must not replace the error being diagnosed. */
  function diagnostic(level, event, details) {
    try {
      fetch('/_workbench/log', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ level: level, event: event, details: details || {} }),
        keepalive: true,
      }).catch(function () {});
    } catch (_) {}
  }

  function copyToastPath() {
    var path = toastCopy;
    if (!path) return;

    function copied() {
      say('Copied ' + path);
    }

    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(path).then(copied, function () {
        copyFallback(path, copied);
      });
      return;
    }
    copyFallback(path, copied);
  }

  function copyFallback(value, done) {
    var input = document.createElement('textarea');
    input.value = value;
    input.setAttribute('readonly', '');
    input.style.position = 'fixed';
    input.style.opacity = '0';
    shell.appendChild(input);
    input.select();
    var copied = document.execCommand('copy');
    input.remove();
    if (copied) done();
    else say('Couldn’t copy ' + value);
  }

  toast.addEventListener('click', copyToastPath);
  toast.addEventListener('keydown', function (e) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    copyToastPath();
  });

  /* The shell says what the canvas is showing — screen, state, lens, story.
     Anything asked before it has settled is the design as addressed. */
  function viewNow() {
    var view = window.wbView ? window.wbView() : null;
    if (view) return view;
    var target = window.wbTarget();
    return { src: target.src, state: target.state, stateLabel: null, lens: null, story: null, url: null, code: [] };
  }

  /* "#pages/sign-in.html@393"           -> "sign-in.jpg".
     "#pages/sign-in.html:error@393"     -> "sign-in-error.jpg", so two states
     of one screen don't land on top of each other in .canonic/.handoffs/ — and through a
     lens "sign-in-error-staging.jpg", so neither do the design and the
     implementation. A story stands where the state does. */
  function shotName() {
    var view = viewNow();
    var base = String(view.src || '').split('/').pop().replace(/\.[^.]+$/, '');
    var state = view.story ? view.story.state : view.state;
    if (state) base += '-' + state;
    if (view.lens) base += '-' + view.lens.key;
    return (base || 'canonic') + '.jpg';
  }

  /* Through a lens the frame is another origin's page: nothing here can read
     into it, and the same-origin capture routes can't take it. Off file://
     the design is the same origin as the shell — a null one — so this is
     only ever true for a lens. */
  function offOrigin() {
    var src = frame.getAttribute('src');
    if (!src) return false;
    try {
      return new URL(src, location.href).origin !== location.origin;
    } catch (e) {
      return true;
    }
  }

  function simulatorView() {
    var current = viewNow();
    return !!(current.lens && current.lens.kind === 'ios-simulator');
  }

  window.addEventListener('wb-say', function (e) {
    say(String((e.detail && e.detail.message) || ''));
  });

  function download(blob) {
    var url = URL.createObjectURL(blob);
    var link = document.createElement('a');
    link.href = url;
    link.download = shotName();
    shell.appendChild(link);
    link.click();
    link.remove();
    setTimeout(function () {
      URL.revokeObjectURL(url);
    }, 1000);
    say('Saved ' + link.download + ' to Downloads');
  }

  /* The workbench extension serves this page and takes the image: one POST and
     it's in the workspace's .canonic/.handoffs/ folder, named after the screen, with no
     folder to pick and no permission to grant. Served by anything else —
     Live Preview, a plain static server — nothing is listening, and the shot
     falls back to a download. */
  function post(blob) {
    return fetch('/_workbench/shot?name=' + encodeURIComponent(shotName()), {
      method: 'POST',
      headers: { 'Content-Type': blob.type || 'image/jpeg' },
      body: blob,
    })
      .then(function (res) {
        return res.json().catch(function () {
          throw new Error('the server answered ' + res.status);
        });
      })
      .then(function (result) {
        if (!result.ok) throw new Error(result.error || 'the server refused it');
        return result.file;
      });
  }

  function captureMarkup() {
    var copy = layer.cloneNode(true);
    copy.classList.remove('is-drawing', 'has-selection');

    /* Notes are the only user-authored HTML in the layer. Carry their words,
       not any rich markup a paste may have introduced. Shapes are generated
       SVG and can be transferred as-is. */
    var sourceText = layer.querySelectorAll('.wb-markup-text, .wb-comment-body');
    var clonedText = copy.querySelectorAll('.wb-markup-text, .wb-comment-body');
    for (var i = 0; i < clonedText.length; i++) {
      clonedText[i].textContent = sourceText[i].textContent;
      clonedText[i].removeAttribute('contenteditable');
    }
    Array.prototype.forEach.call(copy.querySelectorAll('.wb-hit, .wb-sel'), function (node) {
      node.remove();
    });
    return copy.innerHTML;
  }

  function captureRequest(includeMarkup, flush) {
    if (!frame || !frame.getAttribute('src')) return null;
    var box = frameWrap.getBoundingClientRect();
    var url = frame.src;
    var currentView = viewNow();
    if (currentView.lens && currentView.url) url = currentView.url;
    else try { if (frame.contentWindow.location.href !== 'about:blank') url = frame.contentWindow.location.href; } catch (_) {}
    var request = {
      url: url,
      width: Math.round(box.width),
      height: Math.round(box.height),
      scroll: scrollOf(frame),
      markup: includeMarkup ? captureMarkup() : '',
      revision: previewRevision,
      format: 'jpeg',
      mirror: liveMirror ? liveMirror.read(flush) : undefined,
    };
    Object.defineProperty(request, 'mirrorSource', { value: liveMirror });
    return request;
  }

  function traceCapture(path, request) {
    diagnostic('info', 'capture.browser.requested', {
      route: path,
      mirrored: !!request.mirror,
      requestedX: request.scroll.x,
      requestedY: request.scroll.y,
      width: request.width,
      height: request.height,
    });
  }

  function mirrorFetch(path, request, receive) {
    var source = request.mirrorSource;
    receive = receive || json;
    function send(payload) {
      return fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) }).then(receive);
    }
    return send(request).catch(function (error) {
      if (!source || !request.mirror || !/MIRROR_RESYNC/.test(error.message)) throw error;
      // Retry the exact requested state, even if the preview changed meanwhile.
      return send(Object.assign({}, request, { mirror: source.full(request.mirror) }));
    }).then(function (result) {
      if (source && request.mirror) source.acknowledge(request.mirror);
      return result;
    });
  }

  function json(res) {
    return res.json().catch(function () {
      throw new Error('the capture server answered ' + res.status);
    }).then(function (result) {
      if (!res.ok || !result.ok) throw new Error(result.error || 'native capture failed');
      return result;
    });
  }

  function image(res) {
    if (res.ok) return res.blob();
    return res.json().catch(function () { return {}; }).then(function (result) {
      throw new Error(result.error || 'the capture server answered ' + res.status);
    });
  }

  var nativeAvailable = true;
  var lastPreparedKey = null;
  var lastPreparedAt = 0;
  var captureSync = window.wbCaptureSync.create({
    // Mutations and input trigger updates; polling also catches CSSOM changes,
    // canvas drawing and animation phases that don't emit DOM mutations.
    heartbeat: 250,
    read: function () {
      if (frameShell.hidden || location.protocol === 'file:') return null;
      var request = captureRequest(true);
      if (!request || request.width < 1 || request.height < 1) return null;
      return { path: offOrigin() ? '/_workbench/capture/page/prepare' : '/_workbench/capture/prepare', payload: request };
    },
    prepare: function (request) {
      var p = request.payload;
      var key = JSON.stringify([request.path, p.url, p.revision, p.width, p.height, p.scroll, p.markup, p.mirror && p.mirror.revision]);
      if (nativeAvailable && key === lastPreparedKey && Date.now() - lastPreparedAt < 5000) return Promise.resolve();
      return mirrorFetch(request.path, request.payload).then(function (result) {
        lastPreparedKey = key; lastPreparedAt = Date.now(); return result;
      });
    },
    ready: function () { nativeAvailable = true; },
    failed: function (err) {
      if (nativeAvailable) {
        diagnostic('warn', 'capture.prepare.failed', { message: String(err.message || err) });
        console.info('[workbench] capture warm-up failed; retrying in the background', err);
      }
      nativeAvailable = false;
    },
  });

  function scheduleNativePrepare() {
    if (captureSync) captureSync.schedule();
  }
  scheduleNativePrepare();
  window.addEventListener('wb-export-start', function () { captureSync.pause(); });
  window.addEventListener('wb-export-end', function () { captureSync.resume(); });
  window.addEventListener('pagehide', function () { captureSync.stop(); if (liveMirror) liveMirror.stop(); });
  // Marks and text edits are prepared too, so a settled screenshot normally
  // reads the already-painted surface without laying the overlay out again.
  new MutationObserver(scheduleNativePrepare).observe(layer, { subtree: true, childList: true, attributes: true, characterData: true });

  if (window.ResizeObserver) {
    new ResizeObserver(function () {
      scheduleNativePrepare();
    }).observe(frameWrap);
  }

  function captured(result) {
    return { file: result.file, native: true, targets: result.targets || null };
  }

  /* Where each mark points, in the frame's own pixels, for a browser that
     can't be asked from here. Notes have no element to name. */
  function anchors() {
    return marks.map(function (mark) {
      return mark.type === 'text' ? null : intoFrame(anchor(mark));
    });
  }

  function nativeShot() {
    if (offOrigin()) {
      return Promise.reject(new Error(
        'the preview is served from elsewhere'
      ));
    }
    var request;
    try { request = captureRequest(true, true); } catch (error) { error.explained = true; return Promise.reject(error); }
    if (!request) return Promise.reject(new Error('there is no preview to capture'));
    if (!request.mirror) return Promise.reject(new Error('the live preview is still loading'));
    request.name = shotName();

    say('Capturing…');
    traceCapture('/_workbench/capture', request);
    return mirrorFetch('/_workbench/capture', request).then(captured);
  }

  /* A bridged lens uses the same inert live-DOM capture as a local page. An
     unbridged lens retains the URL capture fallback and its separate session. */
  function pageShot(inspectMarks) {
    var flushed = liveMirror && liveMirror.flush ? liveMirror.flush() : Promise.resolve();
    return flushed.then(function () {
      var request = captureRequest(true);
      if (!request) throw new Error('there is no preview to capture');
      request.name = shotName();
      request.anchors = inspectMarks ? anchors() : [];
      say('Capturing…');
      traceCapture('/_workbench/capture/page', request);
      return mirrorFetch('/_workbench/capture/page', request)
        .then(captured);
    });
  }

  function renderShot() {
    var box = frameShell.getBoundingClientRect();
    say('Rendering…');
    return window.modernScreenshot.domToBlob(frameShell, {
      width: Math.round(box.width),
      height: Math.round(box.height),
      scale: 1,
      type: 'image/jpeg',
      quality: 0.9,
      onCloneEachNode: function (node) {
        /* The SVG foreignObject rasterizer resolves SF Pro's variable 590
           weight as the much heavier static bold face on links. Use the
           nearest non-bold static face in the screenshot clone only; the
           live preview keeps the canonical 590 token. */
        if (node.tagName === 'A' && node.style && node.style.fontWeight === '590') {
          node.style.fontWeight = '500';
        }
      },
    });
  }

  function legacyShot() {
    if (!window.modernScreenshot) return Promise.reject(new Error('the DOM renderer did not load'));
    return renderShot().then(function (blob) {
      return post(blob).then(function (file) {
        return { blob: blob, file: file, native: false };
      }, function (error) {
        error.captureBlob = blob;
        throw error;
      });
    });
  }

  function takeShot(inspectMarks) {
    if (simulatorView()) return legacyShot();
    /* The DOM renderer can't see into another origin's frame either — it
       would paint a blank — so a lens's page has one route. */
    if (offOrigin()) return pageShot(inspectMarks);
    // A hosted workbench always captures its live mirror. Fail visibly if the
    // renderer is unavailable; never turn a click into seconds of DOM painting.
    if (location.protocol === 'file:') return legacyShot();
    // The capture includes a fresh snapshot. Avoid another speculative DOM
    // walk competing with it while the helper reads and saves those pixels.
    captureSync.pause();
    return Promise.resolve().then(nativeShot)
      .finally(function () { captureSync.resume(); });
  }

  /* The camera is an export, not a handoff. Ask the same native renderer for
     bytes without persisting them in the project, then let the browser save
     the named JPEG in its Downloads location. */
  function downloadShot() {
    if (simulatorView()) return renderShot();
    if (location.protocol === 'file:') return renderShot();
    var external = offOrigin();
    var capture = function () {
      var request;
      try { request = captureRequest(true, !external); } catch (error) { error.explained = true; return Promise.reject(error); }
      if (!request) return Promise.reject(new Error('there is no preview to capture'));
      if (!external && !request.mirror) return Promise.reject(new Error('the live preview is still loading'));
      request.anchors = [];
      say('Capturing…');
      traceCapture(external ? '/_workbench/capture/page/image' : '/_workbench/capture/image', request);
      return mirrorFetch(
        external ? '/_workbench/capture/page/image' : '/_workbench/capture/image',
        request,
        image
      );
    };
    if (external) {
      return Promise.resolve(liveMirror && liveMirror.flush ? liveMirror.flush() : null).then(capture);
    }
    captureSync.pause();
    return Promise.resolve().then(capture).finally(function () { captureSync.resume(); });
  }

  /* Native capture uses the continuously mirrored Chromium surface. Both it
     and standalone DOM capture return a 1× JPEG and saved file contract. */
  function shoot() {
    if (frameShell.hidden) {
      say('Pick a page or a component first.');
      return;
    }
    /* Handles and a blinking caret would land in the shot. */
    select(null);
    if (document.activeElement && document.activeElement.blur) document.activeElement.blur();

    downloadShot()
      .then(download)
      .catch(function (err) {
        diagnostic('error', 'screenshot.browser.failed', { message: String(err.message || err) });
        say(err.explained ? String(err.message) : 'Couldn’t render the frame.');
        console.error('[workbench] screenshot failed', err);
      });
  }

  /* ------------------------------------------------------------- handoff */

  /* The picture plus everything about it the canvas already knows, copied as
     a prompt. What makes this worth more than dragging
     the image into a chat is the element under each mark: the preview is
     same-origin over http, so instead of the agent inferring what a red box is
     drawn around, we ask the DOM and say so.

     The extension owns the clipboard, so served by anything else the control
     isn't shown at all. */

  function preview() {
    if (simulatorView()) return null;
    try {
      return frame.contentDocument;
    } catch (e) {
      /* Opened off file:// — the preview is a foreign origin and can't be read
         into. The marks still go over, just without their elements. */
      return null;
    }
  }

  /* Layer pixels are the frame's, but the iframe is its own box inside it, so
     go through the rects rather than assuming they share an origin. */
  function intoFrame(p) {
    var lr = layer.getBoundingClientRect();
    var fr = frame.getBoundingClientRect();
    return { x: p.x + lr.left - fr.left, y: p.y + lr.top - fr.top };
  }

  /* The element under a point and its name are describe.js's — the same
     file the server puts into a page its browser opens, so a mark is named
     the same way whichever side is asked. */
  function elementAt(doc, p) {
    var q = intoFrame(p);
    return window.wbDescribe.elementAt(doc, q.x, q.y);
  }

  function describe(el) {
    return window.wbDescribe.describe(el);
  }

  /* The line ends under the middle of the arrow marker, whose triangle carries
     on to its visible tip. Handoff coordinates and hit-testing need that tip,
     not the hidden endpoint, or a close annotation can name the element just
     beyond the thing it visibly points at. */
  function arrowTip(mark) {
    var a = mark.geom.a;
    var b = mark.geom.b;
    var dx = b.x - a.x;
    var dy = b.y - a.y;
    var length = Math.sqrt(dx * dx + dy * dy);
    if (!length) return b;

    var marker = document.getElementById('wb-arrowhead');
    var overhang = marker
      ? marker.markerWidth.baseVal.value - marker.refX.baseVal.value
      : 0;
    return {
      x: b.x + (dx / length) * overhang,
      y: b.y + (dy / length) * overhang,
    };
  }

  /* Where a mark is really pointing: an arrow means its visible tip, a line
     means between its ends, and a shape means what it's drawn around. */
  function anchor(mark) {
    var g = mark.geom;
    if (mark.type === 'arrow') return arrowTip(mark);
    if (mark.type === 'line') return { x: (g.a.x + g.b.x) / 2, y: (g.a.y + g.b.y) / 2 };
    if (mark.type === 'comment') return { x: g.x, y: g.y };
    var box = boxOf(mark);
    return { x: box.x + box.w / 2, y: box.y + box.h / 2 };
  }

  function specOf(mark) {
    var spec = { type: mark.type };

    if (mark.type === 'arrow') {
      spec.from = mark.geom.a;
      spec.to = arrowTip(mark);
    } else if (mark.type === 'line') {
      spec.from = mark.geom.a;
      spec.to = mark.geom.b;
    } else {
      spec.box = boxOf(mark);
    }

    if (mark.type === 'text' || mark.type === 'comment') {
      spec.text = mark.el.textContent.replace(/\s+/g, ' ').trim();
      if (mark.type === 'text') {
        /* A freeform note says what it means; the element under its own box is
           whatever it happens to be covering, which is noise. */
        return spec;
      }
    }

    var doc = preview();
    spec.target = doc ? describe(elementAt(doc, anchor(mark))) : null;
    return spec;
  }

  /* The width switch names itself better than "393" does — an agent told
     "iPhone 15 Pro" knows more than one told a number. */
  function widthName() {
    var mode = stage.dataset.width;
    var button = document.querySelector('.wb-width[data-width="' + mode + '"]');
    return button ? button.title : mode;
  }

  /* An agent told "Wrong password" knows which of the screen's versions it is
     looking at; one told "error" has to guess. The shell already has the
     label — and, through a Storybook lens, the story's name in its place. */
  function stateName(view) {
    if (view.stateLabel) return view.stateLabel;
    var item = window.wbItem(view.src);
    var states = (item && item.states) || [];
    for (var i = 0; i < states.length; i++) {
      if (states[i].id === view.state) return states[i].label;
    }
    return view.state;
  }

  /* Through a lens the prompt also says which implementation, at what
     address, and where its code is — and whether the elements under the
     marks could be read at all. On the design none of that is sent, and the
     prompt reads as it always has. */
  function payload(file, targets) {
    var box = frameWrap.getBoundingClientRect();
    var view = viewNow();
    var item = window.wbItem(view.src);
    /* The elements under the marks: read here when the frame can be read
       into, else what the browser that took the shot answered. */
    var readable = preview() !== null;
    var out = {
      file: file,
      src: view.src,
      label: item ? item.label : view.src,
      state: view.state ? stateName(view) : null,
      width: widthName(),
      frame: { w: Math.round(box.width), h: Math.round(box.height) },
      marks: marks.map(function (mark, i) {
        var spec = specOf(mark);
        if (!readable) spec.target = (targets && targets[i]) || null;
        return spec;
      }),
    };
    if (view.lens) {
      out.lens = { key: view.lens.key, label: view.lens.label, kind: view.lens.kind, url: view.url };
      out.inspected = readable || !!targets;
    }
    if (view.story) out.story = { id: view.story.id, name: view.story.name };
    if (view.code && view.code.length) out.code = view.code.slice();
    return out;
  }

  var handoffBusy = false;

  function finishHandoff() {
    handoffBusy = false;
    handoffButton.disabled = false;
    handoffButton.removeAttribute('aria-busy');
  }

  /* Same shot as the camera takes — saved first, so the prompt can point at a
     file that's already on disk rather than carrying an image. */
  function handoff() {
    if (handoffBusy) return;
    if (frameShell.hidden) {
      say('Pick a page or a component first.');
      return;
    }
    handoffBusy = true;
    handoffButton.disabled = true;
    handoffButton.setAttribute('aria-busy', 'true');
    var targets = null;
    var handoffFile = null;
    var phase = 'capture';
    return takeShot(true)
      .then(function (captured) {
        targets = captured.targets || null;
        handoffFile = captured.file || null;
        var handoffPayload = payload(captured.file, targets);
        clearAll();
        phase = 'send';
        return fetch('/_workbench/handoff', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(handoffPayload),
        }).then(function (res) {
          return res.json();
        });
      })
      .then(function (result) {
        if (!result.ok) {
          var failure = new Error(result.error || 'the editor refused it');
          failure.explained = true;
          throw failure;
        }
        say('Copied handoff to clipboard.');
        finishHandoff();
      })
      .catch(function (err) {
        diagnostic('error', 'handoff.browser.failed', {
          phase: phase,
          file: handoffFile,
          target: 'clipboard',
          message: String(err.message || err),
        });
        say(err.explained ? String(err.message) : 'Couldn’t hand off — see the Canonic Workbench log.');
        console.error('[workbench] handoff failed', err);
        finishHandoff();
      });
  }

  fetch('/_workbench/handoff')
    .then(function (res) {
      return res.json();
    })
    .then(function (result) {
      if (!result.ok || !result.available) throw new Error('clipboard handoff is unavailable');
      handoffControl.hidden = false;
    })
    .catch(function (err) {
      /* Not the extension serving this. A button that can't copy through the
         editor does not belong in the bar. */
      console.warn('[workbench] no handoff available', err);
    });

  /* -------------------------------------------------------------- wiring */

  toolButtons.forEach(function (b) {
    if (b === shapeButton) return;
    b.addEventListener('click', function () {
      /* Clicking the tool you're already holding puts it back. */
      setTool(tool === b.dataset.tool ? 'pointer' : b.dataset.tool);
      closeMenus();
    });
  });

  shapeButton.addEventListener('click', function () {
    setTool(tool === shape ? 'pointer' : shape);
    closeMenus();
  });

  shapeCaret.addEventListener('click', function () {
    openMenu(shapeMenu.hidden);
  });

  menuItems.forEach(function (b) {
    b.addEventListener('click', function () {
      setShape(b.dataset.shape);
      openMenu(false);
    });
  });

  /* Anything outside the layer drops the selection — including a click into
     the preview, which lands on the iframe and takes focus with it. */
  document.addEventListener('pointerdown', function (e) {
    if (!shapeMenu.hidden && !e.target.closest('.wb-shape')) openMenu(false);
    if (selected && !e.target.closest('.wb-markup-layer')) select(null);
  });

  window.addEventListener('blur', function () {
    select(null);
  });

  document.getElementById('undo').addEventListener('click', undo);
  document.getElementById('clearMarkup').addEventListener('click', clearAll);
  document.getElementById('shoot').addEventListener('click', shoot);
  document.getElementById('handoff').addEventListener('click', handoff);

  /* Inside a note or the filter box, the keyboard is the browser's. */
  function typing() {
    var el = document.activeElement;
    if (!el) return false;
    return el.isContentEditable || el.tagName === 'INPUT' || el.tagName === 'TEXTAREA';
  }

  var NUDGE = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      closeMenus();
      if (tool !== 'pointer') setTool('pointer');
      else select(null);
      return;
    }

    if (typing()) return;

    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key.toLowerCase() === 'z') {
      e.preventDefault();
      undo();
      return;
    }

    if (!selected) return;

    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      remove(selected);
      return;
    }

    var step = NUDGE[e.key];
    if (!step) return;
    e.preventDefault();
    var by = e.shiftKey ? 8 : 1;
    moveMark(selected, step[0] * by, step[1] * by);
    drawSelection();
  });

  /* A different file on the canvas means the notes no longer describe what's
     under them. Width changes keep them: the frame resizes, the marks stay
     where they were drawn on it. So does a change of lens: the marks were
     drawn on the design, and seeing the implementation under them is the
     comparison. */
  function screenOf(hash) {
    var target = window.wbAddress.parse(hash);
    return target.src + ':' + (target.state || '');
  }

  var loaded = screenOf(location.hash);
  window.addEventListener('hashchange', function () {
    var next = screenOf(location.hash);
    if (next === loaded) return;
    loaded = next;
    clearAll();
    setTool('pointer');
    closeMenus();
  });

  setShape(shape);
  setTool('pointer');
  select(null);
})();
