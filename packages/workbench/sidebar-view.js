/* The sidebar view
   ----------------
   The space's pages in the editor's own sidebar: the collection list, groups,
   pages, and the states under a page — the same list the workbench draws
   down its left edge, because it is literally the same list.
   The page is this extension's own `workbench/sidebar.html`, and the code that
   builds the rows is `page-list.js` beside it, which the workbench uses too.
   Clicking a row puts that page on the canvas in the editor tab.

   A webview rather than a TreeView. A tree gives
   you the editor's keyboard and collapse memory for free, but it can only
   draw the rows the editor draws: no collection list, no state rows reading
   as versions of the page above them, no filter of our own. The list is the
   part of this tool people look at all day, so it is drawn rather than
   configured — and `sidebar.css` dresses it in VS Code's own theme colours so
   it still belongs in the sidebar it stands in.

   What is added to that page on the way in is in `page()` below: where its own
   folder is, where the space is, and what it may load — a webview's address
   is the editor's, so the document can work out neither by itself. The page
   reads workbench.yaml itself, exactly as it does in a browser, which is why
   it can be opened straight off disk while it is being designed.

   This view follows the canvas: the workbench reports every
   page it routes to, so a link followed inside a live preview moves the
   selection here too.
*/

var vscode = require('vscode');
var path = require('path');
var fs = require('fs');
var crypto = require('node:crypto');
var components = require('./src/server/webview-components.ts');

var VIEW_ID = 'canonic.sidebar';
var PAGE = 'sidebar.html';

/* Everything the page is allowed to reach: the workbench folder, for its
   scripts and styles, and the space's root, which is where it fetches
   workbench.yaml from. Those are two different places now that the workbench
   ships in here — one root each, below. */
function policy(webview, nonce) {
  var source = webview.cspSource;
  return [
    "default-src 'none'",
    'img-src ' + source + ' data:',
    'font-src ' + source,
    'style-src ' + source,
    'script-src ' + source + " 'nonce-" + nonce + "'",
    /* The config is fetched, not built in — the same request a browser makes. */
    'connect-src ' + source,
  ].join('; ');
}

/* Five things are added on the way in, and only five: a <base> saying where
   the workbench folder is, the policy saying what the page may load, the
   space's root — which the page can't work out for itself, because a webview's
   address is the editor's — the spaces to switch between, and which build
   this is. Configuration stays in metadata; the component bootstrap is bundled
   from source in memory and authorized with its own CSP nonce.

   The build number is there to make the string differ. Assigning `webview.html`
   only takes when the value changes; an identical one is dropped, so without a
   stamp the second build of a page whose base and policy are fixed for the life
   of the view would silently do nothing. */
function attribute(value) {
  return String(value).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
}

/* A different space is a different root, so switching builds a new page
   anyway, and the list of spaces never has to change under one that's
   built. */
function page(webview, dir, space, build, spaces) {
  var body = fs.readFileSync(path.join(dir.fsPath, PAGE), 'utf8');
  var nonce = crypto.randomBytes(18).toString('base64');
  /* Replacer functions, because a replacement string reads `$&` and the
     like as patterns, and the bundled code contains them. */
  body = body.replace('href="/_workbench/src/theme/defaults.css"', function () {
    return 'href="' + webview.asWebviewUri(vscode.Uri.file(path.join(dir.fsPath, '../src/theme/defaults.css'))).toString() + '"';
  });
  body = body.replace('<script type="module" src="/_workbench/src/components/bootstrap.ts"></script>', function () {
    return '<script nonce="' + nonce + '">' + components.webviewComponents() + '</script>';
  });
  var root = vscode.Uri.file(space.root);
  /* A space whose workbench.yaml isn't at its root, or is one of several
     in the file, says where the file is and which space it is. */
  var where = '';
  if (space.dir && space.dir !== space.root) {
    where += '  <meta name="canonic-config" content="' + webview.asWebviewUri(vscode.Uri.file(space.dir)).toString() + '/" />\n';
  }
  if (space.key) where += '  <meta name="canonic-space" content="' + attribute(space.key) + '" />\n';
  return body.replace(
    '<head>',
    '<head>\n' +
      '  <base href="' + webview.asWebviewUri(dir).toString() + '/" />\n' +
      '  <meta name="canonic-root" content="' + webview.asWebviewUri(root).toString() + '/" />\n' +
      where +
      '  <meta name="canonic-spaces" content="' + attribute(JSON.stringify(spaces || { spaces: [] })) + '" />\n' +
      '  <meta name="canonic-build" content="' + build + '" />\n' +
      '  <meta http-equiv="Content-Security-Policy" content="' + policy(webview, nonce) + '" />'
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
    collections: (loaded && loaded.catalogCollections) || [],
    problems: (loaded && loaded.problems) || [],
  };
}

