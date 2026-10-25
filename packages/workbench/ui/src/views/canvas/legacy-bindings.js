/* Existing single-artboard shell rails; dimensions remain owned by the shell. */
window.wbCanvasShellBindings = function (options) {
  var canvas = options.canvas;
  var artboard = options.artboard;
  var shell = options.shell;
  var rememberFrameSize = options.remember;
  var showFrameSize = options.showFrameSize;
  /* Chrome-style rails sit outside the preview. Side rails change width, the
     bottom changes height, and either bottom corner changes both. Pointer
     travel is screen pixels, so it is divided by the zoom; a left-edge drag
     moves the frame over by what it grew, so the right edge stays put. */
  var artboardDrag = null;

  Array.prototype.forEach.call(document.querySelectorAll('.wb-artboard-resize'), function (handle) {
    handle.addEventListener('pointerdown', function (e) {
      if (canvas.dataset.size !== 'resizable') return;
      if (window.wbZoom) window.wbZoom.hold();
      artboardDrag = {
        edge: handle.dataset.resize,
        x: e.clientX,
        y: e.clientY,
        width: artboard.offsetWidth,
        height: artboard.offsetHeight,
        scale: window.wbZoom ? window.wbZoom.scale() : 1,
        pan: window.wbZoom ? window.wbZoom.pan() : null,
      };
      handle.setPointerCapture(e.pointerId);
      shell.classList.add('is-frame-resizing');
      e.preventDefault();
    });

    handle.addEventListener('pointermove', function (e) {
      if (!artboardDrag) return;
      var left = artboardDrag.edge.indexOf('left') > -1;
      var horizontal =
        (left ? artboardDrag.x - e.clientX : e.clientX - artboardDrag.x) / artboardDrag.scale;
      if (artboardDrag.edge !== 'bottom') {
        options.width = Math.max(1, Math.round(artboardDrag.width + horizontal));
        artboard.style.width = options.width + 'px';
        if (left && artboardDrag.pan) {
          window.wbZoom.panTo(
            artboardDrag.pan.x - (options.width - artboardDrag.width) * artboardDrag.scale,
            artboardDrag.pan.y,
          );
        }
      }
      if (artboardDrag.edge.indexOf('bottom') > -1) {
        options.height = Math.max(
          1,
          Math.round(artboardDrag.height + (e.clientY - artboardDrag.y) / artboardDrag.scale),
        );
        artboard.style.height = options.height + 'px';
      }
      showFrameSize();
    });

    function endFrameDrag() {
      if (!artboardDrag) return;
      artboardDrag = null;
      shell.classList.remove('is-frame-resizing');
      rememberFrameSize();
    }

    handle.addEventListener('pointerup', endFrameDrag);
    handle.addEventListener('pointercancel', endFrameDrag);
    handle.addEventListener('lostpointercapture', endFrameDrag);
  });
};
