/* Workbench shell
   ---------------
   Builds the sidebar from the project's workbench.yaml — read by
   config.js — and loads the picked file into the canvas iframe.

   Nothing in here is about any one project. What it knows about the project
   arrives as the config: a name, and sections of screens. Two constraints
   shape the rest:
   - the config is the index, except for a Storybook catalog it explicitly
     imports. A screen in neither isn't in the sidebar;
   - the iframe can be a foreign origin (a workbench opened off file://), so
     nothing here reads into it. Reload re-assigns src instead of calling into
     contentWindow.

   Paths in the config are relative to the project root, which is where the
   author thinks in — `pages/sign-in.html` — while this document is served out
   of the extension, beside no project at all. config.js settles the root;
   every src is resolved against it on the way to the frame, and stays
   root-relative everywhere else, including the hash.

   The selection lives in the URL hash, so a reload or a copied link lands on
   the same screen, in the same state, at the same width, through the same
   lens: #pages/sign-in.html:error@393~staging — address.js reads and writes
   it. The default state and the design lens are left out, so a page without
   states — or on the version you land on — reads as it always did.

   States themselves are the page's business, not the shell's: the id travels
   in the frame's URL and states.js applies it inside.

   A lens is an implementation of the screen in the design's place: a story
   in a Storybook, the page on a dev server, the page on staging. The config
   declares them (manifest.js), the toolbar switches them, and the choice
   sticks across screens the way the width does — a screen without that lens
   shows its design. Through a lens the frame is somebody else's origin, so
   nothing here reads into it; what still works is what the address carries.
*/

