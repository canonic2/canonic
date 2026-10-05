/* The editor's half of the workbench
   ----------------------------------
   The page list, running in the Workbench view in VS Code's sidebar. It reads
   the same workbench.yaml the workbench does, builds the same list
   page-list.js builds, and hands every pick to the extension.

   It never touches a preview itself. A pick leaves here as the hash the
   address bar would have carried — `pages/sign-in.html:error` — and the
   extension passes it to whichever workbench tab is open, which routes it the
   way a copied link would. That is the whole contract, and it is why picking
   a page here and pasting a URL there end up in the same place.

     out  canonic-pick     the page picked, as a hash
     out  canonic-ready    the list is built, and what space it read
     out  canonic-space    another space picked, by id
     out  canonic-add-space, canonic-remove-space   the list of spaces
     in   canonic-here     the page the workbench is showing now

   canonic-here is the return leg: follow a link inside a live preview and the
   workbench says where it ended up, so this list marks the row without the
   two halves ever disagreeing.

   A saved workbench.yaml is a new list, but this page does not reload itself
   to get it — the extension rebuilds it. Nothing here works without the
   <base>, the root and the policy the host injects, so a document that reloads
   on its own address comes back stripped of all three and cannot load a single
   one of its own scripts.

   Opened outside the editor — off file://, while designing it — there is no
   host to talk to. The list still builds and still filters; picking a row
   simply has nowhere to go.
*/

(function () {
  var host = typeof acquireVsCodeApi === 'function' ? acquireVsCodeApi() : null;

  var shell = document.querySelector('.sb');
  var problem = document.getElementById('problem');
  var diagnostics = document.getElementById('diagnostics');
  var search = document.getElementById('search');
  var loading = document.getElementById('loading');
  var loadingText = document.getElementById('loadingText');
  search.disabled = true;

  var pageList = null;
  var index = {};

  /* Anything marked data-icon gets its Lucide glyph injected once — the same
     line the workbench's shell runs, for the same reason. */
  Array.prototype.forEach.call(document.querySelectorAll('[data-icon]'), function (el) {
    el.appendChild(window.wbIcon(el.dataset.icon));
  });

  function send(message) {
    if (host) host.postMessage(message);
  }

  /* The spaces come in with the document, from the extension: switching one
     rebuilds this document against the other space's root, so the list never
     has to change under a document that is already built. Shown whenever there
     is a host, even for one space, because it is also where Add a
     space… lives. A space that fails to load still has the switcher. */
  function spaces() {
    var meta = document.querySelector('meta[name="canonic-spaces"]');
    if (!meta || !host) return;
    var listed;
    try {
      listed = JSON.parse(meta.content);
    } catch (error) {
      return;
    }
    var switcher = document.getElementById('spaces');
    switcher.iconRenderer = window.wbIcon;
    switcher.allowAdd = true;
    switcher.allowRemove = true;
    switcher.spaces = listed.spaces;
    switcher.currentId = listed.current;
    switcher.addEventListener('wb-space-pick', function (e) { send({ type: 'canonic-space', id: e.detail.id }); });
    switcher.addEventListener('wb-space-add', function () { send({ type: 'canonic-add-space' }); });
    switcher.addEventListener('wb-space-remove', function (e) { send({ type: 'canonic-remove-space', id: e.detail.id }); });
    switcher.addEventListener('wb-space-toggle', function (e) { shell.classList.toggle('is-dimmed', e.detail.open); });
    switcher.hidden = false;
  }

  spaces();

  function showProblems(problems) {
    problems = (problems || []).filter(Boolean);
    diagnostics.hidden = !problems.length;
    diagnostics.textContent = problems.join('\n');
  }

  function build(config) {
    loading.hidden = true;
    search.disabled = false;
    index = window.wbPageList.index(config.collections);

    pageList = window.wbPageList.create({
      search: search,
      collectionList: document.getElementById('collections'),
      collectionsBlock: document.getElementById('collectionsBlock'),
      title: document.getElementById('collectionName'),
      list: document.getElementById('pageList'),
      collections: config.collections,
      treeKeyboard: true,

      /* No width in the hash: how the canvas is framed is the canvas's
         business, and a pick from here shouldn't resize it. */
      onPick: function (src, state) {
        send({ type: 'canonic-pick', hash: src + (state ? ':' + state : '') });
      },
    });

    /* The workbench may already be open on something — say so, and it
       answers with a canonic-here. The space's name goes with it: the view
       header carries it the way a tree carries the folder it is showing. */
    send({ type: 'canonic-ready', name: config.name });
  }

  function start(config) {
    var imports = config.previews !== false || Object.keys(config.implementations || {}).some(function (key) {
      return config.implementations[key].catalog;
    });
    if (!imports || !host) {
      build(config);
      return;
    }
    var catalogs = Object.keys(config.implementations || {}).filter(function (key) {
      return config.implementations[key].catalog;
    }).map(function (key) { return config.implementations[key].label || key; });
    loadingText.textContent = catalogs.length ? 'Waiting for ' + catalogs.join(', ') + '…' : 'Finding previews…';
    /* The first answer builds the list. Later ones carry what the server
       learned afterwards, such as docs pages' problems; they update the
       problems and leave the list, its folds and its filter as they are. */
    var built = false;
    window.addEventListener('message', function (e) {
      var data = e.data || {};
      if (data.type !== 'canonic-catalog') return;
      showProblems(data.problems);
      if (built) return;
      built = true;
      config.collections = window.wbManifest.mergeCollections(config.collections, data.collections || []);
      build(config);
    });
    send({ type: 'canonic-catalog-request' });
  }

  function refuse(error) {
    shell.hidden = true;
    problem.hidden = false;
    problem.textContent = String(error.message || error);
    console.error('[canonic] ' + String(error.message || error));
  }

  window.addEventListener('message', function (e) {
    var data = e.data || {};

    if (data.type !== 'canonic-here' || !pageList) return;
    var item = index[data.src];
    pageList.reveal(data.src);
    pageList.setCurrent(item ? data.src : null, data.state, item && item.collection);
  });

  window.wbConfig.load(start, refuse);
})();
