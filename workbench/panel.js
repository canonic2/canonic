/* The workbench, as an editor tab
   -------------------------------
   One webview panel holding one iframe holding the workbench. What this buys
   over `simpleBrowser.show`, which is what this replaces: no address bar, no
   back and forward, no reload button, no browser at all — a tab called
   Canonic with the design in it. The workbench's own toolbar already carries
   the two browser affordances that were ever wanted here (reload, and open
   this on its own), so the rest was chrome around chrome.

   It is a panel rather than a view in the sidebar for the obvious reason: a
   design needs the width. The sidebar has the list.

   One panel per window, reused. Picking a second screen moves the one that's
   open rather than stacking tabs, which is also why navigation goes through a
   message rather than a fresh src: the workbench stays loaded, keeps the
   toolbar the way you set it, and changes screen in a hash change.

   The webview and the workbench are different origins — vscode-webview:// and
   http://127.0.0.1 — so nothing here can read into the frame. It only ever
   tells it where to go, and the frame says where it went: the workbench posts
   a `wb-here` after every screen it routes to, whoever asked for it. That is
   the only way the sidebar can stay marked when the canvas moves on its own,
   which it does whenever a link is followed in a live preview.
*/

var vscode = require('vscode');

var VIEW_TYPE = 'canonic.workbench';
var TITLE = 'Canonic';

var panel = null;   /* the one open panel, or null */
var current = null; /* the hash the workbench has acknowledged with wb-here */
var where = { src: null, state: null }; /* the screen it settled on */
var watchers = [];

function moved(at) {
  where = at;
  watchers.forEach(function (fn) {
    fn(at);
  });
}

function nonce() {
  var out = '';
  var alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  for (var i = 0; i < 32; i++) out += alphabet.charAt(Math.floor(Math.random() * alphabet.length));
  return out;
}

/* Safe inside the generated inline script, including the unlikely case that
   an external URI contains the characters that close a script element. */
function scriptValue(value) {
  return JSON.stringify(String(value)).replace(/</g, '\\u003c');
}

/* The frame fills the tab, and one script forwards where-to-go messages from
   the extension into it. The CSP allows exactly one thing — framing the local
   workbench — and the workbench's own page is governed by its own origin. */
