/* The Screens view
   ----------------
   The project's screens in the editor's own sidebar: sections on a rail,
   folders, screens, and the states under a screen — the same list the
   workbench draws down its left edge, because it is literally the same list.
   The page is this extension's own `workbench/sidebar.html`, and the code that
   builds the rows is `nav.js` beside it, which the workbench uses too. Clicking
   a row puts that screen on the canvas in the editor tab.

   A webview rather than a TreeView, which is what this replaced. A tree gives
   you the editor's keyboard and collapse memory for free, but it can only
   draw the rows the editor draws: no rail of sections, no state rows reading
   as versions of the screen above them, no filter of our own. The list is the
   part of this tool people look at all day, so it is drawn rather than
   configured — and `sidebar.css` dresses it in VS Code's own theme colours so
   it still belongs in the sidebar it stands in.

   What is added to that page on the way in is in `page()` below: where its own
   folder is, where the project is, and what it may load — a webview's address
   is the editor's, so the document can work out neither by itself. The page
   reads workbench.yaml itself, exactly as it does in a browser, which is why
   it can be opened straight off disk while it is being designed.

   Unlike the tree, this one follows the canvas: the workbench reports every
   screen it routes to, so a link followed inside a live preview moves the
   selection here too.
*/

var vscode = require('vscode');
var path = require('path');
var fs = require('fs');

var VIEW_ID = 'canonic.screens';
var PAGE = 'sidebar.html';

/* Everything the page is allowed to reach: the workbench folder, for its
   scripts and styles, and the project root, which is where it fetches
   workbench.yaml from. Those are two different places now that the workbench
   ships in here — one root each, below. */
function policy(webview) {
  var source = webview.cspSource;
  return [
    "default-src 'none'",
    'img-src ' + source + ' data:',
    'font-src ' + source,
    'style-src ' + source,
    'script-src ' + source,
    /* The config is fetched, not built in — the same request a browser makes. */
    'connect-src ' + source,
  ].join('; ');
}

/* Five things are added on the way in, and only five: a <base> saying where
   the workbench folder is, the policy saying what the page may load, the
   project root — which the page can't work out for itself, because a webview's
   address is the editor's — the projects to switch between, and which build
   this is. <meta> rather than script tags so nothing inline has to be allowed
   through the policy.

   The build number is there to make the string differ. Assigning `webview.html`
   only takes when the value changes; an identical one is dropped, so without a
   stamp the second build of a page whose base and policy are fixed for the life
   of the view would silently do nothing. */
function attribute(value) {
  return String(value).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
}

/* A different project is a different root, so switching builds a new page
   anyway, and the list of projects never has to change under one that's
   built. */
function page(webview, dir, project, build, projects) {
  var body = fs.readFileSync(path.join(dir.fsPath, PAGE), 'utf8');
  var root = vscode.Uri.file(project.root);
  /* A project whose workbench.yaml isn't at its root, or is one of several
     in the file, says where the file is and which project it is. */
  var where = '';
  if (project.dir && project.dir !== project.root) {
    where += '  <meta name="canonic-config" content="' + webview.asWebviewUri(vscode.Uri.file(project.dir)).toString() + '/" />\n';
  }
  if (project.key) where += '  <meta name="canonic-project" content="' + attribute(project.key) + '" />\n';
  return body.replace(
    '<head>',
    '<head>\n' +
      '  <base href="' + webview.asWebviewUri(dir).toString() + '/" />\n' +
      '  <meta name="canonic-root" content="' + webview.asWebviewUri(root).toString() + '/" />\n' +
      where +
      '  <meta name="canonic-projects" content="' + attribute(JSON.stringify(projects || { projects: [] })) + '" />\n' +
      '  <meta name="canonic-build" content="' + build + '" />\n' +
      '  <meta http-equiv="Content-Security-Policy" content="' + policy(webview) + '" />'
  );
}

/* The sidebar page couldn't be read — a broken install, nothing a project can
   cause. The view says so rather than sitting there empty. */
