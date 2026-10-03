/* Keyboard bridge
   ---------------
   VS Code forwards key events from a webview to its keybinding service, but
   events do not cross an iframe boundary. Canonic has two of those: the
   workbench is framed by the editor tab, and the live page is framed by the
   workbench. Each nested document uses this helper to pass editor-shaped
   chords one level out until the webview can hand them back to VS Code.

   Text-editing chords stay with the focused page. In particular, copy,
   paste, select-all and undo must operate on an input or a note rather than
   on whichever editor happened to be active before the workbench. */
(function () {
  var LOCAL_COMMAND_KEYS = { a: true, c: true, v: true, x: true, y: true, z: true };

  function editable(target) {
    if (!target || target.nodeType !== 1) return false;
    if (target.isContentEditable) return true;
    var tag = String(target.tagName || '').toUpperCase();
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
  }

  /* Keyboard events are retargeted at a custom-element boundary, so a text
     field inside a shadow root looks like its host from up here. Follow the
     composed path back to the real control. For contenteditable descendants,
     return the outer editing host so select-all gets the whole note. */
  function editingTarget(e) {
    var path = e.composedPath ? e.composedPath() : [e.target];
    var contenteditable = null;
    for (var i = 0; i < path.length; i++) {
      var node = path[i];
      if (!node || node.nodeType !== 1) continue;
      var tag = String(node.tagName || '').toUpperCase();
      if (tag === 'INPUT' || tag === 'TEXTAREA') return node;
      if (node.isContentEditable) contenteditable = node;
      else if (contenteditable) return contenteditable;
    }
    return contenteditable;
  }

  function selectedText(field) {
    /* Match native copy restrictions without reading a password's value. */
    if (field && field.tagName === 'INPUT' && field.type === 'password') return '';
    if (field && (field.tagName === 'INPUT' || field.tagName === 'TEXTAREA')) {
      return typeof field.selectionStart === 'number'
        ? field.value.slice(field.selectionStart, field.selectionEnd) : '';
    }
    return String(document.getSelection() || '');
  }

  /* VS Code can also apply the same physical ⌘A to the editor-tab webview.
     Handle it where the focused field lives and cancel that outer default;
     otherwise the field is left alone while the whole workbench is selected. */
  function selectAll(e) {
    if (e.type !== 'keydown' || e.shiftKey || e.altKey || !(e.metaKey || e.ctrlKey) ||
        String(e.key || '').toLowerCase() !== 'a') return false;

    var target = editingTarget(e);
    if (!target) return false;

    var tag = String(target.tagName || '').toUpperCase();
    if ((tag === 'INPUT' || tag === 'TEXTAREA') && typeof target.select === 'function') {
      target.select();
    } else {
      var doc = target.ownerDocument;
      var selection = doc && doc.getSelection ? doc.getSelection() : null;
      if (!selection || !doc.createRange) return false;
      var range = doc.createRange();
      range.selectNodeContents(target);
      selection.removeAllRanges();
      selection.addRange(range);
    }

    e.preventDefault();
    return true;
  }

  /* ⌘= ⌘- ⌘0 zoom the workbench canvas, the way they zoom a Figma canvas,
     rather than the editor window around it. zoom.js handles them. */
  function zoomChord(e) {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return false;
    var key = String(e.key || '');
    var code = String(e.code || '');
    return key === '=' || key === '+' || key === '-' || key === '_' || key === '0' ||
      /^(Equal|Minus|Digit0|NumpadAdd|NumpadSubtract|Numpad0)$/.test(code);
  }

  function belongsToEditor(e) {
    var key = String(e.key || '');
    var lower = key.toLowerCase();
    var command = e.metaKey || e.ctrlKey;

    /* These must keep working in both form fields and markup notes. They are
       deliberately local everywhere: a selection in a preview is still a
       real browser selection even when its target is not itself editable. */
    if (e.getModifierState && e.getModifierState('AltGraph')) return false;
    if (zoomChord(e)) return false;
    if (command && !e.altKey && LOCAL_COMMAND_KEYS[lower] &&
        (!e.shiftKey || lower === 'z' || lower === 'v')) return false;
    if (editingTarget(e) && /^(ArrowLeft|ArrowRight|ArrowUp|ArrowDown|Home|End|Backspace|Delete)$/.test(key)) return false;
    if (command) return true;
    if (e.altKey) return !editingTarget(e) && !editable(e.target);
    return /^F(?:[1-9]|1[0-2])$/.test(key);
  }

  function browserOwns(e) {
    if (e.type !== 'keydown') return false;
    var lower = String(e.key || '').toLowerCase();
    var command = e.metaKey || e.ctrlKey;
    return (command && (lower === 'p' || lower === 'f' || lower === 's')) ||
      lower === 'f1' || lower === 'f5';
  }

  function packet(e) {
    return {
      type: 'wb-keyboard',
      eventType: e.type,
      event: {
        key: String(e.key || ''),
        keyCode: Number(e.keyCode || 0),
        code: String(e.code || ''),
        location: Number(e.location || 0),
        shiftKey: !!e.shiftKey,
        altKey: !!e.altKey,
        ctrlKey: !!e.ctrlKey,
        metaKey: !!e.metaKey,
        repeat: !!e.repeat,
      },
    };
  }

  function relay(destination, options) {
    if (!destination || destination === window) return;
    options = options || {};
    var hasContext = false;

    function clearContext() {
      if (!hasContext) return;
      hasContext = false;
      destination.postMessage({ type: 'wb-context-reset' }, '*');
    }

    function send(e) {
      if (options.enabled && !options.enabled()) return;
      if (e.type === 'keydown') clearContext();
      if (selectAll(e)) return;
      /* Electron's native edit commands target the outer webview document.
         Execute copy/cut in the document that actually owns the selection. */
      var key = String(e.key || '').toLowerCase();
      if (e.type === 'keydown' && (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey &&
          (key === 'c' || key === 'x')) {
        if (document.execCommand(key === 'c' ? 'copy' : 'cut')) e.preventDefault();
        return;
      }
      /* Forward unmodified keys too: they can complete a VS Code chord.
         Their normal text/navigation behavior in the preview stays native. */
      if (!belongsToEditor(e) && (e.metaKey || e.ctrlKey || e.altKey)) return;
      if (browserOwns(e)) e.preventDefault();
      destination.postMessage(packet(e), '*');
    }

    window.addEventListener('keydown', send, true);
    window.addEventListener('keyup', send, true);
    window.addEventListener('pointerdown', clearContext, true);

    /* Native webview menus only hear events from the outer document. Carry
       the preview's menu request and selection up without moving focus. */
    window.addEventListener('contextmenu', function (e) {
      if ((options.enabled && !options.enabled()) || e.defaultPrevented) return;
      var field = editingTarget(e);
      var selection = selectedText(field);
      e.preventDefault();
      hasContext = true;
      destination.postMessage({
        type: 'wb-context-menu', x: e.clientX, y: e.clientY,
        selection: selection, editable: !!field,
      }, '*');
    });

    window.addEventListener('message', function (e) {
      if (e.source !== destination || !e.data || e.data.type !== 'wb-edit' ||
          (options.enabled && !options.enabled())) return;
      var data = e.data;
      var active = document.activeElement;
      while (active && active.shadowRoot && active.shadowRoot.activeElement) active = active.shadowRoot.activeElement;
      if (active && active.tagName === 'IFRAME') {
        active.contentWindow.postMessage(data, '*');
      } else if (active && editable(active)) {
        if (data.command === 'paste' && typeof data.text === 'string') document.execCommand('insertText', false, data.text);
        /* Selection may have collapsed since the menu opened. Delete would
           otherwise remove a character at the caret instead of cutting. */
        else if (data.command === 'cut' && selectedText(active)) document.execCommand('delete');
      }
    });
  }

  function forward(message, destination) {
    if (!destination || !message || message.type !== 'wb-keyboard') return;
    destination.postMessage(message, '*');
  }

  /* Storybook forwards preview keydowns on its own channel. Use that bridge
     when the implementation iframe is on another origin. */
  function storybook(e, frame, url) {
    if (e.source !== frame.contentWindow || e.origin !== new URL(url).origin) return null;
    var data = e.data;
    if (typeof data === 'string') {
      try { data = JSON.parse(data); } catch (error) { return null; }
    }
    if (!data || data.key !== 'storybook-channel' || !data.event || data.event.type !== 'previewKeydown') return null;
    var key = data.event.args && data.event.args[0] && data.event.args[0].event;
    if (!key) return null;
    /* Zoom chords come back too, for the canvas rather than the editor. */
    if (!zoomChord(key) && !belongsToEditor(key) && (key.metaKey || key.ctrlKey || key.altKey)) return null;
    return packet(Object.assign({}, key, { type: 'keydown' }));
  }

  window.wbKeys = {
    relay: relay, forward: forward, belongsToEditor: belongsToEditor, storybook: storybook,
    editingTarget: editingTarget, zoomChord: zoomChord,
  };
})();