function html(url, origin) {
  var key = nonce();
  return [
    '<!doctype html>',
    '<html lang="en">',
    '<head>',
    '<meta charset="utf-8" />',
    '<meta http-equiv="Content-Security-Policy" content="' +
      "default-src 'none'; frame-src " + origin + "; style-src 'unsafe-inline'; " +
      "script-src 'nonce-" + key + "';\" />",
    '<style>',
    /* Nothing here is text. With the webview itself focused, VS Code answers
       ⌘A by selecting this document, which paints the whole frame blue and
       leaves it that way: no click in the frame can collapse a selection out
       here. So this document is not selectable at all. */
    '  html, body { height: 100%; margin: 0; padding: 0; background: transparent; user-select: none; -webkit-user-select: none; }',
    '  iframe { display: block; width: 100%; height: 100%; border: 0; user-select: none; -webkit-user-select: none; }',
    '</style>',
    '</head>',
    '<body>',
    '<iframe id="frame" title="Canonic workbench" allow="clipboard-read; clipboard-write"></iframe>',
    '<script nonce="' + key + '">',
    '  var editor = acquireVsCodeApi();',
    '  var frame = document.getElementById("frame");',
    '  var initial = ' + scriptValue(url) + ';',
    '  var ready = false;',
    '  var pending = null;',
    '  var refreshPending = false;',
    '  var refreshing = false;',
    '  var refreshBlocked = false;',
    '  var menuSelection = null;',
    '  /* Select all has no target here. The frame handles it in its own',
    '     fields; anywhere else it is dropped rather than handed to VS Code,',
    '     and a selection that still lands on this document is cleared. */',
    '  window.addEventListener("keydown", function (e) {',
    '    if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey ||',
    '        String(e.key || "").toLowerCase() !== "a") return;',
    '    e.preventDefault();',
    '    e.stopImmediatePropagation();',
    '  }, true);',
    '  document.addEventListener("selectionchange", function () {',
    '    var selection = document.getSelection();',
    '    if (selection && selection.rangeCount && !selection.isCollapsed) selection.removeAllRanges();',
    '  });',
    '  document.addEventListener("copy", function (e) {',
    '    if (!menuSelection || !e.clipboardData) return;',
    '    if (menuSelection.selection) e.clipboardData.setData("text/plain", menuSelection.selection);',
    '    menuSelection = null;',
    '    e.preventDefault();',
    '  });',
    '  document.addEventListener("cut", function (e) {',
    '    if (!menuSelection || !menuSelection.editable || !e.clipboardData) return;',
    '    if (!menuSelection.selection) { menuSelection = null; e.preventDefault(); return; }',
    '    e.clipboardData.setData("text/plain", menuSelection.selection);',
    '    menuSelection = null;',
    '    e.preventDefault();',
    '    frame.contentWindow.postMessage({ type: "wb-edit", command: "cut" }, "*");',
    '  });',
    '  document.addEventListener("paste", function (e) {',
    '    if (!menuSelection || !menuSelection.editable || !e.clipboardData) return;',
    '    menuSelection = null;',
    '    e.preventDefault();',
    '    frame.contentWindow.postMessage({ type: "wb-edit", command: "paste", text: e.clipboardData.getData("text/plain") }, "*");',
    '  });',
    '  function sendPending() {',
    '    if (!ready || refreshing || refreshBlocked || !pending || !frame.contentWindow) return;',
    '    /* A target is an instruction, not a request to poll until the child',
    '       echoes it. The workbench may reject a stale screen or normalize an',
    '       unknown state; retrying either response creates a message loop that',
    '       can starve the whole webview. Clear before posting so even a',
    '       synchronous test double cannot observe the target as pending. */',
    '    var target = pending;',
    '    pending = null;',
    '    frame.contentWindow.postMessage({ type: "wb-go", hash: target }, "*");',
    '  }',
    '  function sendRefresh() {',
    '    if (!ready || refreshing || !refreshPending || !frame.contentWindow) return false;',
    '    refreshPending = false;',
    '    /* Hold navigation until the child has loaded and acknowledged the',
    '       new config. Otherwise a click from the freshly rebuilt sidebar can',
    '       race the old in-memory index and be rejected as unknown. */',
    '    refreshing = true;',
    '    refreshBlocked = false;',
    '    frame.contentWindow.postMessage({ type: "wb-refresh" }, "*");',
    '    return true;',
    '  }',
    '  function relayKey(data) {',
    '    var kind = data.eventType === "keyup" ? "keyup" : data.eventType === "keydown" ? "keydown" : null;',
    '    var source = data.event || {};',
    '    if (!kind) return;',
    '    var init = {',
    '      key: String(source.key || ""), code: String(source.code || ""),',
    '      location: Number(source.location || 0), shiftKey: !!source.shiftKey,',
    '      altKey: !!source.altKey, ctrlKey: !!source.ctrlKey, metaKey: !!source.metaKey,',
    '      repeat: !!source.repeat, bubbles: true, cancelable: true',
    '    };',
    '    var event = new KeyboardEvent(kind, init);',
    '    var keyCode = Number(source.keyCode || 0);',
    '    Object.defineProperty(event, "keyCode", { get: function () { return keyCode; } });',
    '    Object.defineProperty(event, "which", { get: function () { return keyCode; } });',
    '    window.dispatchEvent(event);',
    '  }',
    '  window.addEventListener("message", function (e) {',
    '    var data = e.data || {};',
    '    if (data.type === "wb-context-reset" && e.source === frame.contentWindow) {',
    '      menuSelection = null;',
    '      return;',
    '    }',
    '    if (data.type === "wb-context-menu" && e.source === frame.contentWindow) {',
    '      menuSelection = { selection: String(data.selection || ""), editable: !!data.editable };',
    '      var rect = frame.getBoundingClientRect();',
    '      frame.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true,',
    '        clientX: rect.left + Number(data.x || 0), clientY: rect.top + Number(data.y || 0), button: 2 }));',
    '      return;',
    '    }',
    '    if (data.type === "wb-keyboard" && e.source === frame.contentWindow) {',
    '      relayKey(data);',
    '      return;',
    '    }',
    '    if (data.type === "canonic-go") {',
    '      /* A sidebar pick can beat the cross-origin frame to readiness.',
    '         Keep the latest one and send it after the first wb-here. */',
    '      pending = String(data.hash || "");',
    '      sendPending();',
    '      return;',
    '    }',
    '    if (data.type === "canonic-refresh") {',
    '      refreshPending = true;',
    '      sendRefresh();',
    '      return;',
    '    }',
    '    if (data.type === "wb-refresh-failed" && e.source === frame.contentWindow) {',
    '      refreshing = false;',
    '      refreshBlocked = true;',
    '      sendRefresh();',
    '      return;',
    '    }',
    '    if (data.type !== "wb-here" || e.source !== frame.contentWindow) return;',
    '    ready = true;',
    '    refreshing = false;',
    '    refreshBlocked = false;',
    '    /* A Storybook pick is not settled until that story renders. The',
    '       pending message only makes this iframe ready for later picks. */',
    '    if (!data.pending) editor.postMessage(data);',
    '    if (sendRefresh()) return;',
    '    sendPending();',
    '  });',
    '  /* Do not clear `ready` from the iframe load event. The child can post',
    '     wb-here before load reaches this wrapper; clearing it afterward',
    '     leaves every later sidebar pick queued forever. This frame is only',
    '     assigned once, and readiness already starts false. */',
    '  /* Assign src last. A fast local server used to answer before the',
    '     message listener existed, losing the initial readiness signal. */',
    '  frame.src = initial;',
    '</script>',
    '</body>',
    '</html>',
  ].join('\n');
}

