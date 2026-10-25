/* Workbench shell
   ---------------
   Builds the sidebar from the space's workbench.yaml — read by
   config.js — and loads the picked file into the canvas iframe.

   Nothing in here is about any one space. What it knows about the space
   arrives as the config: a name, and collections of pages. Two constraints
   shape the rest:
   - the config is the index, with the pages the server adds to it:
     previews, docs and Simulator catalogs. No other page is in the sidebar;
   - the iframe can be a foreign origin (a workbench opened off file://), so
     nothing here reads into it. Reload re-assigns src instead of calling into
     contentWindow.

   Paths in the config are relative to the project root, which is where the
   author thinks in — `pages/sign-in.html` — while this document is served out
   of the extension, beside no project at all. config.js settles the root;
   every src is resolved against it on the way to the frame, and stays
   root-relative everywhere else, including the hash.

   The selection lives in the URL hash, so a reload or a copied link lands on
   the same page, in the same scenario, at the same width, through the same
   lens: #pages/sign-in.html:error@mobile~staging — address.js reads and writes
   it. The default scenario and the design lens are left out, so a page without
   scenarios — or on the version you land on — reads as it always did. A
   `+` part names an artboard's iteration; the shell keeps only those the
   selected scenario declares, and only the design lens renders them.

   Scenarios themselves are the page's business, not the shell's: the id
   travels in the frame's URL and scenarios.js applies it inside.

   A lens is an implementation of the page in the design's place: a story
   in a Storybook, the page on a dev server, the page on staging. The config
   declares them (manifest.js), the top bar switches them, and the choice
   sticks across pages the way the width does — a page without that lens
   shows its design. Through a lens the frame is somebody else's origin, so
   nothing here reads into it; what still works is what the address carries.
*/

