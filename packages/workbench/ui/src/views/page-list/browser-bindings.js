/* Browser-only sidebar sizing; the sidebar webview does not load this binding. */
window.wbPageList.bindBrowser = function (options) {
  var shell = options.shell;
  var resizer = options.resizer;
  var rememberSidebar = options.remember;
  /* ----------------------------------------------------- sidebar resize */

  var SIDEBAR_MIN = 200;
  var SIDEBAR_MAX = 480;
  var SIDEBAR_DEFAULT = 248;

  function setSidebar(px) {
    var w = Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, px)));
    shell.style.setProperty('--wb-sidebar-w', w + 'px');
    return w;
  }

  var dragging = false;

  /* Pointer capture keeps the drag alive over the iframe, which would
     otherwise swallow the move events. No preventDefault here — it would
     suppress the compatibility dblclick that resets the width. The
     is-resizing class handles text selection instead. */
  resizer.addEventListener('pointerdown', function (e) {
    dragging = true;
    resizer.setPointerCapture(e.pointerId);
    shell.classList.add('is-resizing');
  });

  resizer.addEventListener('pointermove', function (e) {
    if (dragging) setSidebar(e.clientX);
  });

  function endDrag(e) {
    if (!dragging) return;
    dragging = false;
    shell.classList.remove('is-resizing');
    if (e.type === 'pointerup') rememberSidebar(setSidebar(e.clientX));
  }

  resizer.addEventListener('pointerup', endDrag);
  /* A cancelled or lost drag must clear the class too, or the shell keeps a
     resize cursor and a dead iframe. */
  resizer.addEventListener('pointercancel', endDrag);
  resizer.addEventListener('lostpointercapture', endDrag);

  resizer.addEventListener('dblclick', function () {
    rememberSidebar(setSidebar(SIDEBAR_DEFAULT));
  });

  /* Arrow keys nudge it once the handle has focus. */
  resizer.addEventListener('keydown', function (e) {
    var step = e.shiftKey ? 32 : 8;
    var width = parseInt(getComputedStyle(shell).getPropertyValue('--wb-sidebar-w'), 10);
    if (e.key === 'ArrowLeft') rememberSidebar(setSidebar(width - step));
    else if (e.key === 'ArrowRight') rememberSidebar(setSidebar(width + step));
    else return;
    e.preventDefault();
  });
  return setSidebar;
};