function nothing(message) {
  return [
    '<!doctype html><html lang="en"><head><meta charset="utf-8" /></head>',
    '<body style="margin:0;padding:12px;font-family:var(--vscode-font-family);',
    'font-size:var(--vscode-font-size);color:var(--vscode-descriptionForeground)">',
    '<p>' + message + '</p>',
    '</body></html>',
  ].join('');
}

function catalogMessage(loaded) {
  return {
    type: 'canonic-catalog',
    sections: (loaded && loaded.catalogSections) || [],
    problems: (loaded && loaded.problems) || [],
  };
}

function catalogFailure(error) {
  return {
    type: 'canonic-catalog',
    sections: [],
    problems: ['Couldn’t load the screen catalogs — ' + String(error.message || error)],
  };
}

/* Builds the view and keeps it in step with both sides.

   `projects()` answers with { current, projects } — the id of the project
   showing, and every project the switcher lists (see projects.js) — and
   `project()` with the one showing as { root, dir, key }: the folder it
   serves, the folder of its workbench.yaml, and its key in that file when
   the file lists several. `open` is given
   { src, state } and puts it on the canvas. `follow` takes a listener called
   with { src, state } whenever the canvas moves, and answers with the way to
   stop listening. `catalog` asks the running server for any Storybook
   sections it imported, which the webview cannot fetch cross-origin. `shown`
   is called whenever the view comes into sight — selecting Workbench in the
   activity bar — so the canvas opens beside the list. `pickProject(id)`,
   `addProject()` and `removeProject(id)` carry the switcher's choices.

   Answers with { retarget() }, for when the project showing has changed or
   the list of projects has: the watcher moves to the current project's file
   and the page is rebuilt. */
