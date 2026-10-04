/* The editor's half of the workbench
   ----------------------------------
   The screen list, running in the Workbench view in VS Code's sidebar. It reads
   the same workbench.yaml the workbench does, builds the same list nav.js
   builds, and hands every pick to the extension.

   It never touches a preview itself. A pick leaves here as the hash the
   address bar would have carried — `pages/sign-in.html:error` — and the
   extension passes it to whichever workbench tab is open, which routes it the
   way a copied link would. That is the whole contract, and it is why picking
   a screen here and pasting a URL there end up in the same place.

     out  canonic-pick     the screen picked, as a hash
     out  canonic-ready    the list is built, and what project it read
     out  canonic-project  another project picked, by id
     out  canonic-add-project, canonic-remove-project   the list of projects
     in   canonic-here     the screen the workbench is showing now

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

  var nav = null;
  var index = {};

  /* Anything marked data-icon gets its Lucide glyph injected once — the same
     line the workbench's shell runs, for the same reason. */
  Array.prototype.forEach.call(document.querySelectorAll('[data-icon]'), function (el) {
    el.appendChild(window.wbIcon(el.dataset.icon));
  });

  function send(message) {
    if (host) host.postMessage(message);
  }

  /* The projects come in with the page, from the extension: switching one
     rebuilds this page against the other project's root, so the list never
     has to change under a page that is already built. Shown whenever there
     is a host, even for one project, because it is also where Add a
     project… lives. A project that fails to load still has the switcher. */
  function projects() {
    var meta = document.querySelector('meta[name="canonic-projects"]');
    if (!meta || !host) return;
    var listed;
    try {
      listed = JSON.parse(meta.content);
    } catch (error) {
      return;
    }
    var switcher = window.wbProjects.create({
      button: document.getElementById('projectButton'),
      menu: document.getElementById('projectMenu'),
      onPick: function (id) { send({ type: 'canonic-project', id: id }); },
      onAdd: function () { send({ type: 'canonic-add-project' }); },
      onRemove: function (id) { send({ type: 'canonic-remove-project', id: id }); },
      onToggle: function (open) { shell.classList.toggle('is-dimmed', open); },
    });
    switcher.set(listed.projects, listed.current);
    document.getElementById('projects').hidden = false;
  }

  projects();

  function showProblems(problems) {
    problems = (problems || []).filter(Boolean);
    diagnostics.hidden = !problems.length;
    diagnostics.textContent = problems.join('\n');
  }

  function build(config) {
    loading.hidden = true;
    search.disabled = false;
    index = window.wbNav.index(config.sections);

    nav = window.wbNav.create({
      search: search,
      sections: document.getElementById('sections'),
      sectionsBlock: document.getElementById('sectionsBlock'),
      title: document.getElementById('sectionName'),
      list: document.getElementById('nav'),
      groups: config.sections,
      treeKeyboard: true,

      /* No width in the hash: how the canvas is framed is the canvas's
         business, and a pick from here shouldn't resize it. */
      onPick: function (src, state) {
        send({ type: 'canonic-pick', hash: src + (state ? ':' + state : '') });
      },
    });

    /* The workbench may already be open on something — say so, and it
       answers with a canonic-here. The project's name goes with it: the view
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
    window.addEventListener('message', function catalog(e) {
      var data = e.data || {};
      if (data.type !== 'canonic-catalog') return;
      window.removeEventListener('message', catalog);
      showProblems(data.problems);
      config.sections = window.wbManifest.mergeSections(config.sections, data.sections || []);
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

    if (data.type !== 'canonic-here' || !nav) return;
    var item = index[data.src];
    nav.reveal(data.src);
    nav.setCurrent(item ? data.src : null, data.state, item && item.group);
  });

  window.wbConfig.load(start, refuse);
})();
