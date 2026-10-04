/* Workbench — the editor half
   -------------------------
   Starts the workbench server when a project that has a workbench opens, and
   says so in the status bar when a screenshot lands.
   Everything that isn't editor-shaped lives in server.js.

   This is installed once, globally, and knows nothing about any particular
   project. The projects it shows are every folder of the window that carries
   a workbench, then the ones the user added from elsewhere on disk, which are
   remembered across windows. One of them is showing at a time; each has a
   server of its own, started the first time it is shown (see projects.js).
*/

var vscode = require('vscode');
var server = require('./server');
var startup = require('./startup');
var screens = require('./screens');
var panel = require('./panel');
var projects = require('./projects');
var path = require('path');
var fs = require('fs');

/* The projects added from outside the window, by folder, in every window. */
var SAVED = 'canonic.projects';
/* The project this window last showed, by folder. */
var CURRENT = 'canonic.project';

var NO_PROJECT = 'There’s no workbench.yaml in this project — that file is what lists the screens Workbench shows. ' +
  'Add one, or add a project that has one with Workbench: Add Project….';

/* The workbench ships in here, so what says a folder wants one is the folder
   naming its screens: workbench.yaml at its root. The manifest activates this
   extension on the same file. Every folder of a multi-root window that has
   one is a project, in the window's order. */
function workspaceRoots() {
  return (vscode.workspace.workspaceFolders || []).map(function (folder) {
    return folder.uri.fsPath;
  }).filter(projects.hasWorkbench);
}

/* One project's server. Answers with it running, or rejects when it can't
   start. Start commands run first, as they always have, in this project's
   own terminals. */