function register(context, options) {
  var open = options.open;
  var follow = options.follow;
  var refresh = options.refresh;
  var catalog = options.catalog;
  var shown = options.shown;
  var dir = vscode.Uri.joinPath(context.extensionUri, 'workbench');
  var lucide = vscode.Uri.file(require.resolve('lucide/dist/umd/lucide.min.js'));
  var lucideDir = vscode.Uri.file(path.dirname(lucide.fsPath));
  var view = null;
  var project = options.project();
  var watcher = null;

  /* What the canvas is showing, as far as this side knows — so a view that
     was closed and reopened, or a page that reloaded, comes back marked. */
  var where = { src: null, state: null };

  function post(at) {
    if (view) view.webview.postMessage({ type: 'canonic-here', src: at.src, state: at.state });
  }

  /* Counted so every build is a different string — see page(). */
  var builds = 0;

  /* What the page may load: its own folder, the icons, the project's root
     and the folder of its workbench.yaml — which change with the project, so
     they are set on every build. */
  function resources() {
    var roots = [dir, lucideDir];
    if (project) {
      roots.push(vscode.Uri.file(project.root));
      if (project.dir && project.dir !== project.root) roots.push(vscode.Uri.file(project.dir));
    }
    view.webview.options = { enableScripts: true, localResourceRoots: roots };
  }

  function build() {
    if (!view) return;
    builds += 1;
    if (!project) {
      view.webview.html = nothing('There’s no project to show.');
      return;
    }
    try {
      resources();
      view.webview.html = page(view.webview, dir, project, builds, options.projects());
    } catch (error) {
      view.webview.html = nothing(
        'Workbench couldn’t read its own ' + PAGE + ': ' + String(error.message || error)
      );
    }
  }

  /* Refresh the canvas first, then expose the new rows. A click on a freshly
     rebuilt sidebar must never arrive while the canvas still has the old
     manifest in memory. The panel bridge queues picks until refresh finishes. */
  function rebuild() {
    if (refresh) refresh();
    build();
  }

  var provider = {
    resolveWebviewView: function (resolved) {
      view = resolved;

      /* Listening before building: setting the html starts the page, and its
         first message is the one asking what the canvas is showing. */
      view.webview.onDidReceiveMessage(function (message) {
        var type = message && message.type;

        /* A row was picked. It arrives as the hash the workbench's address bar
           would have carried — `pages/sign-in.html:error` — which is what the
           canvas takes, so a pick here and a copied link are one instruction. */
        if (type === 'canonic-pick') {
          var hash = String(message.hash || '');
          var colon = hash.indexOf(':');
          open({
            src: colon === -1 ? hash : hash.slice(0, colon),
            state: colon === -1 ? null : hash.slice(colon + 1),
          });
          return;
        }

        if (type === 'canonic-project') {
          if (options.pickProject) options.pickProject(String(message.id || ''));
          return;
        }

        if (type === 'canonic-add-project') {
          if (options.addProject) options.addProject();
          return;
        }

        if (type === 'canonic-remove-project') {
          if (options.removeProject) options.removeProject(String(message.id || ''));
          return;
        }

        if (type === 'canonic-catalog-request') {
          Promise.resolve(catalog ? catalog() : null).then(
            function (loaded) {
              if (view) view.webview.postMessage(catalogMessage(loaded));
            },
            function (error) {
              if (view) view.webview.postMessage(catalogFailure(error));
            }
          );
          return;
        }

        /* A freshly built list, asking what the canvas is already on. The
           view's header just says Workbench: the container and its one view
           share that name, so the editor shows it once, with nothing after. */
        if (type === 'canonic-ready') {
          post(where);
        }
      });

      var stop = follow(function (moved) {
        where = moved;
        post(moved);
      });

      /* The list is half the tool; the canvas is the other half. Selecting
         the view opens the canvas too, or brings it forward if it is open.
         Closing the canvas while the list stays in sight leaves it closed. */
      var seen = view.onDidChangeVisibility(function () {
        if (view && view.visible && shown) shown();
      });

      view.onDidDispose(function () {
        stop();
        seen.dispose();
        view = null;
      });

      build();
      if (view.visible && shown) shown();
    },
  };

  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(VIEW_ID, provider, {
      /* The list keeps its folds and its filter while the view is hidden;
         rebuilding it on every peek would lose both. */
      webviewOptions: { retainContextWhenHidden: true },
    })
  );

  /* The config is edited by hand and by agents; either way the list is stale
     the moment it changes, and one you have to refresh yourself is one you
     stop trusting.

     Rebuilt from this side rather than reloaded from inside the page. The
     document is inert until this side injects its <base>, its root and its
     policy, so a page that calls location.reload() on itself comes back
     without any of them — every stylesheet and script in it is a relative path
     with nothing to resolve against — and the view goes permanently blank. The
     selection is not lost by the rebuild: the fresh page asks for it with
     canonic-ready and post() answers. */
  function watch() {
    if (watcher) watcher.dispose();
    watcher = null;
    if (!project) return;
    watcher = vscode.workspace.createFileSystemWatcher(
      new vscode.RelativePattern(vscode.Uri.file(project.dir || project.root), '{workbench.yaml,workbench.local.yaml}')
    );
    ['onDidChange', 'onDidCreate', 'onDidDelete'].forEach(function (event) {
      watcher[event](rebuild);
    });
  }
  watch();

  /* Another project, or another list of them. The canvas is switched by the
     extension; this page only has to follow, so there is nothing to refresh
     over there first. The selection belongs to the old project and is
     dropped — the canvas reports the new one with its first wb-here. */
  function retarget() {
    var next = options.project();
    if (JSON.stringify(next) !== JSON.stringify(project)) {
      var moved = !next || !project || next.dir !== project.dir || next.key !== project.key;
      var refile = !next || !project || next.dir !== project.dir;
      project = next;
      if (moved) where = { src: null, state: null };
      if (refile) watch();
    }
    build();
  }

  context.subscriptions.push({
    dispose: function () {
      if (watcher) watcher.dispose();
    },
  });

  context.subscriptions.push(
    vscode.commands.registerCommand('canonic.openScreen', function (target) {
      return open(target || {});
    }),
    /* The same rebuild the watcher runs, by hand: for when the page itself
       changed under the editor, which is what happens while the workbench is
       being worked on. */
    vscode.commands.registerCommand('canonic.refreshScreens', rebuild)
  );

  return { retarget: retarget };
}

module.exports = { register: register };
