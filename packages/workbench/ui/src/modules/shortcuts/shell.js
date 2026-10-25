/* Existing shell shortcuts; preview documents keep their own keyboard handling. */
window.wbShellShortcuts = function (options) {
  var search = options.search;
  /* ⌘A is a text command. In a field or a note it selects the text there; over
     the chrome, where nothing is text, it would select the whole shell and the
     frame with it, so it does nothing. Pages inside the frame are their own
     documents and keep the browser's select-all. */
  document.addEventListener(
    'keydown',
    function (e) {
      if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== 'a')
        return;
      if (!window.wbKeys.editingTarget(e)) e.preventDefault();
    },
    true,
  );

  /* ⌘K / ctrl-K jumps to the filter, like every other tool — but only where
     there is one. Embedded, the filter is in the editor's sidebar, and this
     half has no business swallowing the chord. */
  document.addEventListener('keydown', function (e) {
    if (options.host()) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      search.focus();
      search.select();
    }
  });
};
