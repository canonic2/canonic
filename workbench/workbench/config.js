/* The space's half of the workbench
   ---------------------------------
   Everything about the workbench that belongs to the space it is showing —
   its name and everything in its sidebar — is one file at the project root:

     workbench.yaml

   Nothing else in the workbench knows which space it is showing. Point it at
   another one and that file is the only thing that differs.

   Two things this file settles.

   WHERE THE ROOT IS. The workbench ships inside the Canonic extension, so
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

  /* WHICH CONFIG. A workbench.yaml can list several spaces, each with a
     root of its own, so the file isn't always at the root and isn't always
     all one space. Whoever serves a space like that says so: <meta
     name="canonic-config"> is the folder holding the file, and <meta
     name="canonic-space"> is the space's key in it. Without them the file
     is at the root, and a file that lists spaces shows its first. */
  function meta(name) {
    var tag = document.querySelector('meta[name="' + name + '"]');
    return tag && tag.content ? tag.content : '';
  }

  var configBase = (function () {
    var given = meta('canonic-config');
    if (!given) return root;
    try {
      return new URL(given.slice(-1) === '/' ? given : given + '/', document.baseURI).href;
    } catch (error) {
      return root;
    }
  })();
  var spaceKey = meta('canonic-space') || null;

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

  /* A page with fewer than two states has no choice to offer, so it stays a
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

  function page(raw, where, impls, problems) {
    var label = text(raw && raw.label);
    var src = text(raw && raw.src);
    if (!label || !src) {
      problems.push(where + ': every page needs a label and a src.');
      return null;
    }
    if (src.charAt(0) === '/' || src.indexOf('..') > -1) {
      problems.push(where + ': “' + src + '” must be relative to the project root.');
      return null;
    }
    /* They mark something in the address — see address.js. */
    var reserved = window.wbManifest.srcProblem(src);
    if (reserved) {
      problems.push(where + ': ' + reserved);
      return null;
    }
    /* Sizes are resolved by the server, which sends them per page in
       /_workbench/config; see src/sizes/. */
    var item = { label: label, src: src };
    window.wbManifest.pageLensLabel(raw.lensLabel, item, where + ' › ' + label, problems);
    var icon = text(raw.icon);
    if (icon) item.icon = icon;
    var found = states(raw.states, where + ' › ' + label, problems);
    if (found) item.states = found;
    var lenses = window.wbManifest.pageLenses(raw.implementations, item, impls, where + ' › ' + label, problems);
    if (lenses) item.implementations = lenses;
    window.wbManifest.docsPageEntry(raw, item, where + ' › ' + label, problems);
    var code = window.wbManifest.pageCode(raw.code, impls, where + ' › ' + label, problems);
    if (code) item.code = code;
    return item;
  }

  /* A collection holds pages, groups of pages, or both. Groups don't nest —
     one level is a flow, two is a filing cabinet. */
  function entries(raw, where, impls, problems, inGroup) {
    var out = [];
    list(raw).forEach(function (entry, i) {
      var at = where + ' › item ' + (i + 1);
      var group = text(entry && entry.group);

      if (group) {
        if (inGroup) {
          problems.push(at + ': groups don’t nest.');
          return;
        }
        var items = entries(entry.items, where + ' › ' + group, impls, problems, true);
        if (items.length) out.push({ group: group, items: items });
        else problems.push(at + ': group “' + group + '” has nothing in it.');
        return;
      }

      var found = page(entry, at, impls, problems);
      if (found) out.push(found);
    });
    return out;
  }

  function collections(raw, impls, problems) {
    var out = [];
    list(raw).forEach(function (collection, i) {
      var name = text(collection && collection.name);
      if (!name) {
        problems.push('collection ' + (i + 1) + ' has no name.');
        return;
      }
      var items = entries(collection && collection.items, name, impls, problems, false);
      if (!items.length && !text(collection.icon)) {
        problems.push('collection “' + name + '” has nothing in it.');
        return;
      }
      /* Rows wear their collection's glyph, so a collection without one is a
         collection of unlabelled rows. */
      out.push({ name: name, icon: text(collection.icon) || 'file-text', iconPriority: text(collection.icon) ? 7 : 2, items: items });
    });
    return out;
  }

  function normalize(raw, hasLocal) {
    var problems = [];
    raw = window.wbManifest.selectSpace(raw, spaceKey, problems).raw;
    /* Pages refer to implementations by name, so those are read first. */
    var impls = window.wbManifest.implementations(raw && raw.implementations, problems);
    var previews = window.wbManifest.previews(raw && raw.previews, problems);
    var found = collections(raw && raw.collections, impls, problems);
    var mark = window.wbManifest.spaceMark(raw, problems);
    var importsCatalog = previews !== false || Object.keys(impls).some(function (key) { return impls[key].catalog; });
    if (!found.some(function (collection) { return collection.items.length; }) && !importsCatalog) {
      problems.push('nothing to show — a config needs at least one collection with one page in it.');
      throw new Error(problems.join('\n'));
    }
    /* Everything that did parse is still worth showing; what didn't goes to
       the console, where the author can see all of it at once. */
    if (problems.length) {
      console.warn('[workbench] ' + FILE + (hasLocal ? ' + ' + LOCAL : '') + ':\n' + problems.join('\n'));
    }
    return { name: text(raw.name) || 'Workbench', mark: mark, collections: found, implementations: impls, previews: previews, root: root };
  }

  function trouble(file, error) {
    return new Error('There’s a problem in ' + file + ' — ' + String(error.message || error));
  }

  window.wbConfig = {
    root: root,
    space: spaceKey,
    FILE: FILE,
    LOCAL: LOCAL,

    /* Reads the config and hands it over, or hands over the reason it
       couldn't. Either way the caller decides what the shell does about it. */
    load: function (ok, fail) {
      get(
        configBase + FILE,
        function (main) {
          getOptional(
            configBase + LOCAL,
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
