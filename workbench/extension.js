/* Workbench — the editor half
   -------------------------
   Starts the workbench server when a project that has a workbench opens, and
   says so in the status bar when a screenshot lands.
   Everything that isn't editor-shaped lives in server.js.

   This is installed once, globally, and knows nothing about any particular
   project: which folder it serves is whichever open one carries a workbench.
*/

var vscode = require('vscode');
var server = require('./server');
var startup = require('./startup');
var screens = require('./screens');
var panel = require('./panel');
var path = require('path');
var fs = require('fs');

/* The workbench ships in here, so what says a project wants one is the project
   naming its screens: workbench.yaml at the root. The manifest activates this
   extension on the same file; this is the second half of that check, and the
   one that decides which folder gets served when a window holds several. First
   one wins — a window with two of them is a situation, not a default to guess
   at. */
var MARKER = ['workbench.yaml'];

function workbenchRoot() {
  var folders = vscode.workspace.workspaceFolders || [];
  for (var i = 0; i < folders.length; i++) {
    var root = folders[i].uri.fsPath;
    if (fs.existsSync(path.join.apply(path, [root].concat(MARKER)))) return root;
  }
  return null;
}

/* Everything that only makes sense once there is a workbench: the server, and
   shutting it down again. Answers with the running server, or null if it
   couldn't start. The canvas opens from the Workbench view in the activity bar
   or the Open Workbench command; there is no status bar item. */
function serve(context, root, diagnostics) {
  /* One server for the window, started once and awaited by every command —
     so clicking before it's up waits rather than starting a second one. */
  var ready = Promise.resolve().then(function () {
    return startup.run(root, {
      isTrusted: vscode.workspace.isTrusted,
      createTerminal: function (options) { return vscode.window.createTerminal(options); },
      report: function (message) { diagnostics.info('implementation.start ' + message); },
    });
  }).catch(function (error) {
    diagnostics.warn('implementation.start ' + String(error.message || error));
  }).then(function () {
    return server.start({
      root: root,
      eagerCapture: true,
      captureStorage: path.join(context.globalStorageUri.fsPath, 'capture'),
      chromePath: vscode.workspace.getConfiguration('canonic').get('capture.chromePath') || undefined,
      simulatorPermissionOwner: vscode.env.appName || 'Visual Studio Code',
      onLog: function (record) {
        var details = Object.assign({ workspace: root }, record.details || {});
        var line = record.event + ' ' + JSON.stringify(details);
        var write = diagnostics[record.level] || diagnostics.info;
        write.call(diagnostics, line);
      },
      onShot: function (file) {
        vscode.window.setStatusBarMessage('$(device-camera) Saved ' + file, 4000);
      },

      /* A handoff is deliberately just a saved screenshot and a prompt on the
         clipboard. It does not move focus or depend on another extension. */
      onHandoff: function (prompt) {
        return vscode.env.clipboard.writeText(prompt).then(function () {
          vscode.window.showInformationMessage('Handoff copied — ⌘V to paste it into a conversation.');
        });
      },

      /* A screen's source, from the workbench's source menu: a file opens as
         a tab; a folder is shown in the Explorer when it's in this window,
         and in the system's file browser when it isn't. */
      onOpen: function (file) {
        var uri = vscode.Uri.file(file);
        if (!fs.statSync(file).isDirectory()) {
          return vscode.window.showTextDocument(uri, { preview: false });
        }
        var inWindow = (vscode.workspace.workspaceFolders || []).some(function (folder) {
          var top = folder.uri.fsPath;
          return file === top || file.indexOf(top + path.sep) === 0;
        });
        if (inWindow) return vscode.commands.executeCommand('revealInExplorer', uri);
        return vscode.env.openExternal(uri);
      },
    });
  })
    .catch(function (err) {
      vscode.window.showErrorMessage(
        'Workbench couldn’t start: ' + String(err.message || err)
      );
      return null;
    });

  context.subscriptions.push({
    dispose: function () {
      ready.then(function (running) {
        if (running) running.close();
      });
    },
  });

  return ready;
}

function activate(context) {
  /* Installed once for every project, this wakes up in plenty of them that
     have no workbench — running a command from the palette is enough. Nothing
     is served there, but the commands are still registered, so they can say
     why rather than leaving the palette pointing at a command that isn't
     there. */
  var diagnostics = vscode.window.createOutputChannel('Workbench', { log: true });
  context.subscriptions.push(diagnostics);
  var root = workbenchRoot();
  if (root) diagnostics.info('extension.activated ' + JSON.stringify({ workspace: root }));
  var ready = root ? serve(context, root, diagnostics) : Promise.resolve(null);

  context.subscriptions.push(
    vscode.commands.registerCommand('canonic.showWorkbenchLog', function () {
      diagnostics.show(true);
    })
  );

  /* What keeps the Workbench icon out of the activity bar of every project that
     has nothing to do with it: the view is contributed with a `when` on this
     key, and a container whose only view is hidden isn't drawn at all. */
  vscode.commands.executeCommand('setContext', 'canonic.hasWorkbench', !!root);

  function command(name, run) {
    context.subscriptions.push(
      vscode.commands.registerCommand(name, function () {
        return ready.then(function (running) {
          if (running) return run(running);
          vscode.window.showWarningMessage(
            root
              ? 'The Workbench server isn’t running.'
              : 'There’s no workbench.yaml in this project — that file is what lists the screens Workbench shows.'
          );
        });
      })
    );
  }

  /* In an editor tab of our own — see panel.js for why not the built-in
     browser. */
  command('canonic.openWorkbench', function (running) {
    return panel.show(context, running.url, '');
  });

  /* And in a real browser, for when the built-in one gets in the way. */
  command('canonic.openWorkbenchExternally', function (running) {
    return vscode.env.openExternal(vscode.Uri.parse(running.url));
  });

  command('canonic.copyWorkbenchUrl', function (running) {
    return vscode.env.clipboard.writeText(running.url).then(function () {
      vscode.window.setStatusBarMessage('Copied ' + running.url, 3000);
    });
  });

  /* The sidebar. Its rows address the workbench the same way a bookmark does,
     through the hash — `#pages/sign-in.html:error` — so opening a screen from
     the editor and picking it inside the workbench end up in the same place.
     No width is named: the workbench keeps the one the toolbar is set to. */
  if (!root) return;

  screens.register(
    context,
    root,
    function (target) {
      if (!target.src) return;
      return ready.then(function (running) {
        if (!running) {
          vscode.window.showWarningMessage('The Workbench server isn’t running.');
          return;
        }
        return panel.show(context, running.url, target.src + (target.state ? ':' + target.state : ''));
      });
    },
    /* And the way back: the canvas reports every screen it lands on, so the
       list marks it even when the move started over there. */
    panel.onHere,
    panel.refresh,
    function () {
      return ready.then(function (running) { return running ? running.config() : null; });
    },
    /* Selecting the Workbench view opens the canvas, keeping the keyboard
       in the list. A server that failed to start has already said so. */
    function () {
      return ready.then(function (running) {
        if (running) return panel.show(context, running.url, '', { preserveFocus: true });
      });
    }
  );
}

function deactivate() {}

module.exports = { activate: activate, deactivate: deactivate };