/* Opens the workbench at `hash` ("pages/sign-in.html:error", or "" for
   whatever it was last on), reusing the open panel when there is one. */
function show(context, url, hash) {
  var target = String(hash || '');

  /* Remote and Codespaces hand out a different address than 127.0.0.1; local
     editors get the same string back. */
  return Promise.resolve(vscode.env.asExternalUri(vscode.Uri.parse(url))).then(function (external) {
    var address = external.toString();
    var origin = address.replace(/^([a-z]+:\/\/[^/]+).*$/i, '$1');

    if (panel) {
      /* Focus stays where it was, so you can walk the sidebar list with the
         arrow keys and watch the canvas follow. */
      panel.reveal(panel.viewColumn, true);
      /* Only wb-here advances current. If a pick arrived while the webview or
         iframe was loading, a repeat click must send it again rather than be
         discarded as though navigation had succeeded. No target still means
         "open the workbench" and leaves the current canvas alone. */
      if (target && target !== current) {
        panel.webview.postMessage({ type: 'canonic-go', hash: target });
      }
      return panel;
    }

    panel = vscode.window.createWebviewPanel(VIEW_TYPE, TITLE, vscode.ViewColumn.Active, {
      enableScripts: true,
      /* A design you tabbed away from should be there when you tab back,
         still at the width you set and still on the screen you were on. */
      retainContextWhenHidden: true,
      /* Nothing is loaded from disk; everything comes from the local server. */
      localResourceRoots: [],
    });

    panel.iconPath = vscode.Uri.joinPath(context.extensionUri, 'icon.svg');
    current = null;
    panel.webview.html = html(address + (target ? '#' + target : ''), origin);

    /* Every screen the workbench routes to, including the ones nobody here
       asked for. `current` follows it too, so a pick that matches where a
       link already took you isn't dropped as a repeat. */
    panel.webview.onDidReceiveMessage(function (message) {
      if (!message || message.type !== 'wb-here') return;
      current = message.src ? message.src + (message.state ? ':' + message.state : '') : null;
      moved({ src: message.src || null, state: message.state || null });
    });

    panel.onDidDispose(function () {
      panel = null;
      current = null;
      moved({ src: null, state: null });
    }, null, context.subscriptions);

    return panel;
  });
}

/* Who to tell when the canvas moves. Answers with the way to stop listening,
   for a view that is thrown away and rebuilt while the tab stays open. */
function onHere(fn) {
  watchers.push(fn);
  fn(where);
  return function () {
    watchers = watchers.filter(function (other) {
      return other !== fn;
    });
  };
}

/* The manifest changed under an open workbench. The child refreshes its
   in-memory config in place; a closed panel has nothing to update. */
function refresh() {
  if (!panel) return Promise.resolve(false);
  return panel.webview.postMessage({ type: 'canonic-refresh' });
}

module.exports = { show: show, onHere: onHere, refresh: refresh };
