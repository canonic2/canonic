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

/* Four things are added on the way in, and only four: a <base> saying where
   the workbench folder is, the policy saying what the page may load, the
   project root — which the page can't work out for itself, because a webview's
   address is the editor's — and which build this is. A <meta> rather than a
   script tag so nothing inline has to be allowed through the policy.

   The build number is there to make the string differ. Assigning `webview.html`
   only takes when the value changes; an identical one is dropped, so without a
   stamp the second build of a page whose base and policy are fixed for the life
   of the view would silently do nothing. */
function page(webview, dir, root, build) {
  var body = fs.readFileSync(path.join(dir.fsPath, PAGE), 'utf8');
  return body.replace(
    '<head>',
    '<head>\n' +
      '  <base href="' + webview.asWebviewUri(dir).toString() + '/" />\n' +
      '  <meta name="canonic-root" content="' + webview.asWebviewUri(root).toString() + '/" />\n' +
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

   `open` is given { src, state } and puts it on the canvas. `follow` takes a
   listener called with { src, state } whenever the canvas moves, and answers
   with the way to stop listening. `catalog` asks the running server for any
   Storybook sections it imported, which the webview cannot fetch cross-origin.
   `shown` is called whenever the view comes into sight — selecting Workbench
   in the activity bar — so the canvas opens beside the list. */
function register(context, root, open, follow, refresh, catalog, shown) {
  var project = vscode.Uri.file(root);
  var dir = vscode.Uri.joinPath(context.extensionUri, 'workbench');
  var lucide = vscode.Uri.file(require.resolve('lucide/dist/umd/lucide.min.js'));
  var lucideDir = vscode.Uri.file(path.dirname(lucide.fsPath));
  var view = null;

  /* What the canvas is showing, as far as this side knows — so a view that
     was closed and reopened, or a page that reloaded, comes back marked. */
  var where = { src: null, state: null };

  function post(at) {
    if (view) view.webview.postMessage({ type: 'canonic-here', src: at.src, state: at.state });
  }

  /* Counted so every build is a different string — see page(). */
  var builds = 0;

  function build() {
    if (!view) return;
    builds += 1;
    try {
      view.webview.html = page(view.webview, dir, project, builds);
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

      view.webview.options = {
        enableScripts: true,
        localResourceRoots: [dir, lucideDir, project],
      };

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
  var watcher = vscode.workspace.createFileSystemWatcher(
    new vscode.RelativePattern(root, '{workbench.yaml,workbench.local.yaml}')
  );
  ['onDidChange', 'onDidCreate', 'onDidDelete'].forEach(function (event) {
    watcher[event](rebuild);
  });
  context.subscriptions.push(watcher);

  context.subscriptions.push(
    vscode.commands.registerCommand('canonic.openScreen', function (target) {
      return open(target || {});
    }),
    /* The same rebuild the watcher runs, by hand: for when the page itself
       changed under the editor, which is what happens while the workbench is
       being worked on. */
    vscode.commands.registerCommand('canonic.refreshScreens', rebuild)
  );
}

module.exports = { register: register };