(function () {
  var config = null;
  var collections = [];    /* the config's collections */
  var index = {};          /* src -> item */
  var root = window.wbConfig.root; /* the project root, as a URL */
  var pageList = null;     /* the page list, built by page-list.js once the config is in */
  var resolved = null;     /* the config as the server resolves it on its machine, or null */

  /* The annotation layer needs the same lookup for the handoff prompt, and has no
     business re-walking the config to get it. */
  window.wbItem = function (src) {
    return index[src] || null;
  };

  var shell = document.querySelector('.wb');
  var resizer = document.getElementById('resizer');
  var search = document.getElementById('search');
  var canvas = document.getElementById('canvas');
  var frame = document.getElementById('frame');
  var frameBuffer = document.getElementById('frameBuffer');
  var pendingFrame = null;
  var frameReady = false;
  var artboard = document.getElementById('artboard');
  var artboardContent = document.getElementById('artboardContent');
  var artboardName = document.getElementById('artboardName');
  var blank = document.getElementById('blank');
  var openLink = document.getElementById('open');
  var reload = document.getElementById('reload');
  var actionsToggle = document.getElementById('actionsToggle');
  var lensesBox = document.getElementById('lenses');
  var crumb = document.getElementById('crumb');
  var crumbPage = document.getElementById('crumbPage');
  var scenarioControl = document.getElementById('scenarioControl');
  var scenarioButton = document.getElementById('scenarioButton');
  var scenarioName = document.getElementById('scenarioName');
  var scenarioMenu = document.getElementById('scenarioMenu');
  var spaceName = document.getElementById('spaceName');
  var sourceControl = document.getElementById('sourceControl');
  var sourceButton = document.getElementById('openSource');
  var sourceMenu = document.getElementById('sourceMenu');
  var configureSpace = document.getElementById('configureSpace');
  var exportSpace = document.getElementById('exportSpace');
  var exportProgress = document.getElementById('exportProgress');
  var exportProgressBar = document.getElementById('exportProgressBar');
  var exportProgressText = document.getElementById('exportProgressText');
  /* The size switcher and its two dialogs: Web Components that show sizes
     and say what was asked for. This controller owns the size and every
     change to workbench.yaml; see src/components/size-switcher/. */
  var sizeSwitcher = document.getElementById('sizeSwitcher');
  var sizeDialog = document.getElementById('sizeDialog');
  var sizesEditor = document.getElementById('sizesEditor');

  var BLANK_TEXT = 'Pick a page or a component to see it here.';

  var current = null;      /* src of the picked page */
  var currentScenario = null; /* its scenario id, or null for the default one */
  var currentExample = null; /* on a docs page, the example the address names */
  var currentIterations = {}; /* each shown artboard's iteration: artboard id ("" for the implicit one) -> iteration id */
  var lensPref = null;     /* the lens the top bar was left on, or null for the design */
  var view = null;         /* what the canvas is showing — see wbView below */

  /* What the canvas is showing, for the annotation layer: it names screenshots
     and tells the agent what it is looking at, and both want the same
     answers. Null while nothing is picked. */
  window.wbView = function () {
    return view;
  };

  /* The shell has no toast of its own; the annotation layer's is the one. */
  function say(message) {
    window.dispatchEvent(new CustomEvent('wb-say', { detail: { message: message } }));
  }

  function exportJson(response) {
    return response.json().catch(function () {
      throw new Error('The export server answered ' + response.status + '.');
    }).then(function (result) {
      if (!response.ok || !result.ok) throw new Error(result.error || 'Export failed.');
      return result;
    });
  }

  function pollExport(job, startedAt) {
    return fetch('/_workbench/export?job=' + encodeURIComponent(job.id), { cache: 'no-store' }).then(exportJson).then(function (status) {
      exportProgressBar.max = Math.max(1, status.total || 0);
      var elapsed = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
      if (status.total && !status.completed) {
        exportProgressBar.removeAttribute('value');
        exportProgressText.textContent = 'Preparing capture… 0 of ' + status.total +
          ' complete · ' + elapsed + 's';
      } else {
        exportProgressBar.value = status.completed || 0;
        exportProgressText.textContent = status.total
          ? 'Captured ' + status.completed + ' of ' + status.total
          : 'Preparing download…';
      }
      if (status.status === 'failed') throw new Error(status.error || 'Export failed.');
      if (status.status === 'complete') return status;
      return new Promise(function (resolve) { setTimeout(resolve, 250); }).then(function () { return pollExport(job, startedAt); });
    });
  }

  function downloadExport(job) {
    exportProgressText.textContent = 'Downloading export…';
    return fetch('/_workbench/export?job=' + encodeURIComponent(job.id) + '&download=1')
      .then(function (response) {
        if (!response.ok) return exportJson(response).then(function () { throw new Error('Export failed.'); });
        var disposition = response.headers.get('Content-Disposition') || '';
        var match = /filename="([^"]+)"/.exec(disposition);
        return response.blob().then(function (blob) {
          return { blob: blob, name: match ? match[1] : 'canonic-design-system.zip' };
        });
      }).then(function (download) {
        var url = URL.createObjectURL(download.blob);
        var link = document.createElement('a');
        link.href = url;
        link.download = download.name;
        document.body.appendChild(link);
        link.click();
        link.remove();
        /* VS Code copies blob downloads through a temporary file. Keep the
           object URL alive while the editor completes that handoff. */
        setTimeout(function () { URL.revokeObjectURL(url); }, 60000);
        return download.name;
      });
  }

  exportSpace.addEventListener('click', function () {
    var dialog = document.getElementById('exportDialog');
    dialog.pages = Object.keys(resolved.pages).map(function (id) { return { id: id, label: resolved.pages[id].label }; });
    dialog.collections = (resolved.collections || []).concat(resolved.catalogCollections || []).map(function (c) { return c.name; });
    dialog.current = current ? { page: current, scenario: currentScenario, lens: view && view.lens ? view.lens.key : null,
      artboard: window.wbMultiCanvas ? window.wbMultiCanvas.selectedArtboard() : undefined,
      iteration: (window.wbMultiCanvas ? window.wbMultiCanvas.selectedIteration() : currentIterations['']) || undefined,
      size: canvas.dataset.size, width: window.wbMultiCanvas ? window.wbMultiCanvas.selectedSize().width : Math.round(frame.offsetWidth) || 1440,
      height: window.wbMultiCanvas ? window.wbMultiCanvas.selectedSize().height : Math.round(frame.offsetHeight) || 900 } : undefined;
    dialog.show();
  });
  document.getElementById('exportDialog').addEventListener('wb-export-request', function (event) {
    if (exportSpace.disabled) return;
    exportSpace.disabled = true;
    exportSpace.setAttribute('aria-busy', 'true');
    exportProgress.hidden = false;
    exportProgressBar.max = 1;
    exportProgressBar.value = 0;
    exportProgressText.textContent = 'Finding reference views…';
    window.dispatchEvent(new CustomEvent('wb-export-start'));
    say('Preparing export…');
    var finished = null;
    var startedAt = Date.now();
    fetch('/_workbench/export', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(event.detail) })
      .then(exportJson)
      .then(function (job) { return pollExport(job, startedAt); })
      .then(function (status) { finished = status; exportProgressText.textContent = 'Packaging download…'; return downloadExport(status); })
      .then(function (name) {
        say('Downloaded ' + name + (finished && finished.warnings ? ' with ' + finished.warnings + ' export warning(s).' : '.') +
          (finished && finished.warningMessages && finished.warningMessages.length ? ' ' + finished.warningMessages[0] : ''));
      })
      .catch(function (error) { say(String(error.message || error)); })
      .then(function () {
        exportProgress.hidden = true;
        exportSpace.disabled = false;
        exportSpace.removeAttribute('aria-busy');
        window.dispatchEvent(new CustomEvent('wb-export-end'));
      });
  });

  /* Anything marked data-icon gets its Lucide glyph injected once. */
  Array.prototype.forEach.call(document.querySelectorAll('[data-icon]'), function (el) {
    el.appendChild(window.wbIcon(el.dataset.icon));
  });

  window.wbConfigEditor.create({
    button: configureSpace,
    dialog: document.getElementById('configDialog'),
    body: document.getElementById('configBody'),
    status: document.getElementById('configStatus'),
    close: document.getElementById('configClose'),
    addCollection: document.getElementById('configAddCollection'),
    save: document.getElementById('configSave'),
    onSaved: function () {
      window.wbConfig.load(function (loaded) {
        refreshConfig(loaded);
        say('Saved workbench.yaml.');
      }, refuse);
    },
  });

  /* "#pages/sign-in.html:error@mobile~staging" -> { src, scenario, size, lens }.
     address.js does the reading; the size, the scenario, and the lens are
     checked against the page and the config when they're used. */
  function parseHash() {
    return window.wbAddress.parse(location.hash);
  }

  /* The default scenario and the page's default lens are addressed by leaving
     them out, so the hash for a page with scenarios and the hash for one
     without look the same. A pick that names no lens keeps the one the top
     bar is on. Iterations travel only when the caller keeps them — a lens
     switch does; another page, scenario or a followed link starts from the
     authored design. */
  function writeHash(src, scenario, size, lens, example, iterations) {
    /* A docs lens has no size, and only a docs lens has examples. */
    var docs = lensRules.isDocsLens(lensRules.chooseLens(index[src], config.implementations, lens || lensPref));
    location.hash = window.wbAddress.write({ src: src, scenario: scenario, iterations: iterations || null,
      size: docs ? null : size || null, lens: lens || null, example: docs ? example || null : null });
  }

  /* Once the canvas has settled on a page, the address says exactly what's
     showing — the lens that took effect, the story that was picked — so a
     copied link means this and not what was typed. Replaced rather than
     assigned: the canvas is already there, and a hashchange would only send
     it there again. */
  function syncHash() {
    var have = window.wbAddress.parse(location.hash);
    /* A page's default lens is left out: its first docs lens, or its design when it has no docs. */
    var lens = view ? lensRules.addressLens(view.item, config.implementations, view.lens) : null;
    var docs = !!view && lensRules.isDocsLens(view.lens);
    var size = docs ? null : sizeKey() || null;
    var example = docs ? currentExample : null;
    if (have.src === current && have.scenario === currentScenario && have.size === size && have.lens === lens &&
        have.example === example && sameIterations(have.iterations, currentIterations)) return;
    var want = window.wbAddress.write({ src: current, scenario: currentScenario, iterations: currentIterations,
      size: size, lens: lens, example: example });
    history.replaceState(null, '', location.pathname + location.search + (want ? '#' + want : ''));
  }

  window.wbTarget = parseHash;

  /* A single scenario is a page with a note attached, not a fold — the list
     only splits a page open when there is a choice to make. Same rule the
     panel folds by, so it comes from the same place. */
  var scenariosOf = window.wbPageList.scenarios;

  /* An id the item actually declares, or null for its default — which is
     what an unknown one falls back to rather than a blank frame. */
  function scenarioOf(item, id) {
    var scenarios = scenariosOf(item);
    if (!scenarios || !id) return null;
    for (var i = 1; i < scenarios.length; i++) {
      if (scenarios[i].id === id) return id;
    }
    return null;
  }

  function scenarioLabel(item, id) {
    var scenarios = scenariosOf(item) || [];
    for (var i = 0; i < scenarios.length; i++) {
      if (scenarios[i].id === (id || 'default')) return scenarios[i].label;
    }
    return null;
  }

  /* ------------------------------------------------ artboards, iterations */

  /* A scenario as the page index lists it, with the artboards and iterations
     it declares: a preview's from its definition, an authored page's from
     workbench.yaml. No scenario id is the page's first. */
  function scenarioEntry(src, id) {
    var page = resolved && resolved.pages && resolved.pages[src];
    var list = (page && page.preview && page.preview.scenarios) || (index[src] && index[src].scenarios) || [];
    if (!id) return list[0] || null;
    return list.filter(function (scenario) { return scenario.id === id; })[0] || null;
  }

  /* The scenario's declared artboards, or undefined for its one implicit artboard. */
  function declaredArtboards(src, scenario) {
    var entry = scenarioEntry(src, scenario);
    return entry && entry.artboards && entry.artboards.length ? entry.artboards : undefined;
  }

  /* What the address asked for, kept only where the scenario declares that
     artboard and that iteration, in the scenario's artboard order. Anything
     else falls back to the authored design. */
  function knownIterations(src, scenario, wanted) {
    var out = {};
    var entry = scenarioEntry(src, scenario);
    if (!entry || !wanted) return out;
    function keep(key, iterations) {
      var id = wanted[key];
      if (id && (iterations || []).some(function (iteration) { return iteration.id === id; })) out[key] = id;
    }
    if (entry.artboards && entry.artboards.length) {
      entry.artboards.forEach(function (board) { keep(board.id, board.iterations); });
    } else keep('', entry.iterations);
    return out;
  }

  /* The selected iteration of one artboard, with its label and source, or null. */
  function iterationEntry(src, scenario, artboardId, id) {
    var entry = scenarioEntry(src, scenario);
    if (!entry || !id) return null;
    var owner = artboardId
      ? (entry.artboards || []).filter(function (board) { return board.id === artboardId; })[0]
      : entry;
    return ((owner && owner.iterations) || []).filter(function (iteration) { return iteration.id === id; })[0] || null;
  }

  function sameIterations(a, b) {
    return JSON.stringify(a || {}) === JSON.stringify(b || {});
  }

  /* ------------------------------------------------------------ lenses */

  /* Which lens a page shows, and which it has, are decided by the docs
     module (src/modules/docs/canvas/lenses.ts), loaded before the config is
     applied: see whenRules. Docs are a lens there, so a page can be read as
     its docs beside its design. Off a server (file://) the module can't
     load and docs can't render, so pages have their other lenses only. */
  var lensRules = null;
  var NO_DOCS = {
    DESIGN_KEY: 'design',
    pageLenses: function (item, impls) {
      if (!item) return [];
      return Object.keys(impls).filter(function (key) { return !!(item.implementations && item.implementations[key]) && impls[key].kind !== 'docs'; })
        .map(function (key) { return impls[key]; });
    },
    chooseLens: function (item, impls, wanted) {
      if (!item) return null;
      if (item.implementationOnly) return impls[item.implementationOnly] || null;
      return NO_DOCS.pageLenses(item, impls).filter(function (lens) { return lens.key === wanted; })[0] || null;
    },
    hasLens: function (item, impls, key) { return NO_DOCS.pageLenses(item, impls).some(function (lens) { return lens.key === key; }); },
    hasDesignLens: function (item) { return !!item && !item.implementationOnly; },
    addressLens: function (item, impls, lens) { return lens ? lens.key : null; },
    docsLinkLens: function () { return null; },
    isDocsLens: function () { return false; },
    isMarkdownPage: function () { return false; },
    canvasMode: function () { return 'default'; },
  };

  /* Which lenses a page has besides its design, in order. */
  function lensesOf(item) {
    return lensRules.pageLenses(item, config.implementations);
  }

  /* The lens the top bar is on, if this page has it; else the page's default. */
  function effectiveLens(item) {
    return lensRules.chooseLens(item, config.implementations, lensPref);
  }

  function setLensPref(key) {
    lensPref = key || null;
    try {
      localStorage.setItem('canonic-workbench-lens', lensPref || '');
    } catch (e) {
      /* file:// storage can be blocked; the lens still holds this session. */
    }
  }

  /* The address the frame loads: the design with its flags, or the page's
     path on the implementation. Stories are asked for first — see loadStory. */
  function viewUrl(item, lens, scenario) {
    /* The server serves a page's Markdown as its docs, in the lens named. */
    if (lensRules.isDocsLens(lens)) return withFlags(item.markdown, scenario) + '&lens=' + encodeURIComponent(lens.key);
    if (!lens) return withFlags(item.src, scenario, currentIterations['']);
    if (lens.kind === 'workbench') {
      var target = new URL(window.wbLenses.url(lens, item.implementations[lens.key], scenario), config.root);
      target.searchParams.set('actions', actionsOn ? 'on' : 'off');
      var artboard = new URLSearchParams(location.search).get('artboard');
      if (artboard) target.searchParams.set('artboard', artboard);
      return target.href;
    }
    return window.wbLenses.url(lens, item.implementations[lens.key], scenario);
  }

  /* The page into the iframe, the usual way. */
  function showFrame(url) {
    if (window.wbMultiCanvas) {
      var sequence = showSeq;
      var preview = resolved && resolved.pages[current] && resolved.pages[current].preview;
      var declared = (!view.lens || view.lens.kind === 'workbench') && preview ? declaredArtboards(current, currentScenario) : undefined;
      window.wbMultiCanvas.show({ src: current, label: titleParts(view.item, view.scenarioLabel, view.lens).join(' / '), pageLabel: view.item.label, scenario: currentScenario, lens: view.lens ? view.lens.key : null,
        example: currentExample, docs: lensRules.isDocsLens(view.lens), size: sizeByKey(canvas.dataset.size) || spaceSizes[0], actions: actionsOn,
        artboards: declared,
        /* Each renderer keeps its iteration through every lens, so its
           annotations stay with it; only the design lens renders it. */
        iterations: currentIterations,
        select: function (scenario, size, actions, src, lens) {
          if (actions !== undefined) setActions(actions);
          var preset = sizesOf(view.item).find(function (entry) { return entry.kind === 'fixed' && entry.width === size.width && entry.height === size.height; });
          if (preset) canvas.dataset.size = preset.key;
          if ((src && src !== current) || currentScenario !== scenario) writeHash(src || current, scenario, canvas.dataset.size, lens || (view.lens ? view.lens.key : null), currentExample);
        },
      }).catch(function (error) { if (sequence === showSeq) window.wbMultiCanvas.fail(error); });
      if (url) openLink.href = view && view.lens ? window.wbLenses.upstream(view.lens, url) : url;
      else openLink.removeAttribute('href');
      return;
    }
    window.wbSimulator.hide();
    blank.hidden = true;
    blank.classList.remove('is-loading');
    /* With no current document, keep the whole canvas out of sight until the
       first styled page is ready. Later navigation leaves it visible while
       the spare iframe loads behind it. */
    artboard.hidden = !frame.getAttribute('src');
    loadPreview(url);
    /* A lens loads through the workbench's proxy; on its own, the page
       opens at the implementation's own address. */
    openLink.href = view && view.lens ? window.wbLenses.upstream(view.lens, url) : url;
  }

  var pendingStorySwitch = null;
  var renderedStory = null;
  var reportedStorySeq = null;

  function cancelStorySwitch() {
    if (!pendingStorySwitch) return;
    clearTimeout(pendingStorySwitch.timer);
    if (pendingStorySwitch.frame !== pendingFrame) sessions.protect(pendingStorySwitch.frame, false);
    pendingStorySwitch = null;
  }

  function fallBackStorySwitch(switching) {
    if (pendingStorySwitch !== switching || switching.seq !== showSeq) return;
    pendingStorySwitch = null;
    loadPreview(switching.url, true);
  }

  function reportRenderedStory() {
    if (!renderedStory || reportedStorySeq === showSeq || !frameReady || pendingFrame ||
        renderedStory.frame !== frame || renderedStory.seq !== showSeq || !view ||
        !view.story || view.story.id !== renderedStory.id) return;
    reportedStorySeq = showSeq;
    tellHost(current, currentScenario, view.lens.key);
  }

  /* Keep Storybook's loaded preview warm. The channel does not promise that
     the preview accepted a message, so navigate only if no acknowledgement
     arrives. Once accepted, wait for its render event before telling capture
     that the new story is ready. */
  function reuseStoryFrame(previous, lens, storyId, seq) {
    var cached = sessions.get(sessionKey(window.wbLenses.storyUrl(lens, storyId)));
    var target = cached && cached.ready ? cached.frame :
      frameReady && !pendingFrame && previous && previous.lens && previous.lens.kind === 'storybook' && previous.lens.url === lens.url ? frame : null;
    if (!target) return false;
    window.wbSimulator.hide();
    blank.hidden = true;
    blank.classList.remove('is-loading');
    target.wbRequestedUrl = window.wbLenses.storyUrl(lens, storyId);
    if (target !== frame || !frameReady) {
      cancelPendingFrame();
      pendingFrame = target;
      target.wbRestoring = true;
      target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
      sessions.protect(target, true);
    }
    if (target.wbStoryId === storyId) {
      renderedStory = { frame: target, id: storyId, seq: seq };
      if (pendingFrame === target) frameLoaded({ currentTarget: target });
      else { reportRenderedStory(); window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } })); }
      return true;
    }
    if (!window.wbLenses.selectStory(target, lens, storyId)) return false;
    target.wbStoryId = null;
    var switching = { frame: target, lens: lens, id: storyId, seq: seq,
      url: window.wbLenses.storyUrl(lens, storyId), timer: null };
    switching.timer = setTimeout(function () { fallBackStorySwitch(switching); }, 1500);
    pendingStorySwitch = switching;
    return true;
  }

  function storySwitchEvent(message) {
    var switching = pendingStorySwitch;
    if (!view || !view.story || !view.lens || view.lens.kind !== 'storybook') return;
    var target = pendingFrame || frame;
    var event = window.wbLenses.storybookEvent(message, target, view.lens);
    if (!event || event.id !== view.story.id) return;
    if (switching && switching.seq === showSeq && event.type === 'currentStoryWasSet') {
      clearTimeout(switching.timer);
      switching.timer = setTimeout(function () { fallBackStorySwitch(switching); }, 15000);
    } else if (event.type === 'storyRendered') {
      if (switching && switching.seq === showSeq) cancelStorySwitch();
      var wasReported = reportedStorySeq === showSeq;
      renderedStory = { frame: target, id: event.id, seq: showSeq };
      target.wbStoryId = event.id;
      if (target === pendingFrame && target.wbRestoring) frameLoaded({ currentTarget: target });
      reportRenderedStory();
      if (!wasReported && reportedStorySeq === showSeq && target === frame && !pendingFrame) {
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      }
    }
  }

  /* Where the code is, for the handoff: what the story says about itself,
     then every pointer the config resolved for this page on this machine. */
  function codeFor(item, story) {
    var out = [];
    function add(file) {
      if (file && out.indexOf(file) === -1) out.push(file);
    }
    ((story && story.code) || []).forEach(add);
    var info = resolved && resolved.pages && resolved.pages[item.src];
    ((info && info.code) || []).forEach(function (entry) {
      if (entry.exists) add(entry.path);
    });
    return out;
  }

  /* A switcher needs at least two lenses to switch between. A design with no
     implementation has one (itself); a page imported from a single
     implementation, with no design, has one too. Neither shows the control.
     A Markdown page has no design: its docs lenses come first. */
  function drawLenses(item, lens) {
    lensesBox.innerHTML = '';
    /* Docs first, then the page's own design, then its implementations. */
    var lenses = lensesOf(item);
    var docs = lenses.filter(lensRules.isDocsLens);
    var others = lenses.filter(function (impl) { return !lensRules.isDocsLens(impl); });
    var choices = lensRules.hasDesignLens(item) ? docs.concat([null], others) : lenses;
    lensesBox.hidden = choices.length < 2;
    if (lensesBox.hidden) return;
    var showing = lens ? lens.key : lensRules.DESIGN_KEY;
    choices.forEach(function (impl) {
      var key = impl ? impl.key : lensRules.DESIGN_KEY;
      var button = document.createElement('button');
      button.className = 'wb-lens';
      button.type = 'button';
      button.dataset.lens = key;
      button.textContent = impl ? impl.label : window.wbManifest.authoredLensLabel(item);
      button.title = impl ? 'This page as ' + impl.label + ' has it' : 'This page as designed';
      button.setAttribute('aria-pressed', String(showing === key));
      button.addEventListener('click', function () {
        setLensPref(key);
        writeHash(current, currentScenario, sizeKey(), key, currentExample, currentIterations);
      });
      lensesBox.appendChild(button);
    });
  }

  /* Through a lens the page is served by somebody else and actions.js isn't
     in it, so the switch has nothing to switch: it shows on and stays put. */
  function setActionsAvailability(lens) {
    if (lens && lens.kind !== 'workbench' && lens.kind !== 'docs') {
      actionsToggle.disabled = true;
      actionsToggle.checked = true;
      actionsToggle.hint = 'always on here: ' + lens.label +
        ' serves this page, not the workbench, so nothing inside it is switched off';
      return;
    }
    actionsToggle.disabled = false;
    actionsToggle.checked = actionsOn;
    actionsToggle.hint = '';
  }

  /* ----------------------------------------------------------- stories */

  /* A Storybook lens shows one story at a time, and which stories a title
     has is the Storybook's to say — the server asks its index. Answers are
     kept for the session; a refresh forgets them. */
  var storyCache = {};

  function stories(lens, title) {
    if (window.wbMultiCanvas) return window.wbMultiCanvas.stories(lens.key, title);
    if (window.wbRequestStories) return window.wbRequestStories(lens.key, title);
    var key = lens.key + '|' + title;
    if (storyCache[key]) return storyCache[key];
    var request = fetch(
      '/_workbench/stories?implementation=' + encodeURIComponent(lens.key) + '&title=' + encodeURIComponent(title)
    )
      .then(function (res) {
        return res.json().catch(function () {
          throw new Error('The workbench server answered ' + res.status + ' when asked for stories.');
        });
      })
      .then(function (result) {
        if (!result.ok || !result.stories || !result.stories.length) {
          throw new Error(result.error || 'No stories titled “' + title + '”.');
        }
        return result.stories;
      })
      .catch(function (error) {
        delete storyCache[key];
        if (error instanceof TypeError) {
          throw new Error('Stories need the workbench server — the Workbench extension runs one, and so does node server.js.');
        }
        throw error;
      });
    storyCache[key] = request;
    return request;
  }

  function openScenarios(open) {
    scenarioMenu.hidden = !open;
    scenarioButton.setAttribute('aria-expanded', String(open));
  }

  /* The breadcrumb's second half: which version of the page is showing,
     and the others to pick from — the page's scenarios on the design, the
     title's stories under a Storybook lens. rows: [{ label, current, pick }],
     or null when there is nothing to choose between. */
  function drawScenarioMenu(rows, kind) {
    scenarioMenu.innerHTML = '';
    openScenarios(false);
    scenarioControl.hidden = !rows;
    if (!rows) { scenarioName.textContent = ''; scenarioButton.removeAttribute('title'); return; }
    var picked = rows.filter(function (row) { return row.current; })[0] || rows[0];
    scenarioName.textContent = picked.label;
    scenarioButton.title = (kind === 'story' ? 'Story: ' : 'Scenario: ') + picked.label + ' — pick another';
    scenarioMenu.setAttribute('aria-label', kind === 'story' ? 'Story' : 'Scenario');
    rows.forEach(function (entry) {
      var row = document.createElement('button');
      row.className = 'wb-menu-row';
      row.type = 'button';
      row.setAttribute('role', 'menuitem');
      row.setAttribute('aria-current', String(entry === picked));
      row.textContent = entry.label;
      row.dataset.scenario = entry.target || '';
      row.addEventListener('click', function () {
        openScenarios(false);
        entry.pick();
      });
      scenarioMenu.appendChild(row);
    });
  }

  function drawStories(list, picked, lens) {
    drawScenarioMenu(list && list.map(function (story) {
      return {
        label: story.name,
        target: story.scenario,
        current: story.id === picked.id,
        pick: function () { writeHash(current, story.scenario, sizeKey(), lens.key); },
      };
    }), 'story');
  }

  /* A design's scenarios, or a lens that answers to them. An implementation
     with none of its own shows the page itself and nothing to pick. Every
     example in a docs lens gets the page's scenario. */
  function drawScenarios(item, lens) {
    var scenarios = scenariosOf(item);
    var mapped = !lens || item.implementationOnly || lensRules.isDocsLens(lens) || !!item.implementations[lens.key].scenarios;
    if (!scenarios || !mapped || window.wbManifest.streamed(lens)) {
      drawScenarioMenu(null);
      return;
    }
    drawScenarioMenu(scenarios.map(function (scenario, i) {
      return {
        label: scenario.label,
        target: window.wbPageList.scenarioPick(scenario, i),
        current: currentScenario ? currentScenario === scenario.id : i === 0,
        pick: function () {
          writeHash(current, window.wbPageList.scenarioPick(scenario, i), sizeKey(), lens ? lens.key : null);
        },
      };
    }), 'scenario');
  }

  /* The story's turn of show(): the title's stories are asked for, the one
     the address names — or the first — goes in the frame. `seq` is show()'s
     count; an answer for a page that's no longer up is dropped. */
  function loadStory(item, lens, scenario, seq, previousView) {
    var ref = item.implementations[lens.key];
    if (!window.wbMultiCanvas) {
      blank.textContent = 'Loading preview…';
      blank.classList.add('is-loading');
      blank.hidden = false;
    }

    stories(lens, ref.title).then(
      function (list) {
        if (seq !== showSeq) return;
        var picked = window.wbLenses.pick(list, scenario);
        currentScenario = picked.scenario;
        view.story = picked;
        view.scenarioLabel = picked.name;
        view.url = window.wbLenses.storyUrl(lens, picked.id);
        view.code = codeFor(item, picked);
        drawStories(list, picked, lens);
        setTitle(item, picked.name, lens);
        syncHash();
        if (!reuseStoryFrame(previousView, lens, picked.id, seq)) showFrame(view.url);
        openLink.href = window.wbLenses.upstream(lens, window.wbLenses.storyOpenUrl(lens, picked.id));
      },
      function (error) {
        if (seq !== showSeq) return;
        currentScenario = null;
        artboard.hidden = true;
        frameReady = false;
        parkPreview();
        openLink.removeAttribute('href');
        blank.textContent = String(error.message || error);
        blank.classList.remove('is-loading');
        blank.hidden = false;
        if (window.wbMultiCanvas) window.wbMultiCanvas.fail(error);
        tellHost(null, null, null);
        setTitle(item, null, lens);
        syncHash();
      }
    );
  }

  /* ------------------------------------------------------------ source */

  /* The page's source, as the server resolved it on its machine: the
     design file, then wherever each implementation keeps the code. A path
     that isn't there is listed but can't be opened, so the config's intent
     still shows. Nothing serving that answer, no control. */
  function openSources(open) {
    sourceMenu.hidden = !open;
    sourceButton.setAttribute('aria-expanded', String(open));
  }

  function sourceRow(label, sub, enabled, title, onPick) {
    var row = document.createElement('button');
    row.className = 'wb-menu-row';
    row.type = 'button';
    row.setAttribute('role', 'menuitem');
    row.disabled = !enabled;
    row.title = title;
    row.appendChild(document.createTextNode(label));
    var line = document.createElement('span');
    line.className = 'wb-menu-sub';
    line.textContent = sub;
    row.appendChild(line);
    if (enabled) {
      row.addEventListener('click', function () {
        openSources(false);
        onPick();
      });
    }
    sourceMenu.appendChild(row);
  }

  function drawSources(item) {
    var info = item && resolved && resolved.pages && resolved.pages[item.src];
    sourceMenu.innerHTML = '';
    openSources(false);
    sourceControl.hidden = !info;
    if (!info) return;
    if (info.design) {
      sourceRow(lensRules.hasDesignLens(item) ? window.wbManifest.authoredLensLabel(item) : 'Docs', item.src, true, info.design, function () {
        openSource(item.src, null);
      });
    }
    var lenses = lensesOf(item);
    info.code.forEach(function (entry) {
      var impl = lenses.filter(function (lens) { return lens.key === entry.implementation; })[0] || config.implementations[entry.implementation];
      var label = entry.label || (impl ? impl.label : entry.implementation);
      var title = !entry.path
        ? 'Implementation “' + entry.implementation + '” has no root, so this can’t be found'
        : entry.exists ? entry.path : 'Not on this machine: ' + entry.path;
      sourceRow(label, entry.relative, !!entry.exists, title, function () {
        openSource(item.src, entry.path);
      });
    });
  }

  function openSource(src, file) {
    fetch('/_workbench/open', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ src: src, path: file }),
    })
      .then(function (res) {
        return res.json();
      })
      .then(function (result) {
        if (!result.ok) throw new Error(result.error || 'the editor refused it');
        say('Opened ' + result.opened.split('/').pop());
      })
      .catch(function (error) {
        say('Couldn’t open it — ' + String(error.message || error));
        console.error('[workbench] open failed', error);
      });
  }

  /* ------------------------------------------------------------ canvas */

  var showSeq = 0;

  function titleParts(item, scenarioLabelText, lens) {
    var scenarios = scenariosOf(item);
    var shown = scenarioLabelText || (scenarios && !lens ? scenarios[0].label : null);
    return [item.label, shown].filter(Boolean);
  }

  function setTitle(item, scenarioLabelText, lens) {
    /* The scenario and the lens are part of what you're looking at, so they
       belong in the tab title next to the page's name. */
    var parts = titleParts(item, scenarioLabelText, lens);
    document.title = parts.concat([lens ? lens.label : null, config.name])
      .filter(Boolean)
      .join(' · ');
    /* Above the frame, a page with scenarios always names the one showing,
       its first included — the breadcrumb does the same. */
    artboardName.textContent = parts.join(' / ');
    window.dispatchEvent(new Event('wb-view-change'));
  }

  /* The canvas mode follows the lens: 'docs' for a docs lens, whose page
     fills the canvas; 'default' for every other lens, on an artboard. */
  function setCanvasMode(mode) {
    if (heldLayout) { heldLayout.mode = mode; return; }
    applyCanvasMode(mode);
  }

  function applyCanvasMode(mode) {
    if (shell.dataset.canvasMode === mode) return;
    shell.dataset.canvasMode = mode;
    if (window.wbZoom && window.wbZoom.mode) window.wbZoom.mode(mode);
    /* Back from a docs page, the next page's size is applied afresh. */
    if (mode === 'default') appliedSize = '';
  }

  /* While the next page loads behind the one showing, the canvas keeps the
     showing page's mode and size: the next page's are held here and applied
     in the same frame that swaps it in, so the old page is never drawn at
     the new page's dimensions. */
  var heldLayout = null;

  function holdLayout() {
    if (!heldLayout && frameReady && !artboard.hidden) heldLayout = { mode: null, size: null };
  }

  function commitLayout() {
    var held = heldLayout;
    heldLayout = null;
    if (!held) return;
    if (held.mode) applyCanvasMode(held.mode);
    /* Leaving a docs page re-applies the size the canvas was on. */
    var key = held.size || (shell.dataset.canvasMode === 'default' && !appliedSize ? canvas.dataset.size : null);
    var size = key && sizeByKey(key);
    if (size && (key !== canvas.dataset.size || signature(size) !== appliedSize)) applySize(size);
  }

  /* The size the canvas is on, or the one it moves to with the next page. */
  function sizeKey() {
    return (heldLayout && heldLayout.size) || canvas.dataset.size;
  }

  /* The docs page in the frame, when it has loaded its script. */
  function docsPage() {
    try { return frameReady && frame.contentWindow && frame.contentWindow.wbDocsPage || null; } catch (e) { return null; }
  }

  function scrollToExample(example) {
    var page = docsPage();
    if (page && example) page.scrollToExample(example);
  }

  /* An address that names an example opens the page there, once its frame is
     showing. A reload after an edit keeps its own scroll position instead. */
  var exampleToReveal = null;

  function revealExample() {
    if (!exampleToReveal || !docsPage()) return;
    scrollToExample(exampleToReveal);
    exampleToReveal = null;
  }

  function show(src, scenario, example, iterations) {
    var item = index[src];
    var nextLens = item ? effectiveLens(item) : null;
    var docsItem = lensRules.isDocsLens(nextLens);
    var nextScenario = docsItem ? scenarioOf(item, scenario) : null;
    /* Another example on the docs already showing only scrolls them. */
    if (docsItem && view && view.src === src && view.scenario === nextScenario && frameReady &&
        (view.lens ? view.lens.key : null) === nextLens.key) {
      currentExample = example || null;
      scrollToExample(currentExample);
      syncHash();
      /* Routing still acknowledges the selection when its preview is reused.
         The editor holds sidebar picks until a config refresh is acknowledged. */
      tellHost(src, nextScenario, nextLens ? nextLens.key : null);
      return;
    }
    if (window.wbMultiCanvas) window.wbMultiCanvas.begin();
    currentExample = docsItem ? example || null : null;
    exampleToReveal = currentExample;
    holdLayout();
    setCanvasMode(lensRules.canvasMode(nextLens));
    var seq = ++showSeq;
    var previousView = view;
    frame.wbFrameSize = { width: frame.offsetWidth, height: frame.offsetHeight };
    cancelStorySwitch();
    cancelPendingFrame();
    renderedStory = null;
    current = item ? src : null;
    var lens = nextLens;
    var story = !!lens && lens.kind === 'storybook';
    /* Under a Storybook lens the scenario slot carries the story; it's checked
       against the title's stories once they're in. */
    currentScenario = !item ? null : story ? scenario || null : scenarioOf(item, scenario);
    /* A story is no scenario to check iterations against: keep them as they are
       for the way back to the design lens, which checks them again. */
    currentIterations = !item ? {} : story ? Object.assign({}, iterations || {}) : knownIterations(src, currentScenario, iterations);
    updateSizeAvailability(item, lens);

    /* The list marks the row and follows it into its collection, wherever the
       pick came from — its own rows, a link in the preview, or the editor.
       Through a lens that doesn't answer to the page's scenarios, the row
       marked is the page itself. */
    var mapped = !item || item.implementationOnly || docsItem || !lens || (!story && !!item.implementations[lens.key].scenarios);
    var reported = mapped ? currentScenario : null;
    if (pageList) pageList.setCurrent(current, reported, item && item.collection);
    tellHost(current, reported, lens ? lens.key : null, story);

    view = null;
    if (window.wbPreviewControls) window.wbPreviewControls.reset();
    drawLenses(item, lens);
    crumb.hidden = !item;
    crumbPage.textContent = item ? item.label : '';
    /* A story's turn comes once the title's stories are in. */
    if (item && !story) drawScenarios(item, lens);
    else drawScenarioMenu(null);
    drawSources(item);
    setActionsAvailability(lens);

    if (!item) {
      if (window.wbMultiCanvas) window.wbMultiCanvas.clear();
      commitLayout();
      window.wbSimulator.hide();
      artboard.hidden = true;
      frameReady = false;
      parkPreview();
      blank.textContent = BLANK_TEXT;
      blank.classList.remove('is-loading');
      blank.hidden = false;
      openLink.removeAttribute('href');
      document.title = config.name;
      artboardName.textContent = '';
      return;
    }

    view = {
      src: src,
      item: item,
      lens: lens,
      scenario: story ? null : currentScenario,
      scenarioLabel: null,
      /* The implicit artboard's iteration, on the design only. */
      iteration: lens ? null : iterationEntry(src, currentScenario, null, currentIterations['']),
      story: null,
      url: null,
      code: codeFor(item, null),
    };
    if (view.iteration && view.iteration.source && view.code.indexOf(view.iteration.source) === -1) {
      view.code.unshift(view.iteration.source);
    }

    if (lens && lens.problem) {
      var lensError = new Error(lens.problem);
      artboard.hidden = true;
      frameReady = false;
      parkPreview();
      openLink.removeAttribute('href');
      blank.textContent = lens.problem;
      blank.classList.remove('is-loading');
      blank.hidden = false;
      if (window.wbMultiCanvas) window.wbMultiCanvas.fail(lensError);
      setTitle(item, null, lens);
      syncHash();
      return;
    }

    if (story) {
      setTitle(item, null, lens);
      commitLayout();
      loadStory(item, lens, scenario, seq, previousView);
      return;
    }

    if (window.wbManifest.streamed(lens)) {
      commitLayout();
      if (window.wbMultiCanvas) { setTitle(item, null, lens); syncHash(); showFrame(null); return; }
      var streamRef = item.implementations[lens.key];
      currentScenario = null;
      view.url = null;
      frameReady = false;
      parkPreview();
      artboardContent.classList.remove('is-loading');
      artboard.hidden = false;
      blank.hidden = true;
      openLink.removeAttribute('href');
      window.wbSimulator.show({
        kind: lens.kind,
        implementation: lens.key,
        udid: streamRef.device,
        src: item.src,
        label: item.label,
      });
      setTitle(item, null, lens);
      syncHash();
      return;
    }

    var url = viewUrl(item, lens, currentScenario);
    view.url = url;
    view.scenarioLabel = currentScenario ? scenarioLabel(item, currentScenario) : null;
    setTitle(item, view.scenarioLabel, lens);
    syncHash();

    showFrame(url);
    /* With nothing showing yet, the page loads at its own size. Otherwise
       frameLoaded applies the held layout with the page it brings in. */
    if (!frameReady || artboardContent.classList.contains('is-loading')) commitLayout();
  }

  /* ----------------------------------------------------------- actions */

  /* Whether the preview is live. Off means links don't navigate and forms
     don't submit, so clicking around a page never takes you off it.

     The shell can't enforce that itself — off file:// the iframe is a foreign
     origin — so the flag travels in the URL and each page applies it through
     actions.js. It goes on the "open on its own" link too, so a page in its
     own tab behaves the way the top bar says it should. */
  var actionsOn = false;

  /* The address the frame loads: the page resolved against the project root,
     carrying both flags it needs — whether the preview is live, and which
     scenario of the page to render. Same trip, same reason — the shell can't
     reach across the origin, so it hands them over in the URL. */
  function withFlags(src, scenario, iteration) {
    var url = root + src + (src.indexOf('?') > -1 ? '&' : '?') +
      'actions=' + (actionsOn ? 'on' : 'off');
    var artboard = new URLSearchParams(location.search).get('artboard');
    if (artboard) url += '&artboard=' + encodeURIComponent(artboard);
    if (iteration) url += '&iteration=' + encodeURIComponent(iteration);
    return scenario ? url + '&scenario=' + encodeURIComponent(scenario) : url;
  }

  /* Keep the current document visible while its replacement loads in the
     spare iframe, then exchange them in one paint. The first page has nothing
     to preserve, so it uses the frame background until ready. */
  function warmFrame(target) {
    target.wbWarmHost = true;
    target.src = new URL('preview-host.html?actions=off', location.href).href;
  }

  var previewFrames = [frame, frameBuffer];
  var sessions = window.wbPreviewSessions.create({ dispose: disposeFrame });

  function sessionKey(url) {
    var lens = view && view.lens;
    return lens && lens.kind === 'storybook' ? 'storybook|' + lens.key + '|' + lens.url :
      (lens ? lens.key : 'design') + '|' + new URL(url, location.href).href;
  }

  function disposeFrame(target) {
    if (target.wbDisposed) return;
    target.wbDisposed = true;
    target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
    previewFrames = previewFrames.filter(function (candidate) { return candidate !== target; });
    function remove() { target.removeAttribute('src'); target.remove(); }
    var renderer;
    try { renderer = target.wbWarmHost && target.contentWindow && target.contentWindow.wbPreviewHost; } catch (_) {}
    if (renderer) renderer.reset().then(remove, function (error) { console.warn('[workbench] preview cleanup failed', error); remove(); });
    else remove();
  }

  function spareFrame() {
    var available = previewFrames.find(function (candidate) { return candidate !== frame && candidate !== pendingFrame && !sessions.has(candidate); });
    if (available) return available;
    var target = document.createElement('iframe');
    target.title = 'Preview loading';
    target.setAttribute('allow', 'clipboard-read; clipboard-write');
    target.setAttribute('aria-hidden', 'true');
    target.inert = true;
    target.addEventListener('load', frameLoaded);
    artboardContent.appendChild(target);
    previewFrames.push(target);
    return target;
  }

  function cancelPendingFrame() {
    if (!pendingFrame) return;
    var previous = pendingFrame;
    pendingFrame = null;
    previous.wbLoadSequence = (previous.wbLoadSequence || 0) + 1;
    if (previous.wbRestoring) sessions.protect(previous, false);
    else sessions.remove(previous);
  }

  function parkPreview() {
    cancelPendingFrame();
    sessions.park();
    frame.classList.remove('is-active');
    frame.setAttribute('aria-hidden', 'true');
    frame.inert = true;
    freezeFrameSize(frame);
    frameReady = false;
  }

  function freezeFrameSize(target) {
    if (target.style && target.wbFrameSize && target.wbFrameSize.width) {
      target.style.width = target.wbFrameSize.width + 'px';
      target.style.height = target.wbFrameSize.height + 'px';
    }
  }

  function startWarmLoad(target) {
    var address = target.wbRequestedUrl;
    var seq = target.wbLoadSequence;
    var renderer = target.contentWindow && target.contentWindow.wbPreviewHost;
    if (!address || !renderer) return;
    renderer.load(address).then(function () {
      if (seq !== target.wbLoadSequence || target.wbRequestedUrl !== address) return;
      frameLoaded({ currentTarget: target, warm: true });
    }).catch(function () { /* The renderer displays its own loading failure. */ });
  }

  function prepareFrame(target, src) {
    target.wbRequestedUrl = src;
    target.wbLoadSequence = (target.wbLoadSequence || 0) + 1;
    if (/\.workbench\.tsx?(?:[?#]|$)/.test(src) && new URL(src, location.href).origin === location.origin) {
      if (target.wbWarmHost && target.contentWindow && target.contentWindow.wbPreviewHost) startWarmLoad(target);
      else warmFrame(target);
    } else {
      target.wbWarmHost = false;
      target.src = src;
    }
  }

  function loadPreview(src, force) {
    cancelPendingFrame();
    if (previewFrames.indexOf(frame) === -1) frame = spareFrame();
    var key = sessionKey(src);
    var cached = sessions.get(key);
    if (force && cached) {
      if (cached.frame === frame) sessions.forget(frame);
      else sessions.remove(cached.frame);
      cached = null;
    }
    if (cached && cached.ready) {
      var retained = cached.frame;
      if (retained.style) { retained.style.width = ''; retained.style.height = ''; }
      retained.wbRestoring = true;
      retained.wbRequestedUrl = src;
      retained.wbLoadSequence = (retained.wbLoadSequence || 0) + 1;
      sessions.protect(retained, true);
      pendingFrame = retained;
      var sequence = retained.wbLoadSequence;
      var renderer = retained.wbWarmHost && retained.contentWindow && retained.contentWindow.wbPreviewHost;
      Promise.resolve(renderer ? renderer.resume(src) : null).then(function () {
        if (pendingFrame === retained && sequence === retained.wbLoadSequence) frameLoaded({ currentTarget: retained, warm: true });
      }, function (error) {
        if (pendingFrame !== retained || sequence !== retained.wbLoadSequence) return;
        console.warn('[workbench] retained preview could not resume', error);
        loadPreview(src, true);
      });
      return;
    }
    // The warm document: the visible warm host changes page in place, as an
    // application's router would, keeping loaded dependencies, stylesheets and
    // fonts. A new scenario of the same preview re-renders without reloading it.
    if (!force && frame.wbWarmHost && frame.wbRequestedUrl && frameReady && /\.workbench\.tsx?(?:[?#]|$)/.test(src) &&
        new URL(src, location.href).origin === location.origin) {
      var showing = new URL(frame.wbRequestedUrl, location.href);
      var next = new URL(src, location.href);
      sessions.forget(frame);
      frame.wbSessionKey = key;
      frame.wbRestoring = false;
      sessions.add(key, frame);
      var host = frame.contentWindow;
      /* Another iteration can bring its own source, so it loads the document again. */
      var sameIteration = showing.searchParams.get('iteration') === next.searchParams.get('iteration');
      if (showing.pathname === next.pathname && sameIteration && host && host.__workbenchOptions && !host.__workbenchError) {
        frame.wbRequestedUrl = src;
        frame.wbLoadSequence = (frame.wbLoadSequence || 0) + 1;
        host.history.replaceState(host.history.state, '', next.href);
        host.__workbenchReady = false;
        host.postMessage({ type: 'workbench-preview-command', command: 'scenario', scenario: next.searchParams.get('scenario') || undefined }, location.origin);
        frameLoaded({ currentTarget: frame, warm: true });
        return;
      }
      prepareFrame(frame, src);
      return;
    }
    if (!frame.getAttribute('src') || (frame.wbWarmHost && !frame.wbRequestedUrl && !frameReady)) {
      artboardContent.classList.add('is-loading');
      frameReady = false;
      frame.wbSessionKey = key;
      frame.wbRestoring = false;
      sessions.add(key, frame);
      prepareFrame(frame, src);
      return;
    }
    pendingFrame = frameBuffer = spareFrame();
    pendingFrame.wbSessionKey = key;
    pendingFrame.wbRestoring = false;
    sessions.add(key, pendingFrame);
    prepareFrame(pendingFrame, src);
  }

  function frameLoaded(e) {
    var loaded = e.currentTarget;
    if (previewFrames.indexOf(loaded) === -1 || (e.sequence !== undefined && e.sequence !== loaded.wbLoadSequence)) return;
    var sequence = loaded.wbLoadSequence;
    if (loaded.wbWarmHost && !e.warm) {
      startWarmLoad(loaded);
      return;
    }
    try {
      var preview = loaded.contentWindow;
      if (preview && (preview.__workbenchOptions || preview.__workbenchDocs) && !preview.__workbenchReady && !preview.__workbenchError) {
        var attempt = e.previewAttempt || 0;
        if (attempt < 300) {
          window.setTimeout(function () { frameLoaded({ currentTarget: loaded, previewAttempt: attempt + 1, sequence: sequence, warm: true }); }, 50);
          return;
        }
      }
    } catch (_) { /* Foreign-origin previews keep their existing load behavior. */ }
    if (loaded === pendingFrame) {
      window.requestAnimationFrame(function () {
        if (loaded !== pendingFrame || sequence !== loaded.wbLoadSequence) return;
        commitLayout();
        var previous = frame;
        previous.classList.remove('is-active');
        previous.setAttribute('aria-hidden', 'true');
        loaded.classList.add('is-active');
        loaded.removeAttribute('aria-hidden');
        previous.inert = true;
        loaded.inert = false;
        freezeFrameSize(previous);
        if (loaded.style) { loaded.style.width = ''; loaded.style.height = ''; }
        loaded.title = 'Preview';
        previous.title = 'Preview loading';
        frame = loaded;
        frameBuffer = previous;
        pendingFrame = null;
        frameReady = true;
        // A resize can start the spare frame before the initial frame has
        // loaded. Whichever wins must reveal the preview and enable prepare.
        artboardContent.classList.remove('is-loading');
        artboard.hidden = false;
        loaded.wbRestoring = false;
        var outgoing = previous.wbSessionKey && sessions.get(previous.wbSessionKey);
        if (previous !== loaded && outgoing && !outgoing.ready) sessions.remove(previous);
        sessions.activate(loaded);
        if (previous !== loaded && !sessions.has(previous)) disposeFrame(previous);
        reportRenderedStory();
        revealExample();
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      });
      return;
    }
    if (loaded === frame && !pendingFrame) {
      window.requestAnimationFrame(function () {
        if (loaded !== frame || pendingFrame || sequence !== loaded.wbLoadSequence) return;
        commitLayout();
        sessions.activate(loaded);
        loaded.inert = false;
        loaded.classList.add('is-active');
        loaded.removeAttribute('aria-hidden');
        frameReady = true;
        artboardContent.classList.remove('is-loading');
        artboard.hidden = false;
        reportRenderedStory();
        revealExample();
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      });
    }
  }

  frame.addEventListener('load', frameLoaded);
  frameBuffer.addEventListener('load', frameLoaded);
  if (!window.wbMultiArtboardHost && (location.protocol === 'http:' || location.protocol === 'https:')) {
    warmFrame(frame);
    warmFrame(frameBuffer);
  }
  window.addEventListener('pagehide', function () { sessions.close(); });

  /* ---------------------------------------------------------------- host */

  /* Embedded — in the editor, in a webview of its own — the workbench is half
     a tool: the page list stands in the editor's sidebar, where a file tree
     usually is, and this half is the canvas. Two messages hold the two halves
     together, both carrying the same selection the address bar would:

       in   wb-go    the page to show, as a hash
       out  wb-here  the page now showing, after any pick

     wb-here is what keeps the editor's list marked when the pick was made
     over here — a link followed in a live preview, or a copied URL. */
  var host = window.parent !== window ? window.parent : null;
  window.wbEmbedded = !!host;

  if (host) {
    shell.classList.add('is-embedded');
    window.wbKeys.relay(host);
  }

  /* The lens rides along for whoever wants it; the editor's list only marks
     pages and scenarios, and reads past it. */
  function tellHost(src, scenario, lens, pending) {
    if (window.wbAgentContext) window.wbAgentContext.report();
    if (!host) return;
    var message = { type: 'wb-here', src: src || null, scenario: scenario || null,
      lens: lens || null, pending: !!pending };
    /* The implicit artboard's iteration, when the design shows one. */
    if (src && src === current && !lens && currentIterations['']) message.iteration = currentIterations[''];
    host.postMessage(message, '*');
  }

  function tellHostRefreshFailed() {
    if (host) host.postMessage({ type: 'wb-refresh-failed' }, '*');
  }

  /* -------------------------------------------------------------- spaces */

  /* A workbench with more than one space names the one showing at the
     start of the breadcrumb, and, in a browser, swaps the sidebar's header
     for a switcher. Each space is its own server, so switching is going to
     another address: the editor does that for its tab, which it is asked to
     with wb-space; a browser asks this server for the space's address,
     starting its server if it has to, and goes there. One space shows
     neither, and a server that doesn't answer is one space. */
  function switchSpace(id) {
    if (host) {
      host.postMessage({ type: 'wb-space', id: id }, '*');
      return;
    }
    fetch('/_workbench/spaces/open', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: id }),
    })
      .then(function (res) { return res.json(); })
      .then(function (answer) {
        if (!answer || !answer.ok) throw new Error((answer && answer.error) || 'The space didn’t open.');
        location.href = answer.url;
      })
      .catch(function (error) {
        window.alert('Couldn’t open that space — ' + String(error.message || error));
      });
  }

  var crumbSwitcher = null;
  var sideSwitcher = null;

  /* Read on start and again whenever the config is: a space's name,
     color and icon come from its workbench.yaml. */
  function loadSpaces() {
    if (location.protocol !== 'http:' && location.protocol !== 'https:') return;
    fetch('/_workbench/spaces')
      .then(function (res) { return res.ok ? res.json() : null; })
      .then(function (answer) {
        var several = !!(answer && answer.ok && answer.spaces && answer.spaces.length > 1);
        document.getElementById('spaceCrumb').hidden = !several;
        document.getElementById('spaces').hidden = !several || !!host;
        document.getElementById('sidebarHead').hidden = several && !host;
        if (!several) return;
        if (!crumbSwitcher) {
          crumbSwitcher = document.getElementById('spaceCrumbSwitcher');
          crumbSwitcher.iconRenderer = window.wbIcon;
          crumbSwitcher.addEventListener('wb-space-pick', function (e) { switchSpace(e.detail.id); });
        }
        crumbSwitcher.spaces = answer.spaces;
        crumbSwitcher.currentId = answer.current;
        if (host) return;
        if (!sideSwitcher) {
          sideSwitcher = document.getElementById('spaces');
          sideSwitcher.iconRenderer = window.wbIcon;
          sideSwitcher.addEventListener('wb-space-pick', function (e) { switchSpace(e.detail.id); });
        }
        sideSwitcher.spaces = answer.spaces;
        sideSwitcher.currentId = answer.current;
      })
      .catch(function () { /* one space, as far as anyone can tell */ });
  }

  /* Links in a live preview ask the shell to navigate so they get the same
     double-buffered transition as sidebar choices.

     The page can't tell one of the space's own pages from anywhere else —
     it doesn't know where the project root is, and shouldn't have to — so it
     hands over the address it was about to visit and this decides. Anything
     outside the root, or inside it but not in the config, is not a page of
     this workbench and is ignored. */
  window.addEventListener('message', function (e) {
    storySwitchEvent(e);
    /* Storybook's own key channel is the only way a cross-origin story's
       keys reach the shell: zoom chords drive the canvas, editor chords go
       on to the editor. */
    if (view && view.lens && view.lens.kind === 'storybook') {
      var storyKey = window.wbKeys.storybook(e, frame, view.lens.url);
      if (storyKey && !(window.wbZoom && window.wbZoom.key(storyKey.event)) && host) {
        window.wbKeys.forward(storyKey, host);
      }
    }
    var data = e.data || {};

    /* The live preview is another iframe down. Its physical key event cannot
       bubble here, so carry the serialized event through the shell to the
       editor-tab webview. Accept it only from the active preview frame. */
    if (data.type === 'wb-keyboard') {
      if (!frameReady || e.source !== frame.contentWindow) return;
      window.wbKeys.forward(data, host);
      return;
    }

    if (data.type === 'wb-context-menu' && host && e.source === frame.contentWindow) {
      /* The page reports its own pixels; the canvas may be zoomed. */
      var rect = frame.getBoundingClientRect();
      var scale = rect.width / (frame.offsetWidth || rect.width || 1);
      host.postMessage({
        type: 'wb-context-menu', x: rect.left + Number(data.x || 0) * scale, y: rect.top + Number(data.y || 0) * scale,
        selection: String(data.selection || ''), editable: !!data.editable,
      }, '*');
      return;
    }

    if (data.type === 'wb-context-reset' && host && e.source === frame.contentWindow) {
      host.postMessage({ type: 'wb-context-reset' }, '*');
      return;
    }

    /* A pick from the editor's list. It travels as the hash the address bar
       would have carried, so an editor pick and a copied link are the same
       instruction, and a repeat of the one already showing still re-routes
       rather than silently doing nothing. Only the host can send it. */
    if (data.type === 'wb-go' && host && e.source === host) {
      var next = '#' + String(data.hash || '');
      if (location.hash === next) route();
      else location.hash = next;
      return;
    }

    if (data.type === 'wb-refresh' && host && e.source === host) {
      window.wbConfig.load(refreshConfig, function (error) {
        refuse(error);
        tellHostRefreshFailed();
      });
      return;
    }

    /* A TypeScript preview asking for another: a link its definition maps,
       or context.navigate. It names the preview by ID; the page is
       whichever one renders it. */
    if (data.type === 'workbench-preview' && data.event === 'navigate') {
      if (e.source !== frame.contentWindow || e.origin !== location.origin) return;
      var pages = (resolved && resolved.pages) || {};
      var target = Object.keys(pages).find(function (src) {
        return index[src] && pages[src].preview && pages[src].preview.id === data.preview;
      });
      if (!target) return;
      if (pageList) pageList.reveal(target);
      writeHash(target, scenarioOf(index[target], data.scenario), sizeKey());
      return;
    }

    if (data.type === 'workbench-preview' && data.docsPage) {
      if (e.source !== frame.contentWindow || e.origin !== location.origin || !view || !lensRules.isDocsLens(view.lens)) return;
      /* A link names the other page's Markdown; it opens that page's docs,
         in this docs lens when it has it. */
      var linked = data.event === 'docs-navigate' && Object.keys(index).filter(function (src) {
        return index[src].markdown === data.page;
      })[0];
      if (linked) {
        var linkLens = lensRules.docsLinkLens(index[linked], config.implementations, view.lens.key);
        if (linkLens) setLensPref(linkLens.key);
        if (pageList) pageList.reveal(linked);
        writeHash(linked, null, null, linkLens ? linkLens.key : null, data.example || null);
      }
      return;
    }

    if (e.source !== frame.contentWindow || data.type !== 'wb-preview-navigate') return;

    var href = String(data.href || '');
    if (href.indexOf(root) !== 0) return;

    var rest = href.slice(root.length);
    var query = rest.indexOf('?');
    var src = decodeURIComponent(query === -1 ? rest : rest.slice(0, query));
    if (!index[src]) return;

    if (pageList) pageList.reveal(src);
    writeHash(src, scenarioOf(index[src], data.scenario), sizeKey());
  });

  function setActions(on) {
    actionsOn = !!on;
    actionsToggle.checked = actionsOn;
    try {
      localStorage.setItem('canonic-workbench-actions', actionsOn ? 'on' : 'off');
    } catch (e) {
      /* file:// storage can be blocked; the toggle still works this session. */
    }
  }

  var RESIZABLE_DEFAULT_WIDTH = 1024;
  var RESIZABLE_DEFAULT_HEIGHT = 768;
  var resizableWidth = RESIZABLE_DEFAULT_WIDTH;
  var resizableHeight = RESIZABLE_DEFAULT_HEIGHT;

  /* The space's sizes, in order, as the server resolves them (see
     src/sizes/). Without the server, the defaults; without the sizes module
     either (a workbench opened off disk), Fit alone. */
  var FIT_ONLY = [{ key: 'fit', label: 'Fit', icon: 'minimize-2', button: true, kind: 'fit', width: null, height: null }];
  var spaceSizes = FIT_ONLY;
  /* The size the address asked for, until the page it names shows. */
  var requestedSize = null;
  /* The size stored from last time, tried when the address names none. */
  var rememberedSize = null;
  /* An artboard runtime sets its size directly; the page's sizes then only
     enable the buttons. */
  var pinnedSize = false;

  function loadSpaceSizes() {
    var sizes = window.wbSizes;
    spaceSizes = resolved && resolved.sizes && resolved.sizes.length ? resolved.sizes
      : sizes ? sizes.defaultSizes() : FIT_ONLY;
  }

  /* Page size support is independent of its source format. A Markdown page
     can also have a Storybook or URL lens. showSizes disables only docs lenses. */
  function sizesOf(item) {
    if (!item) return spaceSizes;
    var page = resolved && resolved.pages ? resolved.pages[item.src] : null;
    return window.wbSizes ? window.wbSizes.supported(spaceSizes, page) : spaceSizes;
  }

  function sizeByKey(key) {
    var shown = current && index[current] ? sizesOf(index[current]) : spaceSizes;
    return shown.concat(spaceSizes).find(function (size) { return size.key === key; }) || null;
  }

  /* The size the canvas is on, as the space describes it, for the annotation
     layer and the artboard runtime. */
  window.wbSize = function () {
    return sizeByKey(canvas.dataset.size);
  };

  function showFrameSize() {
    if (window.wbZoom) window.wbZoom.update();
  }

  /* The artboard at its CSS size, or with an axis left to zoom.js, which
     knows how much canvas there is and fills the flagged axes. */
  var appliedSize = '';

  function signature(size) {
    return [size.key, size.kind, size.width, size.height].join(' ');
  }

  function applySize(size) {
    var fixed = size.kind === 'fixed';
    appliedSize = signature(size);
    canvas.dataset.size = size.key;
    canvas.dataset.fillWidth = String(size.kind === 'fit' || (fixed && size.width === 'fill'));
    canvas.dataset.fillHeight = String(size.kind === 'fit' || (fixed && size.height === 'fill'));
    sizeSwitcher.current = size.key;
    if (size.kind === 'resizable') {
      artboard.style.width = resizableWidth + 'px';
      artboard.style.height = resizableHeight + 'px';
      showFrameSize();
    } else {
      artboard.style.width = fixed && typeof size.width === 'number' ? size.width + 'px' : '';
      artboard.style.height = fixed && typeof size.height === 'number' ? size.height + 'px' : '';
    }
    if (window.wbZoom) window.wbZoom.fit();
  }

  /* Every change of size fits the artboard to the canvas, the way a fresh
     artboard opens, and is remembered for next time by key. */
  function setSize(key) {
    var size = sizeByKey(key);
    if (!size) return;
    if (heldLayout) heldLayout.size = size.key;
    else applySize(size);
    rememberedSize = size.key;
    try {
      localStorage.setItem('canonic-workbench-size', size.key);
    } catch (e) {
      /* file:// storage can be blocked; the size switcher still works. */
    }
  }

  /* A docs lens takes the canvas's width: every size stays in the size
     switcher, off and unpressed, and the lens or page after it gets its size
     back. Any other lens shows the size the address asked for, the one the
     canvas is on, or the remembered one, if the page supports it; else its
     first size. */
  /* What the size switcher shows for a page in a lens: the space's sizes,
     then the page's own, those it supports enabled. A docs lens has none, so
     none is pressed. */
  function showSizes(item, lens) {
    var docs = lensRules.isDocsLens(lens);
    var page = item && resolved && resolved.pages ? resolved.pages[item.src] : null;
    sizeSwitcher.sizes = spaceSizes.concat((!docs && page && page.ownSizes) || []);
    sizeSwitcher.supported = docs ? [] : sizesOf(item).map(function (size) { return size.key; });
    sizeSwitcher.setAttribute('unsupported-reason', docs ? 'docs fill the canvas' : 'not supported by this page');
    if (docs) sizeSwitcher.current = '';
  }

  function updateSizeAvailability(item, lens) {
    var docs = lensRules.isDocsLens(lens);
    var supported = sizesOf(item);
    showSizes(item, lens);
    var asked = requestedSize;
    requestedSize = null;
    if (!item || docs || pinnedSize) return;
    var wanted = [asked, canvas.dataset.size, rememberedSize];
    var key = window.wbSizes ? window.wbSizes.choose(supported, asked, canvas.dataset.size, rememberedSize)
      : (wanted.find(function (candidate) { return candidate && supported.some(function (size) { return size.key === candidate; }); }) ||
        (supported[0] && supported[0].key));
    /* The same key can change dimensions when the config does. */
    var size = key && sizeByKey(key);
    if (size && (key !== canvas.dataset.size || signature(size) !== appliedSize)) setSize(key);
  }

  /* A size switcher change only resizes the frame already on the canvas.
     Keep the address shareable without sending the same page through the
     hash router, which would navigate its iframe and reset live state. */
  function chooseSize(key) {
    var item = current ? index[current] : null;
    if (item && !sizesOf(item).some(function (size) { return size.key === key; })) return;
    if (view && lensRules.isDocsLens(view.lens)) return;
    pinnedSize = false;
    setSize(key);
    if (window.wbMultiCanvas) window.wbMultiCanvas.resize(sizeByKey(key)).catch(function (error) { say(String(error.message || error)); });
    if (current) syncHash();
  }

  /* ----------------------------------------------------- sidebar resize */

  var SIDEBAR_MIN = 200;
  var SIDEBAR_MAX = 480;
  var SIDEBAR_DEFAULT = 248;

  function setSidebar(px) {
    var w = Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, px)));
    shell.style.setProperty('--wb-sidebar-w', w + 'px');
    return w;
  }

  function rememberSidebar(w) {
    try {
      localStorage.setItem('canonic-workbench-sidebar', String(w));
    } catch (e) {
      /* file:// storage can be blocked; the drag still works. */
    }
  }

  var dragging = false;

  /* Pointer capture keeps the drag alive over the iframe, which would
     otherwise swallow the move events. No preventDefault here — it would
     suppress the compatibility dblclick that resets the width. The
     is-resizing class handles text selection instead. */
  resizer.addEventListener('pointerdown', function (e) {
    dragging = true;
    resizer.setPointerCapture(e.pointerId);
    shell.classList.add('is-resizing');
  });

  resizer.addEventListener('pointermove', function (e) {
    if (dragging) setSidebar(e.clientX);
  });

  function endDrag(e) {
    if (!dragging) return;
    dragging = false;
    shell.classList.remove('is-resizing');
    if (e.type === 'pointerup') rememberSidebar(setSidebar(e.clientX));
  }

  resizer.addEventListener('pointerup', endDrag);
  /* A cancelled or lost drag must clear the class too, or the shell keeps a
     resize cursor and a dead iframe. */
  resizer.addEventListener('pointercancel', endDrag);
  resizer.addEventListener('lostpointercapture', endDrag);

  resizer.addEventListener('dblclick', function () {
    rememberSidebar(setSidebar(SIDEBAR_DEFAULT));
  });

  /* Arrow keys nudge it once the handle has focus. */
  resizer.addEventListener('keydown', function (e) {
    var step = e.shiftKey ? 32 : 8;
    var width = parseInt(getComputedStyle(shell).getPropertyValue('--wb-sidebar-w'), 10);
    if (e.key === 'ArrowLeft') rememberSidebar(setSidebar(width - step));
    else if (e.key === 'ArrowRight') rememberSidebar(setSidebar(width + step));
    else return;
    e.preventDefault();
  });

  /* ------------------------------------------------------------- wiring */

  sizeSwitcher.iconRenderer = window.wbIcon;
  actionsToggle.iconRenderer = window.wbIcon;
  sizeDialog.iconRenderer = window.wbIcon;
  sizesEditor.iconRenderer = window.wbIcon;
  sizeSwitcher.addEventListener('wb-size-pick', function (e) { chooseSize(e.detail.key); });
  sizeSwitcher.addEventListener('wb-size-custom', openSizeDialog);
  sizeSwitcher.addEventListener('wb-size-edit', openSizesEditor);

  /* Whether workbench.yaml lists the page itself, so it can hold sizes of its
     own; a discovered preview or a catalog page isn't there. */
  function listedInYaml(src) {
    function visit(items) {
      return (items || []).some(function (entry) { return entry.items ? visit(entry.items) : entry.src === src; });
    }
    return !!(resolved && (resolved.collections || []).some(function (collection) { return visit(collection.items); }));
  }

  function postSizes(body) {
    return fetch('/_workbench/sizes', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
      .then(function (response) {
        return response.json().catch(function () { return {}; }).then(function (result) {
          if (!response.ok || !result.ok) throw new Error(result.error || 'Could not save workbench.yaml.');
          return result;
        });
      });
  }

  /* After a size change the canvas reads the config again. `key`, when
     given, is the size the address asks for next, so a new size shows. */
  function reloadSizes(key, message) {
    if (key && current) {
      var have = window.wbAddress.parse(location.hash);
      have.size = key;
      history.replaceState(null, '', location.pathname + location.search + '#' + window.wbAddress.write(have));
    }
    window.wbConfig.load(function (loaded) {
      refreshConfig(loaded);
      say(message);
    }, refuse);
  }

  function openSizeDialog() {
    var item = current ? index[current] : null;
    var docs = lensRules.isMarkdownPage(item);
    var page = item && resolved && resolved.pages ? resolved.pages[item.src] : null;
    var listed = !!item && !docs && listedInYaml(item.src);
    sizeDialog.allowPageOnly = listed;
    sizeDialog.notice = docs ? 'A Markdown page has no size; the new size is added to the space.'
      : page && page.preview && page.preview.sizes && !listed
        ? 'This page’s preview definition lists its sizes; add the new size’s key there to use it on this page.' : '';
    sizeDialog.open = true;
  }

  sizeDialog.addEventListener('wb-size-add', function (e) {
    var item = current ? index[current] : null;
    var docs = lensRules.isMarkdownPage(item);
    var size = Object.assign({}, e.detail, { page: item && !docs && listedInYaml(item.src) ? item.src : null });
    sizeDialog.error = '';
    sizeDialog.pending = true;
    postSizes({ action: 'add', size: size }).then(function (result) {
      sizeDialog.pending = false;
      sizeDialog.open = false;
      reloadSizes(docs ? null : result.key, 'Added ' + e.detail.name.trim() + ' to workbench.yaml.');
    }, function (error) {
      sizeDialog.pending = false;
      sizeDialog.error = String(error.message || error);
    });
  });
  sizeDialog.addEventListener('wb-size-dialog-close', function () { sizeDialog.open = false; });

  function openSizesEditor() {
    sizesEditor.sizes = (resolved && resolved.sizes) || spaceSizes;
    sizesEditor.open = true;
  }

  sizesEditor.addEventListener('wb-sizes-save', function (e) {
    sizesEditor.error = '';
    sizesEditor.pending = true;
    postSizes({ action: 'update', sizes: e.detail.sizes }).then(function () {
      sizesEditor.pending = false;
      sizesEditor.open = false;
      reloadSizes(null, 'Saved the sizes to workbench.yaml.');
    }, function (error) {
      sizesEditor.pending = false;
      sizesEditor.error = String(error.message || error);
    });
  });
  sizesEditor.addEventListener('wb-sizes-close', function () { sizesEditor.open = false; });

  /* Chrome-style rails sit outside the preview. Side rails change width, the
     bottom changes height, and either bottom corner changes both. Pointer
     travel is screen pixels, so it is divided by the zoom; a left-edge drag
     moves the frame over by what it grew, so the right edge stays put. */
  var artboardDrag = null;

  function rememberFrameSize() {
    try {
      localStorage.setItem('canonic-workbench-resizable-width', String(resizableWidth));
      localStorage.setItem('canonic-workbench-resizable-height', String(resizableHeight));
    } catch (e) {
      /* file:// storage can be blocked; resizing still works this session. */
    }
  }

  Array.prototype.forEach.call(document.querySelectorAll('.wb-artboard-resize'), function (handle) {
    handle.addEventListener('pointerdown', function (e) {
      if (canvas.dataset.size !== 'resizable') return;
      if (window.wbZoom) window.wbZoom.hold();
      artboardDrag = {
        edge: handle.dataset.resize,
        x: e.clientX,
        y: e.clientY,
        width: artboard.offsetWidth,
        height: artboard.offsetHeight,
        scale: window.wbZoom ? window.wbZoom.scale() : 1,
        pan: window.wbZoom ? window.wbZoom.pan() : null,
      };
      handle.setPointerCapture(e.pointerId);
      shell.classList.add('is-frame-resizing');
      e.preventDefault();
    });

    handle.addEventListener('pointermove', function (e) {
      if (!artboardDrag) return;
      var left = artboardDrag.edge.indexOf('left') > -1;
      var horizontal = (left ? artboardDrag.x - e.clientX : e.clientX - artboardDrag.x) / artboardDrag.scale;
      if (artboardDrag.edge !== 'bottom') {
        resizableWidth = Math.max(1, Math.round(artboardDrag.width + horizontal));
        artboard.style.width = resizableWidth + 'px';
        if (left && artboardDrag.pan) {
          window.wbZoom.panTo(artboardDrag.pan.x - (resizableWidth - artboardDrag.width) * artboardDrag.scale, artboardDrag.pan.y);
        }
      }
      if (artboardDrag.edge.indexOf('bottom') > -1) {
        resizableHeight = Math.max(1, Math.round(artboardDrag.height + (e.clientY - artboardDrag.y) / artboardDrag.scale));
        artboard.style.height = resizableHeight + 'px';
      }
      showFrameSize();
    });

    function endFrameDrag() {
      if (!artboardDrag) return;
      artboardDrag = null;
      shell.classList.remove('is-frame-resizing');
      rememberFrameSize();
    }

    handle.addEventListener('pointerup', endFrameDrag);
    handle.addEventListener('pointercancel', endFrameDrag);
    handle.addEventListener('lostpointercapture', endFrameDrag);
  });

  /* Reloading is what applies it — the flag is read as the page parses. */
  actionsToggle.addEventListener('wb-actions-toggle', function (e) {
    setActions(e.detail.checked);
    if (current) show(current, currentScenario, undefined, currentIterations);
  });

  reload.addEventListener('click', function () {
    if (!current) return;
    if (view && window.wbManifest.streamed(view.lens)) {
      window.wbSimulator.reload();
      return;
    }
    if (!frame.getAttribute('src')) {
      show(current, currentScenario, undefined, currentIterations);
      return;
    }
    loadPreview(view && view.url ? view.url : frame.src, true);
  });

  scenarioButton.addEventListener('click', function () {
    openSources(false);
    openScenarios(scenarioMenu.hidden);
  });

  sourceButton.addEventListener('click', function () {
    openScenarios(false);
    openSources(sourceMenu.hidden);
  });

  document.addEventListener('pointerdown', function (e) {
    if (!scenarioMenu.hidden && !e.target.closest('.wb-scenarios-control')) openScenarios(false);
    if (!sourceMenu.hidden && !e.target.closest('.wb-sources')) openSources(false);
  });

  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Escape') return;
    openScenarios(false);
    openSources(false);
  });

  function route() {
    if (window.wbArtboardHeld) { syncHash(); return; }
    var target = parseHash();
    requestedSize = target.size || null;
    /* A lens in the address is an instruction — a copied link says which —
       and one that isn't declared here means the design. No lens keeps the
       one the top bar is on. */
    var item = index[target.src];
    var known = config.implementations[target.lens] || lensRules.hasLens(item, config.implementations, target.lens);
    if (target.lens !== null) setLensPref(known ? target.lens : null);
    show(target.src, target.scenario, target.example, target.iterations);
  }

  window.addEventListener('hashchange', route);

  /* ⌘A is a text command. In a field or a note it selects the text there; over
     the chrome, where nothing is text, it would select the whole shell and the
     frame with it, so it does nothing. Pages inside the frame are their own
     documents and keep the browser's select-all. */
  document.addEventListener('keydown', function (e) {
    if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== 'a') return;
    if (!window.wbKeys.editingTarget(e)) e.preventDefault();
  }, true);

  /* ⌘K / ctrl-K jumps to the filter, like every other tool — but only where
     there is one. Embedded, the filter is in the editor's sidebar, and this
     half has no business swallowing the chord. */
  document.addEventListener('keydown', function (e) {
    if (host) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      search.focus();
      search.select();
    }
  });

  /* How the top bar was left. These are the tool's preferences rather than
     the space's, so they are shared by every space the workbench opens —
     the sidebar you sized once stays that size. */
  function restore() {
    var saved = {};
    try {
      saved.size = localStorage.getItem('canonic-workbench-size');
      saved.sidebar = localStorage.getItem('canonic-workbench-sidebar');
      saved.actions = localStorage.getItem('canonic-workbench-actions');
      saved.resizableWidth = localStorage.getItem('canonic-workbench-resizable-width');
      saved.resizableHeight = localStorage.getItem('canonic-workbench-resizable-height');
      saved.lens = localStorage.getItem('canonic-workbench-lens');
    } catch (e) {
      /* no storage; fall back to the defaults below. */
    }
    /* The lens is checked against the config where it's used, so a name from
       another space's top bar just means each page's default lens here. */
    lensPref = saved.lens || null;
    if (saved.resizableWidth) resizableWidth = parseInt(saved.resizableWidth, 10) || RESIZABLE_DEFAULT_WIDTH;
    if (saved.resizableHeight) resizableHeight = parseInt(saved.resizableHeight, 10) || RESIZABLE_DEFAULT_HEIGHT;
    /* Checked against each page's sizes when it shows, so a key from another
       space just means that page's first size here. */
    rememberedSize = saved.size || null;
    if (saved.sidebar) setSidebar(parseInt(saved.sidebar, 10));
    /* Only an explicit "on" turns them on — anything else, including nothing
       stored, lands on off. Must run before route(), which builds the src. */
    setActions(saved.actions === 'on');
  }

  /* -------------------------------------------------------------- boot */

  /* Nothing above runs until the space's config is in: the sidebar is the
     config, and there is no useful half-built version of it to show while
     waiting. */
  /* What the server makes of the config on its machine: absolute paths for
     the source menu and the handoff. Served by anything else — or off disk —
     there is no answer, and the two do without. */
  function loadResolved(then) {
    var request;
    try {
      request = fetch('/_workbench/config');
    } catch (e) {
      request = Promise.reject(e);
    }
    request
      .then(function (res) {
        return res.json();
      })
      .then(
        function (result) {
          resolved = result && result.ok ? result : null;
          exportSpace.hidden = !resolved;
          configureSpace.hidden = !resolved;
        },
        function () {
          resolved = null;
          exportSpace.hidden = true;
          configureSpace.hidden = true;
        }
      )
      .then(function () { then(resolved); });
  }

  function applyConfig(loaded, first) {
    loadResolved(function () {
      blank.classList.remove('is-loading');
      config = loaded;
      /* The server names an unnamed space after its root folder, as the space
         switcher does; the browser can't always see that folder. */
      if (resolved && resolved.name) config.name = resolved.name;
      if (resolved && resolved.implementations) config.implementations = resolved.implementations;
      if (resolved) resolveWorkbenchLenses(config.collections, resolved.collections, config.implementations);
      showProblems(resolved ? resolved.problems : config.problems);
      loadSpaceSizes();
      /* Custom size… and Edit sizes… write through the server. */
      sizeSwitcher.allowEdit = !!resolved;
      showSizes(current ? index[current] : null, view && view.lens);
      collections = window.wbManifest.mergeCollections(config.collections, (resolved && resolved.catalogCollections) || []);
      index = window.wbPageList.index(collections);
      storyCache = {};

      document.title = config.name;
      spaceName.textContent = config.name;
      loadSpaces();

      if (first) restore();

      /* The list is built here and picked from there: a row hands back the
         page and the scenario it stands for, and the shell turns that into
         the hash, which is the one place the selection lives. */
      if (pageList) {
        pageList.update(collections);
      } else {
        pageList = window.wbPageList.create({
          search: search,
          collectionList: document.getElementById('collections'),
          collectionsBlock: document.getElementById('collectionsBlock'),
          title: document.getElementById('collectionName'),
          list: document.getElementById('pageList'),
          collections: collections,
          onPick: function (src, scenario) {
            writeHash(src, scenario, sizeKey());
          },
        });
      }

      /* A newly opened space has no destination when switched to from the
         sidebar or breadcrumb. Pick from the complete, merged page list;
         explicit links and later config refreshes keep their destination. */
      if (first && !parseHash().src) {
        var initial = window.wbPageList.firstPage(collections);
        if (initial) history.replaceState(null, '', '#' + window.wbAddress.write(initial));
      }
      route();
    });
  }

  /* The space's problems above the page list in a browser window, as the VS
     Code sidebar shows them: the server's list, which every reader agrees on,
     or this reader's own when there is no server. The editor tab has no
     sidebar here; the VS Code sidebar beside it shows the same list. */
  function showProblems(problems) {
    var block = document.getElementById('problems');
    if (!block) return;
    problems = (problems || []).filter(Boolean);
    block.hidden = !problems.length;
    block.textContent = problems.join('\n');
  }

  function eachPage(list, visit) {
    (list || []).forEach(function (collection) {
      (collection.items || []).forEach(function (entry) {
        ('group' in entry ? entry.items || [] : [entry]).forEach(visit);
      });
    });
  }

  /* A page's Workbench lens names a preview by ID; the server resolves it to
     the preview's definition path and its scenarios' paths (`{ path,
     scenarios }`) in its own copy of the collections. The page takes that
     resolved reference, so its lens URL, scenario menu and references read
     the same address the renderer loads. */
  function resolveWorkbenchLenses(list, served, impls) {
    var refs = {};
    eachPage(served, function (page) { if (page.src && page.implementations) refs[page.src] = page.implementations; });
    eachPage(list, function (page) {
      Object.keys(page.implementations || {}).forEach(function (key) {
        var ref = page.implementations[key];
        var answer = refs[page.src] && refs[page.src][key];
        if (impls && impls[key] && impls[key].kind === 'workbench' && ref && !ref.path && answer && answer.path) {
          page.implementations[key] = answer;
        }
      });
    });
  }

  /* Size and docs bootstraps load their capability rules after this script;
     it waits for
     one, or for it to fail to load. */
  function whenModule(present, event, script, then) {
    if (present() || location.protocol === 'file:') { then(); return; }
    var done = false;
    function go() { if (!done) { done = true; then(); } }
    window.addEventListener(event, go, { once: true });
    var tag = document.querySelector('script[src$="' + script + '"]');
    if (tag) tag.addEventListener('error', go, { once: true });
    else go();
  }

  function whenRules(then) {
    whenModule(function () { return !!window.wbSizes; }, 'wb-sizes-ready', 'bootstrap/sizes.js', function () {
      whenModule(function () { return !!window.wbDocsLenses; }, 'wb-docs-ready', 'bootstrap/docs.js', function () {
        lensRules = window.wbDocsLenses || NO_DOCS;
        if (window.wbMultiArtboardHost) whenModule(function () { return !!window.wbMultiCanvas; }, 'wb-multi-ready', 'bootstrap/workbench.js', then);
        else then();
      });
    });
  }

  function start(loaded) {
    whenRules(function () { applyConfig(loaded, true); });
  }

  function refreshConfig(loaded) {
    whenRules(function () {
      cancelStorySwitch();
      cancelPendingFrame();
      sessions.clearInactive();
      sessions.forget(frame);
      applyConfig(loaded, false);
      if (window.wbMultiCanvas) window.wbMultiCanvas.refresh().catch(function (error) { say(String(error.message || error)); });
    });
  }

  /* No config, no workbench — say which file and why, where the pages would
     have been. The top bar stays; it just has nothing to act on. */
  function refuse(error) {
    if (window.wbMultiCanvas) window.wbMultiCanvas.fail(error);
    blank.classList.remove('is-loading');
    blank.textContent = String(error.message || error);
    blank.hidden = false;
    console.error('[workbench] ' + String(error.message || error));
    /* A space whose config is broken can still be switched away from. */
    loadSpaces();
  }

  window.wbConfig.load(start, refuse);
})();
