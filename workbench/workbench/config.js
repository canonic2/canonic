/* The project's half of the workbench
   -----------------------------------
   Everything about the workbench that belongs to the project it is showing —
   its name and everything in its sidebar — is one file at the project root:

     workbench.yaml

   Nothing else in the workbench knows which project it is showing. Point it at
   another one and that file is the only thing that differs.

   Two things this file settles.

   WHERE THE PROJECT IS. The workbench ships inside the Canonic extension, so
   it is not in the project it shows and cannot find it by walking up from its
   own address. It is told, in this order:

     <meta>     an embedder that owns the document but not its address says it
                with <meta name="canonic-root">. The editor's sidebar is one:
                a webview's address is the editor's, so nothing about the
                document itself says where the project is.
     ?root=…    the same answer, for when the address is yours to write —
                opening the tool off disk against a project, say.
     http       whatever the server serves at /, which is the project — the
                extension mounts the two together on one origin.
     file://    two levels up, which is where a copy of this folder dropped
                into a repo still sits. Nothing ships that way any more; it
                stays because opening the tool straight off disk is useful.

   Every src in the config is relative to that root — `pages/sign-in.html`, the
   way you'd type it in a terminal at the top of the repo — rather than
   relative to the workbench.

   HOW YAML GETS READ AT ALL. There is no build step: the config is fetched and
   parsed in the browser by yaml.js. Fetched, which is the one thing that needs
   saying — over http (the Canonic extension serves the project) it just works,
   but a page opened straight off the disk isn't allowed to read its neighbours
   unless the browser was started with --allow-file-access-from-files. Without
   either, the workbench comes up empty and says so rather than looking broken.
*/