function catalogFailure(error) {
  return {
    type: 'canonic-catalog',
    collections: [],
    problems: ['Couldn’t load the page catalogs — ' + String(error.message || error)],
  };
}

/* Builds the view and keeps it in step with both sides.

   `spaces()` answers with { current, spaces } — the id of the space
   showing, and every space the switcher lists (see spaces.js) — and
   `space()` with the one showing as { root, dir, key }: the folder it
   serves, the folder of its workbench.yaml, and its key in that file when
   the file lists several. `open` is given
   { src, state } and puts it on the canvas. `follow` takes a listener called
   with { src, state } whenever the canvas moves, and answers with the way to
   stop listening. `catalog` asks the running server for any Storybook
   collections it imported, which the webview cannot fetch cross-origin. `shown`
   is called whenever the view comes into sight — selecting Workbench in the
   activity bar — so the canvas opens beside the list. `pickSpace(id)`,
   `addSpace()` and `removeSpace(id)` carry the switcher's choices.

   Answers with { retarget() }, for when the space showing has changed or
   the list of spaces has: the watcher moves to the current space's file
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
  var space = options.space();
  var watcher = null;

  /* What the canvas is showing, as far as this side knows — so a view that
     was closed and reopened, or a page that reloaded, comes back marked. */
  var where = { src: null, state: null };

  function post(at) {
    if (view) view.webview.postMessage({ type: 'canonic-here', src: at.src, state: at.state });
  }

  /* Counted so every build is a different string — see page(). */
  var builds = 0;

  /* What the page may load: its own folder, the icons, the space's root
     and the folder of its workbench.yaml — which change with the space, so
     they are set on every build. */
  function resources() {
    var roots = [dir, lucideDir, vscode.Uri.joinPath(context.extensionUri, 'src/theme')];
    if (space) {
      roots.push(vscode.Uri.file(space.root));
      if (space.dir && space.dir !== space.root) roots.push(vscode.Uri.file(space.dir));
    }
    view.webview.options = { enableScripts: true, localResourceRoots: roots };
  }

  function build() {
    if (!view) return;
    builds += 1;
    if (!space) {
      view.webview.html = nothing('There’s no space to show.');
      return;
    }
    try {
      resources();
      view.webview.html = page(view.webview, dir, space, builds, options.spaces());
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

        if (type === 'canonic-space') {
          if (options.pickSpace) options.pickSpace(String(message.id || ''));
          return;
        }

        if (type === 'canonic-add-space') {
          if (options.addSpace) options.addSpace();
          return;
        }

        if (type === 'canonic-remove-space') {
          if (options.removeSpace) options.removeSpace(String(message.id || ''));
          return;
        }

        if (type === 'canonic-catalog-request') {
          sendCatalog();
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
    if (!space) return;
    watcher = vscode.workspace.createFileSystemWatcher(
      new vscode.RelativePattern(vscode.Uri.file(space.dir || space.root), '{workbench.yaml,workbench.local.yaml}')
    );
    ['onDidChange', 'onDidCreate', 'onDidDelete'].forEach(function (event) {
      watcher[event](rebuild);
    });
  }
  watch();

  /* Another space, or another list of them. The canvas is switched by the
     extension; this page only has to follow, so there is nothing to refresh
     over there first. The selection belongs to the old space and is
     dropped — the canvas reports the new one with its first wb-here. */
  function retarget() {
    var next = options.space();
    if (JSON.stringify(next) !== JSON.stringify(space)) {
      var moved = !next || !space || next.dir !== space.dir || next.key !== space.key;
      var refile = !next || !space || next.dir !== space.dir;
      space = next;
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
    vscode.commands.registerCommand('canonic.openPage', function (target) {
      return open(target || {});
    }),
    /* The same rebuild the watcher runs, by hand: for when the page itself
       changed under the editor, which is what happens while the workbench is
       being worked on. */
    vscode.commands.registerCommand('canonic.refreshPages', rebuild)
  );

  /* The catalog, again: the server learned something after it last answered,
     such as a docs page's problems. */
  function sendCatalog() {
    if (!view) return;
    Promise.resolve(catalog ? catalog() : null).then(
      function (loaded) {
        if (view) view.webview.postMessage(catalogMessage(loaded));
      },
      function (error) {
        if (view) view.webview.postMessage(catalogFailure(error));
      }
    );
  }

  return { retarget: retarget, catalogChanged: sendCatalog };
}

module.exports = { register: register };