function serve(context, project, diagnostics, shared) {
  var root = project.root;
  return Promise.resolve().then(function () {
    return startup.run({ dir: project.dir, key: project.key }, {
      isTrusted: vscode.workspace.isTrusted,
      createTerminal: function (options) { return vscode.window.createTerminal(options); },
      report: function (message) { diagnostics.info('implementation.start ' + message); },
    });
  }).catch(function (error) {
    diagnostics.warn('implementation.start ' + String(error.message || error));
  }).then(function () {
    return server.start({
      root: root,
      config: { dir: project.dir, key: project.key },
      isTrusted: vscode.workspace.isTrusted,
      eagerCapture: true,
      capture: shared.capture(),
      captureShared: true,
      captureStorage: path.join(context.globalStorageUri.fsPath, 'capture'),
      projects: shared.hub,
      screenCapturePermissionOwner: vscode.env.appName || 'Visual Studio Code',
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
  });
}

function activate(context) {
  /* Installed once for every project, this wakes up in plenty of windows
     that have no workbench. Nothing is served there unless the user has
     added projects, and even then nothing starts until the Workbench view or
     canvas is opened; the commands are still registered, so they can say why
     rather than leaving the palette pointing at a command that isn't there. */
  var diagnostics = vscode.window.createOutputChannel('Workbench', { log: true });
  context.subscriptions.push(diagnostics);

  /* One screenshot service for every project's server: it is an Electron
     process, and a second project is no reason for a second one. */
  var capture = null;
  var shared = {
    capture: function () {
      if (!capture) capture = server.createCapture(path.join(context.globalStorageUri.fsPath, 'capture'));
      return capture;
    },
    hub: null,
  };
  var hub = projects.create({
    start: function (project) {
      diagnostics.info('project.starting ' + JSON.stringify({ workspace: project.root, project: project.key }));
      return serve(context, project, diagnostics, shared);
    },
  });
  shared.hub = hub;

  context.subscriptions.push({
    dispose: function () {
      hub.close().then(function () {
        if (capture) return capture.close();
      });
    },
  });

  function saved() {
    var list = context.globalState.get(SAVED);
    return Array.isArray(list) ? list.filter(function (root) { return typeof root === 'string'; }) : [];
  }

  /* The window's folders first, then the added ones; a folder that is both
     counts as the window's, and an added folder whose workbench.yaml has gone
     is left off the list without being forgotten. Each folder's file gives
     one project, or one per entry of its `projects`. */
  function entries() {
    var own = workspaceRoots().map(function (dir) { return { dir: dir, removable: false }; });
    var added = saved().filter(projects.hasWorkbench).map(function (dir) { return { dir: dir, removable: true }; });
    return own.concat(added);
  }

  var listed = [];
  var currentId = null;

  function find(id) {
    return listed.filter(function (project) { return project.id === id; })[0] || null;
  }

  function currentProject() {
    return find(currentId);
  }

  /* Rereads the list, keeping the current project when it is still on it.
     Answers whether the current project changed. */
  function sync() {
    hub.set(entries());
    listed = hub.list();
    var before = currentId;
    if (!find(currentId)) {
      /* Remembered by id; a folder path is a project remembered before a
         file could list several. */
      var remembered = context.workspaceState.get(CURRENT);
      var again = typeof remembered === 'string'
        ? find(/^[0-9a-f]{10}$/.test(remembered) ? remembered : projects.projectId(remembered))
        : null;
      currentId = again ? again.id : (listed[0] ? listed[0].id : null);
    }
    /* What keeps the Workbench icon out of the activity bar of every window
       that has nothing to do with it: the view is contributed with a `when`
       on this key, and a container whose only view is hidden isn't drawn. */
    vscode.commands.executeCommand('setContext', 'canonic.hasWorkbench', listed.length > 0);
    return before !== currentId;
  }

  /* The current project's server, started if it isn't running. A failure
     says so once per attempt; asking again tries again. */
  function ready() {
    var project = currentProject();
    if (!project) return Promise.resolve(null);
    return hub.open(project.id).catch(function (err) {
      vscode.window.showErrorMessage('Workbench couldn’t start ' + project.name + ': ' + String(err.message || err));
      return null;
    });
  }

  function url() {
    return ready().then(function (running) { return running && running.url; });
  }

  sync();
  if (currentProject()) {
    diagnostics.info('extension.activated ' + JSON.stringify({ workspace: currentProject().root }));
    /* A window's own project starts with the window, as it always has. An
       added one waits to be opened: it is not this window's to start. */
    if (!currentProject().removable) ready();
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('canonic.showWorkbenchLog', function () {
      diagnostics.show(true);
    })
  );

  function command(name, run) {
    context.subscriptions.push(
      vscode.commands.registerCommand(name, function () {
        var project = currentProject();
        return ready().then(function (running) {
          if (running) return run(running);
          if (!project) vscode.window.showWarningMessage(NO_PROJECT);
        });
      })
    );
  }

  /* In an editor tab of our own — see panel.js for why not the built-in
     browser. */
  function openCanvas(options, hash) {
    if (!currentProject()) {
      vscode.window.showWarningMessage(NO_PROJECT);
      return;
    }
    return panel.show(context, url(), hash || '', options);
  }
  context.subscriptions.push(vscode.commands.registerCommand('canonic.openWorkbench', function () {
    return openCanvas();
  }));

  /* And in a real browser, for when the built-in one gets in the way. */
  command('canonic.openWorkbenchExternally', function (running) {
    return vscode.env.openExternal(vscode.Uri.parse(running.url));
  });

  command('canonic.copyWorkbenchUrl', function (running) {
    return vscode.env.clipboard.writeText(running.url).then(function () {
      vscode.window.setStatusBarMessage('Copied ' + running.url, 3000);
    });
  });

  /* ------------------------------------------------------------ projects */

  var sidebar = null;

  /* Shows another project: the tab loads its server and the sidebar its
     screens, together, wherever the switch was made. */
  function select(id) {
    var project = find(id);
    if (!project || id === currentId) return;
    currentId = id;
    context.workspaceState.update(CURRENT, id);
    diagnostics.info('project.selected ' + JSON.stringify({ workspace: project.root, project: project.key }));
    /* Projects that share a folder share its server.json; the one showing
       claims it, so agents there read what the user sees. */
    var opened = url();
    ready().then(function (running) {
      if (running && running.announce && currentId === id) running.announce();
    });
    panel.retarget(opened);
    if (sidebar) sidebar.retarget();
  }

  function ids() {
    return listed.map(function (project) { return project.id; }).join(' ');
  }

  /* After the list may have changed under the current project — removed
     here, added in another window, or with the window's folders. */
  function relist() {
    var before = ids();
    var moved = sync();
    if (moved) panel.retarget(url());
    if (sidebar && (moved || before !== ids())) sidebar.retarget();
  }

  function addProject() {
    return vscode.window.showOpenDialog({
      canSelectFolders: true,
      canSelectFiles: false,
      canSelectMany: false,
      openLabel: 'Add Project',
      title: 'Add a project to Workbench',
    }).then(function (picked) {
      if (!picked || !picked.length) return;
      var root = picked[0].fsPath;
      if (!projects.hasWorkbench(root)) {
        vscode.window.showWarningMessage(
          'There’s no workbench.yaml in ' + path.basename(root) + '. Add one that lists its screens, then add the project again.'
        );
        return;
      }
      var dir = path.resolve(root);
      var list = saved();
      var known = listed.some(function (project) { return project.dir === dir; });
      if (!known && list.indexOf(dir) === -1) list.push(dir);
      return Promise.resolve(context.globalState.update(SAVED, list)).then(function () {
        relist();
        /* A file that lists several projects shows its first. */
        var first = listed.filter(function (project) { return project.dir === dir; })[0];
        if (first) select(first.id);
        if (!panel.isOpen()) openCanvas({ preserveFocus: true });
      });
    });
  }

  /* Only an added folder leaves the list; a window's own folders are the
     window's. What was added is a folder, so removing one of its projects
     removes the folder and every project its file lists. Their servers stop
     with them. */
  function removeProject(id) {
    var project = find(id);
    if (!project || !project.removable) return;
    var list = saved().filter(function (dir) { return path.resolve(dir) !== project.dir; });
    return Promise.resolve(context.globalState.update(SAVED, list)).then(relist);
  }

  function switchProject() {
    if (!listed.length) {
      vscode.window.showWarningMessage(NO_PROJECT);
      return;
    }
    var items = listed.map(function (project) {
      return {
        label: (project.id === currentId ? '$(check) ' : '') + project.name,
        description: project.root,
        id: project.id,
      };
    });
    items.push({ label: '$(add) Add a project…', add: true });
    return vscode.window.showQuickPick(items, { title: 'Workbench projects', placeHolder: 'Switch to a project' }).then(function (picked) {
      if (!picked) return;
      if (picked.add) return addProject();
      select(picked.id);
      return openCanvas();
    });
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('canonic.addProject', addProject),
    vscode.commands.registerCommand('canonic.switchProject', switchProject),
    vscode.workspace.onDidChangeWorkspaceFolders(relist),
    /* Added in another window: the list is shared, but nothing announces a
       change, so it is reread whenever this window comes back to the front. */
    vscode.window.onDidChangeWindowState(function (state) {
      if (state.focused) relist();
    })
  );
  panel.onProject(select);

  /* The sidebar. Its rows address the workbench the same way a bookmark does,
     through the hash — `#pages/sign-in.html:error` — so opening a screen from
     the editor and picking it inside the workbench end up in the same place.
     No width is named: the workbench keeps the one the toolbar is set to. */
  sidebar = screens.register(context, {
    project: function () {
      var project = currentProject();
      return project ? { root: project.root, dir: project.dir, key: project.key } : null;
    },
    /* Read again on every build: a saved workbench.yaml rebuilds the
       sidebar, and a project's name, color, icon or root may be what
       changed, or the projects themselves. If the one showing is gone, the
       tab and the list move to the first, after this build. */
    projects: function () {
      if (sync()) {
        panel.retarget(url());
        setTimeout(function () { if (sidebar) sidebar.retarget(); }, 0);
      }
      return { current: currentId, projects: listed };
    },
    open: function (target) {
      if (!target.src) return;
      return openCanvas(null, target.src + (target.state ? ':' + target.state : ''));
    },
    /* And the way back: the canvas reports every screen it lands on, so the
       list marks it even when the move started over there. */
    follow: panel.onHere,
    refresh: panel.refresh,
    catalog: function () {
      return ready().then(function (running) { return running ? running.config() : null; });
    },
    /* Selecting the Workbench view opens the canvas, keeping the keyboard
       in the list. A server that failed to start has already said so. */
    shown: function () {
      if (currentProject()) return openCanvas({ preserveFocus: true });
    },
    pickProject: select,
    addProject: addProject,
    removeProject: removeProject,
  });
}

function deactivate() {}

module.exports = { activate: activate, deactivate: deactivate };