(function () {
  var FILE = 'workbench.yaml';
  /* Machine-specific parts — a dev server's port, where the product's code is
     on this disk — merged over the committed file, and left out of git. */
  var LOCAL = 'workbench.local.yaml';

  /* The project root, as a URL the frame's src can be resolved against.

     Resolved against the document's base rather than its address: in the
     editor's sidebar this file is loaded into a webview whose own URL is the
     editor's, not the workbench's, and a <base> is what says where the folder
     really is. With no base tag the two are the same thing. */
  function told() {
    var meta = document.querySelector('meta[name="canonic-root"]');
    if (meta && meta.content) return meta.content;
    var flag = /[?&]root=([^&]*)/.exec(location.search);
    if (!flag) return '';
    try {
      return decodeURIComponent(flag[1]);
    } catch (error) {
      /* An unreadable answer is worth less than a guess. */
      return '';
    }
  }

  function projectRoot() {
    var given = told();
    if (given) {
      /* Trailing slash or not is the caller's business, not the project's:
         without one the last segment reads as a filename and is dropped. */
      try {
        return new URL(given.slice(-1) === '/' ? given : given + '/', document.baseURI).href;
      } catch (error) {
        /* Fall through to working it out. */
      }
    }
    if (location.protocol !== 'file:') return new URL('/', document.baseURI).href;
    return new URL('../../', document.baseURI).href;
  }

  var root = projectRoot();

  function get(url, ok, fail) {
    var request = new XMLHttpRequest();
    request.open('GET', url, true);
    request.onload = function () {
      /* file:// answers 0 on success — there is no status in a filesystem. */
      if (request.status === 0 || (request.status >= 200 && request.status < 300)) {
        ok(request.responseText);
        return;
      }
      fail(new Error(
        request.status === 404
          ? 'There is no ' + FILE + ' at the project root.'
          : 'Asking for ' + FILE + ' answered ' + request.status + '.'
      ));
    };
    /* A file:// read that fails gives no reason — a missing file and a
       blocked one arrive identically — so the message has to carry both. */
    request.onerror = function () {
      fail(new Error(
        location.protocol === 'file:'
          ? 'Couldn’t read ' + FILE + '. Either it isn’t at the project root, or the ' +
            'browser is refusing to read local files: serve the project — the Workbench ' +
            'extension does — or start the browser with --allow-file-access-from-files.'
          : 'Couldn’t read ' + FILE + '.'
      ));
    };
    request.send();
  }

  /* The local file is optional: not there answers null rather than failing.
     Off file:// a missing file and a blocked one arrive identically, but the
     main file was already read the same way, so a failure here is a missing
     file. */
  function getOptional(url, ok, fail) {
    var request = new XMLHttpRequest();
    request.open('GET', url, true);
    request.onload = function () {
      if (request.status === 0 || (request.status >= 200 && request.status < 300)) ok(request.responseText);
      else if (request.status === 404) ok(null);
      else fail(new Error('Asking for ' + LOCAL + ' answered ' + request.status + '.'));
    };
    request.onerror = function () {
      ok(null);
    };
    request.send();
  }

  function text(value) {
    return typeof value === 'string' ? value.trim() : '';
  }

  function list(value) {
    return Object.prototype.toString.call(value) === '[object Array]' ? value : [];
  }

  /* A screen with fewer than two states has no choice to offer, so it stays a
     single row and its states are dropped. The first state is the page as
     authored, whatever it calls itself. */
  function states(raw, where, problems) {
    var out = [];
    list(raw).forEach(function (state, i) {
      var id = text(state && state.id);
      var label = text(state && state.label);
      if (!id || !label) {
        problems.push(where + ': state ' + (i + 1) + ' needs both an id and a label.');
        return;
      }
      if (!/^[a-z0-9-]+$/.test(id)) {
        problems.push(where + ': state id “' + id + '” must be kebab-case — it travels in a URL.');
        return;
      }
      out.push({ id: id, label: label });
    });
    return out.length > 1 ? out : null;
  }

  function screen(raw, where, impls, problems) {
    var label = text(raw && raw.label);
    var src = text(raw && raw.src);
    if (!label || !src) {
      problems.push(where + ': every screen needs a label and a src.');
      return null;
    }
    if (src.charAt(0) === '/' || src.indexOf('..') > -1) {
      problems.push(where + ': “' + src + '” must be relative to the project root.');
      return null;
    }
    /* Both mark something in the address — see address.js. */
    if (src.indexOf(':') > -1 || src.indexOf('~') > -1) {
      problems.push(where + ': src “' + src + '” can’t contain “:” or “~” — they mark the state and the lens in the address.');
      return null;
    }
    var item = { label: label, src: src };
    item.viewports = window.wbManifest.screenViewports(raw.viewports, where + ' › ' + label, problems);
    var icon = text(raw.icon);
    if (icon) item.icon = icon;
    var found = states(raw.states, where + ' › ' + label, problems);
    if (found) item.states = found;
    var lenses = window.wbManifest.screenLenses(raw.implementations, item, impls, where + ' › ' + label, problems);
    if (lenses) item.implementations = lenses;
    var code = window.wbManifest.screenCode(raw.code, impls, where + ' › ' + label, problems);
    if (code) item.code = code;
    return item;
  }

  /* A section holds screens, folders of screens, or both. Folders don't nest —
     one level is a flow, two is a filing cabinet. */
  function entries(raw, where, impls, problems, inFolder) {
    var out = [];
    list(raw).forEach(function (entry, i) {
      var at = where + ' › item ' + (i + 1);
      var folder = text(entry && entry.folder);

      if (folder) {
        if (inFolder) {
          problems.push(at + ': folders don’t nest.');
          return;
        }
        var items = entries(entry.items, where + ' › ' + folder, impls, problems, true);
        if (items.length) out.push({ folder: folder, items: items });
        else problems.push(at + ': folder “' + folder + '” has nothing in it.');
        return;
      }

      var found = screen(entry, at, impls, problems);
      if (found) out.push(found);
    });
    return out;
  }

  function sections(raw, impls, problems) {
    var out = [];
    list(raw).forEach(function (section, i) {
      var name = text(section && section.name);
      if (!name) {
        problems.push('section ' + (i + 1) + ' has no name.');
        return;
      }
      var items = entries(section && section.items, name, impls, problems, false);
      if (!items.length) {
        problems.push('section “' + name + '” has nothing in it.');
        return;
      }
      /* Rows wear their section's glyph, so a section without one is a
         section of unlabelled rows. */
      out.push({ group: name, icon: text(section.icon) || 'file-text', items: items });
    });
    return out;
  }

  function normalize(raw, hasLocal) {
    var problems = [];
    /* Screens refer to implementations by name, so those are read first. */
    var impls = window.wbManifest.implementations(raw && raw.implementations, problems);
    var previews = window.wbManifest.previews(raw && raw.previews, problems);
    var found = sections(raw && raw.sections, impls, problems);
    var importsCatalog = previews !== false || Object.keys(impls).some(function (key) { return impls[key].catalog; });
    if (!found.length && !importsCatalog) {
      problems.push('nothing to show — a config needs at least one section with one screen in it.');
      throw new Error(problems.join('\n'));
    }
    /* Everything that did parse is still worth showing; what didn't goes to
       the console, where the author can see all of it at once. */
    if (problems.length) {
      console.warn('[workbench] ' + FILE + (hasLocal ? ' + ' + LOCAL : '') + ':\n' + problems.join('\n'));
    }
    return { name: text(raw.name) || 'Workbench', sections: found, implementations: impls, previews: previews, root: root };
  }

  function trouble(file, error) {
    return new Error('There’s a problem in ' + file + ' — ' + String(error.message || error));
  }

  window.wbConfig = {
    root: root,
    FILE: FILE,
    LOCAL: LOCAL,

    /* Reads the config and hands it over, or hands over the reason it
       couldn't. Either way the caller decides what the shell does about it. */
    load: function (ok, fail) {
      get(
        root + FILE,
        function (main) {
          getOptional(
            root + LOCAL,
            function (local) {
              var config;
              var raw;
              try {
                raw = window.wbYaml(main);
              } catch (error) {
                fail(trouble(FILE, error));
                return;
              }
              if (local !== null) {
                try {
                  raw = window.wbManifest.merge(raw, window.wbYaml(local));
                } catch (error) {
                  fail(trouble(LOCAL, error));
                  return;
                }
              }
              try {
                config = normalize(raw, local !== null);
              } catch (error) {
                fail(trouble(FILE, error));
                return;
              }
              ok(config);
            },
            fail
          );
        },
        fail
      );
    },
  };
})();