(function () {
  var config = null;
  var groups = [];         /* the config's sections */
  var index = {};          /* src -> item */
  var root = window.wbConfig.root; /* the project root, as a URL */
  var nav = null;          /* the screen list, built by nav.js once the config is in */
  var resolved = null;     /* the config as the server resolves it on its machine, or null */

  /* The markup layer needs the same lookup for the handoff prompt, and has no
     business re-walking the config to get it. */
  window.wbItem = function (src) {
    return index[src] || null;
  };

  var shell = document.querySelector('.wb');
  var resizer = document.getElementById('resizer');
  var search = document.getElementById('search');
  var stage = document.getElementById('stage');
  var frame = document.getElementById('frame');
  var frameBuffer = document.getElementById('frameBuffer');
  var pendingFrame = null;
  var frameReady = false;
  var frameShell = document.getElementById('frameShell');
  var frameWrap = document.getElementById('frameWrap');
  var frameSize = document.getElementById('frameSize');
  var blank = document.getElementById('blank');
  var openLink = document.getElementById('open');
  var reload = document.getElementById('reload');
  var actionsToggle = document.getElementById('actionsToggle');
  var lensesBox = document.getElementById('lenses');
  var storyControl = document.getElementById('storyControl');
  var storyButton = document.getElementById('storyButton');
  var storyName = document.getElementById('storyName');
  var storyMenu = document.getElementById('storyMenu');
  var sourceControl = document.getElementById('sourceControl');
  var sourceButton = document.getElementById('openSource');
  var sourceMenu = document.getElementById('sourceMenu');
  var configureProject = document.getElementById('configureProject');
  var exportProject = document.getElementById('exportProject');
  var exportProgress = document.getElementById('exportProgress');
  var exportProgressBar = document.getElementById('exportProgressBar');
  var exportProgressText = document.getElementById('exportProgressText');
  var widthButtons = Array.prototype.slice.call(
    document.querySelectorAll('.wb-width')
  );

  var BLANK_TEXT = blank.textContent;
  var ACTIONS_TITLE = actionsToggle.title;

  var current = null;      /* src of the picked screen */
  var currentState = null; /* its state id, or null for the default one */
  var lensPref = null;     /* the lens the toolbar was left on, or null for the design */
  var view = null;         /* what the canvas is showing — see wbView below */
  var widths = widthButtons.map(function (b) {
    return b.dataset.width;
  });

  /* What the canvas is showing, for the markup layer: it names screenshots
     and tells the agent what it is looking at, and both want the same
     answers. Null while nothing is picked. */
  window.wbView = function () {
    return view;
  };

  /* The shell has no toast of its own; the markup layer's is the one. */
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
          : 'Packaging source files…';
      }
      if (status.status === 'failed') throw new Error(status.error || 'Export failed.');
      if (status.status === 'complete') return status;
      return new Promise(function (resolve) { setTimeout(resolve, 250); }).then(function () { return pollExport(job, startedAt); });
    });
  }

  function downloadExport(job) {
    exportProgressText.textContent = job.parts && job.parts.length > 1 ? 'Downloading ZIP bundle…' : 'Downloading ZIP…';
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

  exportProject.addEventListener('click', function () {
    if (exportProject.disabled) return;
    exportProject.disabled = true;
    exportProject.setAttribute('aria-busy', 'true');
    exportProgress.hidden = false;
    exportProgressBar.max = 1;
    exportProgressBar.value = 0;
    exportProgressText.textContent = 'Finding reference views…';
    window.dispatchEvent(new CustomEvent('wb-export-start'));
    say('Preparing design-system export…');
    var finished = null;
    var startedAt = Date.now();
    fetch('/_workbench/export', { method: 'POST' })
      .then(exportJson)
      .then(function (job) { return pollExport(job, startedAt); })
      .then(function (status) { finished = status; exportProgressText.textContent = 'Packaging download…'; return downloadExport(status); })
      .then(function (name) {
        say('Downloaded ' + name + (finished && finished.warnings ? ' with ' + finished.warnings + ' screenshot warning(s).' : '.'));
      })
      .catch(function (error) { say(String(error.message || error)); })
      .then(function () {
        exportProgress.hidden = true;
        exportProject.disabled = false;
        exportProject.removeAttribute('aria-busy');
        window.dispatchEvent(new CustomEvent('wb-export-end'));
      });
  });

  /* Anything marked data-icon gets its Lucide glyph injected once. */
  Array.prototype.forEach.call(document.querySelectorAll('[data-icon]'), function (el) {
    el.appendChild(window.wbIcon(el.dataset.icon));
  });

  window.wbConfigEditor.create({
    button: configureProject,
    dialog: document.getElementById('configDialog'),
    body: document.getElementById('configBody'),
    status: document.getElementById('configStatus'),
    close: document.getElementById('configClose'),
    addSection: document.getElementById('configAddSection'),
    save: document.getElementById('configSave'),
    onSaved: function () {
      window.wbConfig.load(function (loaded) {
        refreshConfig(loaded);
        say('Saved workbench.yaml.');
      }, refuse);
    },
  });

  /* "#pages/sign-in.html:error@393~staging" -> { src, state, width, lens }.
     address.js does the reading; a width that isn't one of the buttons is
     dropped here, the state and the lens are checked against the screen and
     the config when they're used. */
  function parseHash() {
    var target = window.wbAddress.parse(location.hash);
    if (target.width !== null && widths.indexOf(target.width) === -1) target.width = null;
    return target;
  }

  /* The default state and the design lens are addressed by leaving them out,
     so the hash for a screen with states and the hash for one without look
     the same. A pick that names no lens keeps the one the toolbar is on. */
  function writeHash(src, state, width, lens) {
    location.hash = window.wbAddress.write({ src: src, state: state, width: width, lens: lens || null });
  }

  /* Once the canvas has settled on a screen, the address says exactly what's
     showing — the lens that took effect, the story that was picked — so a
     copied link means this and not what was typed. Replaced rather than
     assigned: the canvas is already there, and a hashchange would only send
     it there again. */
  function syncHash() {
    var have = window.wbAddress.parse(location.hash);
    var lens = view && view.lens ? view.lens.key : null;
    if (have.src === current && have.state === currentState && have.width === stage.dataset.width && have.lens === lens) return;
    var want = window.wbAddress.write({ src: current, state: currentState, width: stage.dataset.width, lens: lens });
    history.replaceState(null, '', location.pathname + location.search + (want ? '#' + want : ''));
  }

  window.wbTarget = parseHash;

  /* A single state is a page with a note attached, not a group — the list
     only splits a screen open when there is a choice to make. Same rule the
     panel folds by, so it comes from the same place. */
  var statesOf = window.wbNav.states;

  /* An id the item actually declares, or null for its default — which is
     what an unknown one falls back to rather than a blank frame. */
  function stateOf(item, id) {
    var states = statesOf(item);
    if (!states || !id) return null;
    for (var i = 1; i < states.length; i++) {
      if (states[i].id === id) return id;
    }
    return null;
  }

  function stateLabel(item, id) {
    var states = statesOf(item) || [];
    for (var i = 0; i < states.length; i++) {
      if (states[i].id === (id || 'default')) return states[i].label;
    }
    return null;
  }

  /* ------------------------------------------------------------ lenses */

  /* Which lenses a screen has, in the order the config declared them. */
  function lensesOf(item) {
    if (!item || !item.implementations) return [];
    return Object.keys(config.implementations).filter(function (key) {
      return !!item.implementations[key];
    });
  }

  /* The lens the toolbar is on, if this screen has it; else the design. */
  function effectiveLens(item) {
    if (item && item.implementationOnly) return config.implementations[item.implementationOnly] || null;
    if (!lensPref || !item.implementations || !item.implementations[lensPref]) return null;
    return config.implementations[lensPref] || null;
  }

  function setLensPref(key) {
    lensPref = key || null;
    try {
      localStorage.setItem('canonic-workbench-lens', lensPref || '');
    } catch (e) {
      /* file:// storage can be blocked; the lens still holds this session. */
    }
  }

  /* The address the frame loads: the design with its flags, or the screen's
     path on the implementation. Stories are asked for first — see loadStory. */
  function viewUrl(item, lens, state) {
    if (!lens) return withFlags(item.src, state);
    return window.wbLenses.url(lens, item.implementations[lens.key], state);
  }

  /* The page into the iframe, the usual way. */
  function showFrame(url) {
    window.wbSimulator.hide();
    blank.hidden = true;
    /* With no current document, keep the whole canvas out of sight until the
       first styled page is ready. Later navigation leaves it visible while
       the spare iframe loads behind it. */
    frameShell.hidden = !frame.getAttribute('src');
    loadPreview(url);
    openLink.href = url;
  }

  var pendingStorySwitch = null;
  var renderedStory = null;
  var reportedStorySeq = null;

  function cancelStorySwitch() {
    if (!pendingStorySwitch) return;
    clearTimeout(pendingStorySwitch.timer);
    pendingStorySwitch = null;
  }

  function fallBackStorySwitch(switching) {
    if (pendingStorySwitch !== switching || switching.seq !== showSeq) return;
    pendingStorySwitch = null;
    showFrame(switching.url);
  }

  function reportRenderedStory() {
    if (!renderedStory || reportedStorySeq === showSeq || !frameReady || pendingFrame ||
        renderedStory.frame !== frame || renderedStory.seq !== showSeq || !view ||
        !view.story || view.story.id !== renderedStory.id) return;
    reportedStorySeq = showSeq;
    tellHost(current, currentState, view.lens.key);
  }

  /* Keep Storybook's loaded preview warm. The channel does not promise that
     the preview accepted a message, so navigate only if no acknowledgement
     arrives. Once accepted, wait for its render event before telling capture
     that the new story is ready. */
  function reuseStoryFrame(previous, lens, storyId, seq) {
    if (!frameReady || pendingFrame || !previous || !previous.lens ||
        previous.lens.kind !== 'storybook' || previous.lens.url !== lens.url) return false;
    if (!window.wbLenses.selectStory(frame, lens, storyId)) return false;
    var switching = { frame: frame, lens: lens, id: storyId, seq: seq,
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
      reportRenderedStory();
      if (!wasReported && reportedStorySeq === showSeq && target === frame && !pendingFrame) {
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      }
    }
  }

  /* Where the code is, for the handoff: what the story says about itself,
     then every pointer the config resolved for this screen on this machine. */
  function codeFor(item, story) {
    var out = [];
    function add(file) {
      if (file && out.indexOf(file) === -1) out.push(file);
    }
    ((story && story.code) || []).forEach(add);
    var info = resolved && resolved.screens && resolved.screens[item.src];
    ((info && info.code) || []).forEach(function (entry) {
      if (entry.exists) add(entry.path);
    });
    return out;
  }

  function drawLenses(item, lens) {
    lensesBox.innerHTML = '';
    var keys = lensesOf(item);
    lensesBox.hidden = !keys.length;
    if (!keys.length) return;
    (item.implementationOnly ? keys : [null].concat(keys)).forEach(function (key) {
      var impl = key ? config.implementations[key] : null;
      var button = document.createElement('button');
      button.className = 'wb-lens';
      button.type = 'button';
      button.dataset.lens = key || '';
      button.textContent = impl ? impl.label : 'Design';
      button.title = impl ? 'This screen as ' + impl.label + ' has it' : 'This screen as designed';
      button.setAttribute('aria-pressed', String((lens ? lens.key : null) === key));
      button.addEventListener('click', function () {
        setLensPref(key);
        writeHash(current, currentState, stage.dataset.width, key);
      });
      lensesBox.appendChild(button);
    });
  }

  /* Through a lens the page is served by somebody else and actions.js isn't
     in it, so the switch has nothing to switch: it shows on and stays put. */
  function setActionsAvailability(lens) {
    if (lens) {
      actionsToggle.disabled = true;
      actionsToggle.setAttribute('aria-checked', 'true');
      actionsToggle.title = 'Actions — always on here: ' + lens.label +
        ' serves this page, not the workbench, so nothing inside it is switched off';
      return;
    }
    actionsToggle.disabled = false;
    actionsToggle.setAttribute('aria-checked', String(actionsOn));
    actionsToggle.title = ACTIONS_TITLE;
  }

  /* ----------------------------------------------------------- stories */

  /* A Storybook lens shows one story at a time, and which stories a title
     has is the Storybook's to say — the server asks its index. Answers are
     kept for the session; a refresh forgets them. */
  var storyCache = {};

  function stories(lens, title) {
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
          throw new Error('Stories need the workbench server — the Canonic extension runs one, and so does node server.js.');
        }
        throw error;
      });
    storyCache[key] = request;
    return request;
  }

  function openStories(open) {
    storyMenu.hidden = !open;
    storyButton.setAttribute('aria-expanded', String(open));
  }

  function drawStories(list, picked, lens) {
    storyMenu.innerHTML = '';
    openStories(false);
    storyControl.hidden = !list;
    if (!list) return;
    storyName.textContent = picked.name;
    storyButton.title = 'Story: ' + picked.name + ' — pick another';
    list.forEach(function (story) {
      var row = document.createElement('button');
      row.className = 'wb-menu-row';
      row.type = 'button';
      row.setAttribute('role', 'menuitem');
      row.setAttribute('aria-current', String(story.id === picked.id));
      row.textContent = story.name;
      row.addEventListener('click', function () {
        openStories(false);
        writeHash(current, story.state, stage.dataset.width, lens.key);
      });
      storyMenu.appendChild(row);
    });
  }

  /* The story's turn of show(): the title's stories are asked for, the one
     the address names — or the first — goes in the frame. `seq` is show()'s
     count; an answer for a screen that's no longer up is dropped. */
  function loadStory(item, lens, state, seq, previousView) {
    var ref = item.implementations[lens.key];
    var slow = setTimeout(function () {
      if (seq === showSeq) say('Asking ' + lens.label + '…');
    }, 400);

    stories(lens, ref.title).then(
      function (list) {
        clearTimeout(slow);
        if (seq !== showSeq) return;
        var picked = window.wbLenses.pick(list, state);
        currentState = picked.state;
        if (item.implementationOnly && nav) nav.setCurrent(current, currentState, item.group);
        view.story = picked;
        view.stateLabel = picked.name;
        view.url = window.wbLenses.storyUrl(lens, picked.id);
        view.code = codeFor(item, picked);
        if (!reuseStoryFrame(previousView, lens, picked.id, seq)) showFrame(view.url);
        openLink.href = window.wbLenses.storyOpenUrl(lens, picked.id);
        drawStories(list, picked, lens);
        setTitle(item, picked.name, lens);
        syncHash();
      },
      function (error) {
        clearTimeout(slow);
        if (seq !== showSeq) return;
        currentState = null;
        frameShell.hidden = true;
        frameReady = false;
        frame.removeAttribute('src');
        frameBuffer.removeAttribute('src');
        openLink.removeAttribute('href');
        blank.textContent = String(error.message || error);
        blank.hidden = false;
        tellHost(null, null, null);
        setTitle(item, null, lens);
        syncHash();
      }
    );
  }

  /* ------------------------------------------------------------ source */

  /* The screen's source, as the server resolved it on its machine: the
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
    var info = item && resolved && resolved.screens && resolved.screens[item.src];
    sourceMenu.innerHTML = '';
    openSources(false);
    sourceControl.hidden = !info;
    if (!info) return;
    if (info.design) {
      sourceRow('Design', item.src, true, info.design, function () {
        openSource(item.src, null);
      });
    }
    info.code.forEach(function (entry) {
      var impl = config.implementations[entry.implementation];
      var label = impl ? impl.label : entry.implementation;
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

  function setTitle(item, stateLabelText, lens) {
    /* The state and the lens are part of what you're looking at, so they
       belong in the tab title next to the screen's name. */
    document.title = [item.label, stateLabelText, lens ? lens.label : null, config.name]
      .filter(Boolean)
      .join(' · ');
  }

  function show(src, state) {
    var item = index[src];
    var seq = ++showSeq;
    var previousView = view;
    cancelStorySwitch();
    renderedStory = null;
    current = item ? src : null;
    var lens = item ? effectiveLens(item) : null;
    var story = !!lens && lens.kind === 'storybook';
    /* Under a Storybook lens the state slot carries the story; it's checked
       against the title's stories once they're in. */
    currentState = !item ? null : story ? state || null : stateOf(item, state);
    updateWidthAvailability(item);

    /* The list marks the row and follows it into its section, wherever the
       pick came from — its own rows, a link in the preview, or the editor.
       Through a lens that doesn't answer to the screen's states, the row
       marked is the screen itself. */
    var mapped = item.implementationOnly || !lens || (!story && !!item.implementations[lens.key].states);
    var reported = mapped ? currentState : null;
    if (nav) nav.setCurrent(current, reported, item && item.group);
    tellHost(current, reported, lens ? lens.key : null, story);

    view = null;
    drawLenses(item, lens);
    drawStories(null);
    drawSources(item);
    setActionsAvailability(lens);

    if (!item) {
      window.wbSimulator.hide();
      frameShell.hidden = true;
      frameReady = false;
      frame.removeAttribute('src');
      frameBuffer.removeAttribute('src');
      blank.textContent = BLANK_TEXT;
      blank.hidden = false;
      openLink.removeAttribute('href');
      /* The picked item names itself in the sidebar and the tab title —
         the top bar stays for tools. */
      document.title = config.name;
      return;
    }

    view = {
      src: src,
      item: item,
      lens: lens,
      state: story ? null : currentState,
      stateLabel: null,
      story: null,
      url: null,
      code: codeFor(item, null),
    };

    if (story) {
      loadStory(item, lens, state, seq, previousView);
      return;
    }

    if (lens && lens.kind === 'ios-simulator') {
      var simulatorRef = item.implementations[lens.key];
      currentState = null;
      view.url = null;
      frameReady = false;
      pendingFrame = null;
      frame.removeAttribute('src');
      frameBuffer.removeAttribute('src');
      frameWrap.classList.remove('is-loading');
      frameShell.hidden = false;
      blank.hidden = true;
      openLink.removeAttribute('href');
      window.wbSimulator.show({
        implementation: lens.key,
        udid: simulatorRef.device,
        label: item.label,
      });
      setTitle(item, null, lens);
      syncHash();
      return;
    }

    var url = viewUrl(item, lens, currentState);
    view.url = url;
    view.stateLabel = currentState ? stateLabel(item, currentState) : null;
    setTitle(item, view.stateLabel, lens);
    syncHash();

    showFrame(url);
  }

  /* ----------------------------------------------------------- actions */

  /* Whether the preview is live. Off means links don't navigate and forms
     don't submit, so clicking around a screen never takes you off it.

     The shell can't enforce that itself — off file:// the iframe is a foreign
     origin — so the flag travels in the URL and each page applies it through
     actions.js. It goes on the "open on its own" link too, so a page in its
     own tab behaves the way the toolbar says it should. */
  var actionsOn = false;

  /* The address the frame loads: the screen resolved against the project root,
     carrying both flags it needs — whether the preview is live, and which
     state of the screen to render. Same trip, same reason — the shell can't
     reach across the origin, so it hands them over in the URL. */
  function withFlags(src, state) {
    var url = root + src + (src.indexOf('?') > -1 ? '&' : '?') +
      'actions=' + (actionsOn ? 'on' : 'off');
    return state ? url + '&state=' + encodeURIComponent(state) : url;
  }

  /* Keep the current document visible while its replacement loads in the
     spare iframe, then exchange them in one paint. The first page has nothing
     to preserve, so it uses the frame background until ready. */
  function loadPreview(src) {
    if (!frame.getAttribute('src')) {
      frameWrap.classList.add('is-loading');
      frameReady = false;
      frame.src = src;
      return;
    }
    pendingFrame = frameBuffer;
    pendingFrame.src = src;
  }

  function frameLoaded(e) {
    var loaded = e.currentTarget;
    if (loaded === pendingFrame) {
      window.requestAnimationFrame(function () {
        if (loaded !== pendingFrame) return;
        var previous = frame;
        previous.classList.remove('is-active');
        previous.setAttribute('aria-hidden', 'true');
        loaded.classList.add('is-active');
        loaded.removeAttribute('aria-hidden');
        loaded.title = 'Preview';
        previous.title = 'Preview loading';
        frame = loaded;
        frameBuffer = previous;
        pendingFrame = null;
        frameReady = true;
        // A resize can start the spare frame before the initial frame has
        // loaded. Whichever wins must reveal the preview and enable prepare.
        frameWrap.classList.remove('is-loading');
        frameShell.hidden = false;
        previous.removeAttribute('src');
        reportRenderedStory();
        window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } }));
      });
      return;
    }
    if (loaded === frame) {
      window.requestAnimationFrame(function () {
        frameReady = true;
        frameWrap.classList.remove('is-loading');
        frameShell.hidden = false;
        reportRenderedStory();
      });
    }
  }

  frame.addEventListener('load', frameLoaded);
  frameBuffer.addEventListener('load', frameLoaded);

  /* ---------------------------------------------------------------- host */

  /* Embedded — in the editor, in a webview of its own — the workbench is half
     a tool: the screen list stands in the editor's sidebar, where a file tree
     usually is, and this half is the canvas. Two messages hold the two halves
     together, both carrying the same selection the address bar would:

       in   wb-go    the screen to show, as a hash
       out  wb-here  the screen now showing, after any pick

     wb-here is what keeps the editor's list marked when the pick was made
     over here — a link followed in a live preview, or a copied URL. */
  var host = window.parent !== window ? window.parent : null;
  window.wbEmbedded = !!host;

  if (host) {
    shell.classList.add('is-embedded');
    window.wbKeys.relay(host);
  }

  /* The lens rides along for whoever wants it; the editor's list only marks
     screens and states, and reads past it. */
  function tellHost(src, state, lens, pending) {
    if (!host) return;
    host.postMessage({ type: 'wb-here', src: src || null, state: state || null,
      lens: lens || null, pending: !!pending }, '*');
  }

  function tellHostRefreshFailed() {
    if (host) host.postMessage({ type: 'wb-refresh-failed' }, '*');
  }

  /* Links in a live preview ask the shell to navigate so they get the same
     double-buffered transition as sidebar choices.

     The page can't tell one of the project's own screens from anywhere else —
     it doesn't know where the project root is, and shouldn't have to — so it
     hands over the address it was about to visit and this decides. Anything
     outside the root, or inside it but not in the config, is not a screen of
     this workbench and is ignored. */
  window.addEventListener('message', function (e) {
    storySwitchEvent(e);
    if (host && view && view.lens && view.lens.kind === 'storybook') {
      var storyKey = window.wbKeys.storybook(e, frame, view.lens.url);
      if (storyKey) window.wbKeys.forward(storyKey, host);
    }
    var data = e.data || {};

    /* The live preview is another iframe down. Its physical key event cannot
       bubble here, so carry the serialized event through the shell to the
       editor-tab webview. Accept it only from one of our two preview frames. */
    if (data.type === 'wb-keyboard') {
      if (e.source !== frame.contentWindow && e.source !== frameBuffer.contentWindow) return;
      window.wbKeys.forward(data, host);
      return;
    }

    if (data.type === 'wb-context-menu' && host && e.source === frame.contentWindow) {
      var rect = frame.getBoundingClientRect();
      host.postMessage({
        type: 'wb-context-menu', x: rect.left + Number(data.x || 0), y: rect.top + Number(data.y || 0),
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

    if (e.source !== frame.contentWindow || data.type !== 'wb-preview-navigate') return;

    var href = String(data.href || '');
    if (href.indexOf(root) !== 0) return;

    var rest = href.slice(root.length);
    var query = rest.indexOf('?');
    var src = decodeURIComponent(query === -1 ? rest : rest.slice(0, query));
    if (!index[src]) return;

    if (nav) nav.reveal(src);
    writeHash(src, stateOf(index[src], data.state), stage.dataset.width);
  });

  function setActions(on) {
    actionsOn = !!on;
    actionsToggle.setAttribute('aria-checked', String(actionsOn));
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

  function showFrameSize() {
    frameSize.textContent = resizableWidth + ' × ' + resizableHeight;
  }

  /* A mode carries its width and, for devices with a real screen size, its
     height too. Resizable restores the last freeform size instead. */
  function setWidth(mode) {
    var button = null;
    stage.dataset.width = mode;
    widthButtons.forEach(function (b) {
      var on = b.dataset.width === mode;
      if (on) button = b;
      b.setAttribute('aria-pressed', String(on));
    });
    if (mode === 'fit') {
      frameShell.style.width = '';
      frameShell.style.height = '';
    } else if (mode === 'resizable') {
      frameShell.style.width = resizableWidth + 'px';
      frameShell.style.height = resizableHeight + 'px';
      showFrameSize();
    } else {
      frameShell.style.width = mode + 'px';
      frameShell.style.height = button && button.dataset.height
        ? button.dataset.height + 'px'
        : '';
    }
    try {
      localStorage.setItem('canonic-workbench-width', mode);
    } catch (e) {
      /* file:// storage can be blocked; the toolbar still works. */
    }
  }

  function updateWidthAvailability(item) {
    var supported = item ? window.wbManifest.viewportWidths(item.viewports) : widths.slice();
    widthButtons.forEach(function (button) {
      var enabled = supported.indexOf(button.dataset.width) !== -1;
      button.disabled = !enabled;
      if (!button.dataset.enabledTitle) button.dataset.enabledTitle = button.title;
      button.title = enabled ? button.dataset.enabledTitle : button.dataset.enabledTitle + ' · not supported by this screen';
    });
    if (item && supported.indexOf(stage.dataset.width) === -1) setWidth(supported[0] || 'fit');
  }

  /* A toolbar width change only resizes the frame already on the canvas.
     Keep the address shareable without sending the same screen through the
     hash router, which would navigate its iframe and reset live state. */
  function chooseWidth(mode) {
    var button = widthButtons.find(function (candidate) { return candidate.dataset.width === mode; });
    if (button && button.disabled) return;
    setWidth(mode);
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

  widthButtons.forEach(function (b) {
    b.addEventListener('click', function () {
      chooseWidth(b.dataset.width);
    });
  });

  /* Chrome-style rails sit outside the preview. Side rails change width, the
     bottom changes height, and either bottom corner changes both. */
  var frameDrag = null;

  function rememberFrameSize() {
    try {
      localStorage.setItem('canonic-workbench-resizable-width', String(resizableWidth));
      localStorage.setItem('canonic-workbench-resizable-height', String(resizableHeight));
    } catch (e) {
      /* file:// storage can be blocked; resizing still works this session. */
    }
  }

  Array.prototype.forEach.call(document.querySelectorAll('.wb-frame-resize'), function (handle) {
    handle.addEventListener('pointerdown', function (e) {
      if (stage.dataset.width !== 'resizable') return;
      var box = frameShell.getBoundingClientRect();
      frameDrag = {
        edge: handle.dataset.resize,
        x: e.clientX,
        y: e.clientY,
        width: box.width,
        height: box.height,
      };
      handle.setPointerCapture(e.pointerId);
      shell.classList.add('is-frame-resizing');
      e.preventDefault();
    });

    handle.addEventListener('pointermove', function (e) {
      if (!frameDrag) return;
      var horizontal = frameDrag.edge.indexOf('left') > -1
        ? frameDrag.x - e.clientX
        : e.clientX - frameDrag.x;
      if (frameDrag.edge !== 'bottom') {
        resizableWidth = Math.max(320, Math.round(frameDrag.width + horizontal));
        frameShell.style.width = resizableWidth + 'px';
      }
      if (frameDrag.edge.indexOf('bottom') > -1) {
        resizableHeight = Math.max(320, Math.round(frameDrag.height + e.clientY - frameDrag.y));
        frameShell.style.height = resizableHeight + 'px';
      }
      showFrameSize();
    });

    function endFrameDrag() {
      if (!frameDrag) return;
      frameDrag = null;
      shell.classList.remove('is-frame-resizing');
      rememberFrameSize();
    }

    handle.addEventListener('pointerup', endFrameDrag);
    handle.addEventListener('pointercancel', endFrameDrag);
    handle.addEventListener('lostpointercapture', endFrameDrag);
  });

  /* Reloading is what applies it — the flag is read as the page parses. */
  actionsToggle.addEventListener('click', function () {
    setActions(!actionsOn);
    if (current) show(current, currentState);
  });

  reload.addEventListener('click', function () {
    if (!current) return;
    if (view && view.lens && view.lens.kind === 'ios-simulator') {
      window.wbSimulator.reload();
      return;
    }
    if (!frame.getAttribute('src')) {
      show(current, currentState);
      return;
    }
    loadPreview(view && view.url ? view.url : frame.src);
  });

  storyButton.addEventListener('click', function () {
    openSources(false);
    openStories(storyMenu.hidden);
  });

  sourceButton.addEventListener('click', function () {
    openStories(false);
    openSources(sourceMenu.hidden);
  });

  document.addEventListener('pointerdown', function (e) {
    if (!storyMenu.hidden && !e.target.closest('.wb-stories')) openStories(false);
    if (!sourceMenu.hidden && !e.target.closest('.wb-sources')) openSources(false);
  });

  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Escape') return;
    openStories(false);
    openSources(false);
  });

  function route() {
    var target = parseHash();
    if (target.width) setWidth(target.width);
    /* A lens in the address is an instruction — a copied link says which —
       and one that isn't declared here means the design. No lens keeps the
       one the toolbar is on. */
    if (target.lens !== null) setLensPref(config.implementations[target.lens] ? target.lens : null);
    show(target.src, target.state);
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

  /* How the toolbar was left. These are the tool's preferences rather than
     the project's, so they are shared by every project the workbench opens —
     the sidebar you sized once stays that size. */
  function restore() {
    var saved = {};
    try {
      saved.width = localStorage.getItem('canonic-workbench-width');
      saved.sidebar = localStorage.getItem('canonic-workbench-sidebar');
      saved.actions = localStorage.getItem('canonic-workbench-actions');
      saved.resizableWidth = localStorage.getItem('canonic-workbench-resizable-width');
      saved.resizableHeight = localStorage.getItem('canonic-workbench-resizable-height');
      saved.lens = localStorage.getItem('canonic-workbench-lens');
    } catch (e) {
      /* no storage; fall back to the defaults below. */
    }
    /* The lens is checked against the config where it's used, so a name from
       another project's toolbar just means the design here. */
    lensPref = saved.lens || null;
    if (saved.resizableWidth) resizableWidth = parseInt(saved.resizableWidth, 10) || RESIZABLE_DEFAULT_WIDTH;
    if (saved.resizableHeight) resizableHeight = parseInt(saved.resizableHeight, 10) || RESIZABLE_DEFAULT_HEIGHT;
    /* A stored mode from an older build (or a hand-edited value) is ignored
       rather than leaving the switch with nothing pressed. */
    setWidth(widths.indexOf(saved.width) > -1 ? saved.width : 'fit');
    if (saved.sidebar) setSidebar(parseInt(saved.sidebar, 10));
    /* Only an explicit "on" turns them on — anything else, including nothing
       stored, lands on off. Must run before route(), which builds the src. */
    setActions(saved.actions === 'on');
  }

  /* -------------------------------------------------------------- boot */

  /* Nothing above runs until the project's config is in: the sidebar is the
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
          exportProject.hidden = !resolved;
          configureProject.hidden = !resolved;
        },
        function () {
          resolved = null;
          exportProject.hidden = true;
          configureProject.hidden = true;
        }
      )
      .then(function () { then(resolved); });
  }

  function applyConfig(loaded, first) {
    loadResolved(function () {
      config = loaded;
      if (resolved && resolved.implementations) config.implementations = resolved.implementations;
      groups = window.wbManifest.mergeSections(config.sections, (resolved && resolved.catalogSections) || []);
      index = window.wbNav.index(groups);
      storyCache = {};

      document.title = config.name;

      if (first) restore();

      /* The list is built here and picked from there: a row hands back the
         screen and the state it stands for, and the shell turns that into the
         hash, which is the one place the selection lives. */
      if (nav) {
        nav.update(groups);
      } else {
        nav = window.wbNav.create({
          rail: document.getElementById('rail'),
          title: document.getElementById('sectionName'),
          search: search,
          list: document.getElementById('nav'),
          groups: groups,
          onPick: function (src, state) {
            writeHash(src, state, stage.dataset.width);
          },
        });
      }

      route();
    });
  }

  function start(loaded) {
    applyConfig(loaded, true);
  }

  function refreshConfig(loaded) {
    applyConfig(loaded, false);
  }

  /* No config, no workbench — say which file and why, where the screens would
     have been. The toolbar stays; it just has nothing to act on. */
  function refuse(error) {
    blank.textContent = String(error.message || error);
    blank.hidden = false;
    console.error('[workbench] ' + String(error.message || error));
  }

  window.wbConfig.load(start, refuse);
})();
