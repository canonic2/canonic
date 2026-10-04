/* Implementations — the part of the config that names them
   -------------------------------------------------------
   A design screen can be seen through lenses onto its implementation: a
   Storybook story, a page on a dev server, the same page on staging. The
   config declares those once, at the top, and each screen says which of them
   it has and where it is there:

     implementations:
       storybook:
         kind: storybook
         url: http://localhost:6006
         root: ../product/packages/ui     # optional; where that code lives
       dev:
         kind: url
         base: http://localhost:3710
         root: ../product
       staging:
         kind: url
         base: https://staging.example.com
       emulator:
         kind: window                     # a live stream of an app's window, on macOS
         app: com.example.emulator        # bundle ID, or part of it

     sections:
       - name: Pages
         items:
           - label: Sign in
             src: pages/sign-in.html
             implementations:
               dev: /                     # one path, or a map of state ids to paths
               staging:
                 default: /
                 error: /?error=1
               emulator: Example Phone    # the window's title, or part of it
             code:                       # implementation -> path(s), relative to its root
               dev: packages/auth/src/pages/login

   Machine-specific parts — a folder path, a port — go in workbench.local.yaml
   beside it, which is merged over the committed file and left out of git.

   Two readers use these rules: the workbench in the browser and the server in
   node, which answers for the machine it runs on. One file, so the two can't
   disagree about which lens a screen has. Pure functions, no DOM, no fs; the
   footer hands it to whichever of the two is loading it. */
(function (host, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else host.wbManifest = factory();
})(typeof window !== 'undefined' ? window : this, function () {
  var KEY = /^[a-z0-9-]+$/;
  var HTTP = /^https?:\/\/[^/]+/i;
  var SCHEME = /^[a-z][a-z0-9+.-]*:\/\//i;
  var BUNDLE = /^[A-Za-z0-9.-]+$/;
  /* Kinds shown as a live stream of a native window rather than an iframe. */
  var STREAMED = ['ios-simulator', 'window'];
  var VIEWPORTS = ['fit', 'desktop', 'mobile', 'responsive'];

  function text(value) {
    if (typeof value === 'number') return String(value);
    return typeof value === 'string' ? value.trim() : '';
  }

  function isMap(value) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
  }

  function isList(value) {
    return Object.prototype.toString.call(value) === '[object Array]';
  }

  /* "local-dev" -> "Local dev": the button reads like a word, not a key. */
  function labelOf(key) {
    var words = String(key).split('-').join(' ');
    return words.charAt(0).toUpperCase() + words.slice(1);
  }

  /* How an exported screen should be framed. Responsive deliberately means
     two references; the renderer expands it into desktop and mobile. */
  function screenViewports(raw, where, problems) {
    if (raw === undefined || raw === null || raw === '') return VIEWPORTS.slice();
    var values = isList(raw) ? raw : [raw];
    var out = [];
    values.forEach(function (entry) {
      var value = text(entry).toLowerCase();
      if (VIEWPORTS.indexOf(value) === -1) {
        problems.push(where + ': viewports may only contain desktop, mobile, responsive, or fit.');
        return;
      }
      if (out.indexOf(value) === -1) out.push(value);
    });
    return out.length ? out : VIEWPORTS.slice();
  }

  function viewportWidths(viewports) {
    var map = { fit: 'fit', desktop: '1512', mobile: '393', responsive: 'resizable' };
    return (viewports || VIEWPORTS).map(function (viewport) { return map[viewport]; }).filter(Boolean);
  }

  function catalog(raw, where, problems) {
    if (raw === undefined || raw === null || raw === false) return null;
    if (raw === true) return { icon: 'book-open', icons: {} };
    if (!isMap(raw)) {
      problems.push(where + ': catalog must be true, false, or a map of icon settings.');
      return null;
    }
    var fallback = text(raw.icon) || 'book-open';
    if (!KEY.test(fallback)) {
      problems.push(where + ': catalog icon “' + fallback + '” must be a kebab-case Lucide icon name.');
      fallback = 'book-open';
    }
    var icons = {};
    if (raw.icons !== undefined && !isMap(raw.icons)) {
      problems.push(where + ': catalog icons must map Storybook title prefixes to Lucide icon names.');
    } else {
      Object.keys(raw.icons || {}).forEach(function (prefix) {
        var icon = text(raw.icons[prefix]);
        if (!text(prefix) || !KEY.test(icon)) {
          problems.push(where + ': catalog icon for “' + prefix + '” must be a kebab-case Lucide icon name.');
          return;
        }
        icons[text(prefix).replace(/\/+$/, '')] = icon;
      });
    }
    return { icon: fallback, icons: icons };
  }

  /* A local command may bring an implementation up before its catalog is
     read. The check is explicit so a busy port or a different health route
     can be chosen by the project rather than guessed from its URL. */
  function startupProbe(raw, where, problems) {
    if (!isMap(raw)) {
      problems.push(where + ' must contain a port or URL.');
      return null;
    }
    if (raw.port !== undefined && raw.url === undefined &&
        Number.isInteger(Number(raw.port)) && Number(raw.port) >= 1 && Number(raw.port) <= 65535) {
      return { port: Number(raw.port), host: text(raw.host) || '127.0.0.1' };
    }
    if (raw.url !== undefined && raw.port === undefined && HTTP.test(text(raw.url))) {
      return { url: text(raw.url) };
    }
    problems.push(where + ' needs exactly one valid port (1–65535) or http(s) URL.');
    return null;
  }

  function startup(raw, where, problems) {
    if (raw === undefined || raw === null) return null;
    if (!isMap(raw)) {
      problems.push(where + ': start must be a map with command and check.');
      return null;
    }
    var command = text(raw.command);
    if (!command) problems.push(where + ': start.command must be a nonempty shell command.');
    var probe = startupProbe(raw.check, where + ': start.check', problems);
    var ready = raw.ready === undefined ? null : startupProbe(raw.ready, where + ': start.ready', problems);
    var timeout = raw.timeout;
    if (timeout !== undefined && (!Number.isInteger(Number(timeout)) || Number(timeout) < 1 || Number(timeout) > 300)) {
      problems.push(where + ': start.timeout must be 1–300 seconds.');
      timeout = null;
    }
    if (!command || !probe || (raw.ready !== undefined && !ready)) return null;
    var result = {
      command: command,
      cwd: text(raw.cwd) || '.',
      check: probe,
      timeout: timeout === undefined || timeout === null ? 60 : Number(timeout),
    };
    if (ready) result.ready = ready;
    return result;
  }

  /* A story id is Storybook's kebab of the title, two dashes, the kebab of
     the story's name: "components-button--icon-only". The part after the
     dashes is what stands in the address where a design state would. */
  function storyState(id) {
    var at = String(id).indexOf('--');
    return at === -1 ? String(id) : String(id).slice(at + 2);
  }

  /* workbench.local.yaml over workbench.yaml. Top-level keys replace, except
     the implementations map, which merges one level deep so a local file can
     say `dev: { base: http://localhost:4000 }` and leave the rest alone. */
  function merge(base, local) {
    var out = {};
    var key;
    base = isMap(base) ? base : {};
    local = isMap(local) ? local : {};
    for (key in base) out[key] = base[key];
    for (key in local) {
      /* Projects merge by key, each one the way the whole file does, so a
         local file can change one project's port and leave the rest. */
      if (key === 'projects' && isMap(base.projects) && isMap(local.projects)) {
        var projects = {};
        var id;
        for (id in base.projects) projects[id] = base.projects[id];
        for (id in local.projects) {
          projects[id] = isMap(projects[id]) && isMap(local.projects[id])
            ? merge(projects[id], local.projects[id])
            : local.projects[id];
        }
        out.projects = projects;
        continue;
      }
      if (key !== 'implementations' || !isMap(base.implementations) || !isMap(local.implementations)) {
        out[key] = local[key];
        continue;
      }
      var impls = {};
      var name;
      for (name in base.implementations) impls[name] = base.implementations[name];
      for (name in local.implementations) {
        impls[name] = isMap(impls[name]) && isMap(local.implementations[name])
          ? Object.assign({}, impls[name], local.implementations[name])
          : local.implementations[name];
      }
      out.implementations = impls;
    }
    return out;
  }

  /* The declared implementations, keyed by name. Anything malformed is named
     in `problems` and left out; the rest still work. */
  function previews(raw, problems) {
    if (raw === false) return false;
    if (raw === undefined || raw === null) return {};
    if (!isMap(raw)) { problems.push('previews: must be false or a map with include and config.'); return false; }
    var result = {};
    if (raw.include !== undefined) {
      if (!Array.isArray(raw.include) || !raw.include.length || raw.include.some(function (pattern) { return typeof pattern !== 'string' || !pattern.trim() || pattern.charAt(0) === '/' || pattern.indexOf('..') !== -1; })) {
        problems.push('previews.include: must be a nonempty list of project-relative glob patterns.');
        return false;
      }
      result.include = raw.include.slice();
    }
    if (raw.config !== undefined) {
      var file = text(raw.config);
      if (!file || file.charAt(0) === '/' || file.indexOf('..') !== -1 || SCHEME.test(file)) { problems.push('previews.config: must be a project-relative config file.'); return false; }
      result.config = file;
    }
    return result;
  }

  function implementations(raw, problems) {
    var out = {};
    if (raw === undefined || raw === null || raw === '') return out;
    if (!isMap(raw)) {
      problems.push('implementations: must be a map of names to implementations.');
      return out;
    }
    Object.keys(raw).forEach(function (key) {
      var where = 'implementations › ' + key;
      var spec = raw[key];
      if (!KEY.test(key)) {
        problems.push('implementations: “' + key + '” must be kebab-case — it travels in a URL.');
        return;
      }
      if (!isMap(spec)) {
        problems.push(where + ': needs a kind, and a url or base.');
        return;
      }
      var kind = text(spec.kind);
      if (kind !== 'url' && kind !== 'storybook' && kind !== 'workbench' && kind !== 'ios-simulator' && kind !== 'window') {
        problems.push(where + ': kind must be url, storybook, workbench, ios-simulator, or window.');
        return;
      }
      var impl = { key: key, label: text(spec.label) || labelOf(key), kind: kind, root: null };
      if (kind === 'workbench') {
        impl.root = '.';
      } else if (kind === 'ios-simulator') {
        impl.device = text(spec.device) || 'booted';
      } else if (kind === 'window') {
        var app = text(spec.app);
        if (!BUNDLE.test(app)) {
          problems.push(where + ': needs an app — the application’s bundle ID, or part of it, such as com.example.app.');
          return;
        }
        impl.app = app;
      } else {
        var field = kind === 'url' ? 'base' : 'url';
        var implementationAddress = text(spec[field]);
        if (kind === 'storybook' && implementationAddress === 'auto') {
          impl.url = 'auto';
          impl.auto = true;
        } else if (!HTTP.test(implementationAddress)) {
          problems.push(where + ': needs a ' + field + ' starting with http:// or https://' +
            (kind === 'storybook' ? ', or url: auto.' : '.'));
          return;
        } else {
          impl[field] = implementationAddress.replace(/\/+$/, '');
        }
      }

      var imported = catalog(spec.catalog, where, problems);
      if (imported) {
        if (kind !== 'storybook' && kind !== 'ios-simulator') {
          problems.push(where + ': catalog is only available for Storybook and iOS Simulator implementations.');
        }
        else {
          impl.catalog = true;
          impl.catalogIcon = kind === 'ios-simulator' && imported.icon === 'book-open' ? 'smartphone' : imported.icon;
          impl.catalogIcons = imported.icons;
        }
      }

      if (spec.render !== undefined) {
        problems.push(where + ': render is no longer used; remove it. URL and Storybook implementations use iframes.');
      }

      if (spec.start !== undefined) {
        if (kind === 'ios-simulator' || kind === 'window' || kind === 'workbench') {
          problems.push(where + ': start is available for URL and Storybook implementations.');
        } else {
          impl.start = startup(spec.start, where, problems);
        }
      }

      var root = text(spec.root);
      if (root) {
        if (kind === 'workbench' && root !== '.') { problems.push(where + ': Workbench previews use this project root.'); return; }
        if (SCHEME.test(root)) problems.push(where + ': root must be a folder path, not a URL.');
        else impl.root = root;
      }

      out[key] = impl;
    });
    return out;
  }

  /* Where an implementation lives: its base or url. */
  function address(impl) {
    return impl ? impl.base || impl.url || '' : '';
  }

  /* Manual and imported sections share one rail. Matching names are one
     section, not duplicate buttons that both claim the same selection. */
  function mergeSections(base, imported) {
    function clone(items) { return (items || []).map(function (item) { return item.folder ? Object.assign({}, item, { items: clone(item.items) }) : Object.assign({}, item); }); }
    function mergeItems(target, items) {
      items.forEach(function (item) {
        var folder = item.folder && target.find(function (candidate) { return candidate.folder === item.folder; });
        if (folder) mergeItems(folder.items, item.items || []);
        else target.push(item);
      });
      return target;
    }
    var out = (base || []).map(function (section) {
      return Object.assign({}, section, { items: clone(section.items) });
    });
    function existing(src) {
      var found = null;
      function visit(items) { items.forEach(function (item) { if (item.folder) visit(item.items); else if (item.src === src) found = item; }); }
      out.forEach(function (section) { visit(section.items); });
      return found;
    }
    (imported || []).forEach(function (section) {
      function remaining(items) { return (items || []).map(function (item) {
        if (item.folder) { var children = remaining(item.items); return children.length ? Object.assign({}, item, { items: children }) : null; }
        var original = item.workbench && existing(item.src);
        if (original) { original.states = item.states; original.workbench = true; if (!original.viewports && item.viewports) original.viewports = item.viewports; return null; }
        return item;
      }).filter(Boolean); }
      var items = remaining(clone(section.items));
      if (!items.length) return;
      var found = out.find(function (candidate) { return candidate.group === section.group; });
      if (found) mergeItems(found.items, items);
      else out.push(Object.assign({}, section, { items: mergeItems([], items) }));
    });
    return out;
  }

  function pathOf(value, key, impl, where, problems) {
    var p = text(value);
    if (p.charAt(0) === '/') return p;
    problems.push(
      where + ': implementation “' + key + '” paths must start with / — they’re appended to ' +
        address(impl) + '.'
    );
    return null;
  }

  /* The lenses one screen has, checked against what's declared and against
     the screen's own states. Answers with the map, or undefined when nothing
     survived — a screen without lenses carries no key at all. */
  function screenLenses(raw, item, impls, where, problems) {
    if (raw === undefined || raw === null || raw === '') return undefined;
    if (!isMap(raw)) {
      problems.push(where + ': implementations must be a map of implementation names to paths.');
      return undefined;
    }
    var out = {};
    var count = 0;
    var states = (item && item.states ? item.states : []).map(function (state) {
      return state.id;
    });
    var first = states.length ? states[0] : 'default';

    Object.keys(raw).forEach(function (key) {
      var impl = impls[key];
      var value = raw[key];
      if (!impl) {
        problems.push(where + ': implementation “' + key + '” isn’t declared under implementations.');
        return;
      }

      if (impl.kind === 'storybook') {
        var title = text(value);
        if (!title) {
          problems.push(where + ': implementation “' + key + '” needs a story title.');
          return;
        }
        out[key] = { title: title };
        count += 1;
        return;
      }

      if (impl.kind === 'workbench') {
        var preview = text(value);
        if (!/^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/.test(preview)) { problems.push(where + ': Workbench lens needs a preview ID.'); return; }
        out[key] = { preview: preview };
        count += 1;
        return;
      }

      if (impl.kind === 'ios-simulator') {
        var device = text(value);
        if (!device) {
          problems.push(where + ': implementation “' + key + '” needs a simulator device name or UDID.');
          return;
        }
        out[key] = { device: device };
        count += 1;
        return;
      }

      if (impl.kind === 'window') {
        var windowTitle = text(value);
        if (!windowTitle) {
          problems.push(where + ': implementation “' + key + '” needs the window’s title, or part of it.');
          return;
        }
        out[key] = { window: windowTitle };
        count += 1;
        return;
      }

      if (typeof value === 'string' || typeof value === 'number') {
        var single = pathOf(value, key, impl, where, problems);
        if (single === null) return;
        out[key] = { path: single };
        count += 1;
        return;
      }

      if (!isMap(value)) {
        problems.push(
          where + ': implementation “' + key + '” needs a path, or a map of this screen’s state ids to paths.'
        );
        return;
      }

      var ref = { path: null, states: {} };
      Object.keys(value).forEach(function (stateId) {
        var p = pathOf(value[stateId], key, impl, where, problems);
        if (p === null) return;
        /* The first state is the page as authored, whatever it calls itself,
           and "default" always means it too. */
        if (stateId === first || stateId === 'default') {
          ref.path = p;
          return;
        }
        if (states.indexOf(stateId) === -1) {
          problems.push(
            where + ': implementation “' + key + '” maps state “' + stateId + '”, which this screen doesn’t declare.'
          );
          return;
        }
        ref.states[stateId] = p;
      });
      if (ref.path === null) {
        problems.push(
          where + ': implementation “' + key + '” needs a path for the default state (“' + first + '”).'
        );
        return;
      }
      out[key] = ref;
      count += 1;
    });

    return count ? out : undefined;
  }

  /* Where a screen's implementation code lives, by implementation. Paths are
     relative to that implementation's root — or absolute, for a machine that
     keeps them somewhere else, which is what workbench.local.yaml is for. */
  function screenCode(raw, impls, where, problems) {
    if (raw === undefined || raw === null || raw === '') return undefined;
    if (!isMap(raw)) {
      problems.push(where + ': code must be a map of implementation names to paths.');
      return undefined;
    }
    var out = [];
    Object.keys(raw).forEach(function (key) {
      if (!impls[key]) {
        problems.push(where + ': code names implementation “' + key + '”, which isn’t declared under implementations.');
        return;
      }
      var values = isList(raw[key]) ? raw[key] : [raw[key]];
      var found = 0;
      values.forEach(function (value) {
        var p = text(value);
        if (!p) return;
        out.push({ implementation: key, path: p });
        found += 1;
      });
      if (!found) {
        problems.push(where + ': code for “' + key + '” needs a path relative to that implementation’s root.');
      }
    });
    return out.length ? out : undefined;
  }

  /* Whether a lens shows a native window's stream instead of a page. */
  function streamed(lens) {
    return !!lens && STREAMED.indexOf(lens.kind) > -1;
  }

  /* How the project is marked in the project switcher: `color`, one of the
     named colours or a hex value, and `icon`, a Lucide icon name or an image
     file in the project. Both are optional; without them the mark is the
     name's first letter on a colour chosen from the project's folder. A value
     that can't be used is named in `problems` and left out. */
  var PROJECT_COLORS = ['blue', 'green', 'orange', 'purple', 'pink', 'teal', 'red', 'yellow', 'gray'];
  var HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;
  var IMAGE = /\.(?:svg|png|jpe?g|webp|gif)$/i;

  function projectMark(raw, problems) {
    var out = { color: null, icon: null, image: null };
    raw = isMap(raw) ? raw : {};
    if (raw.color !== undefined && raw.color !== null && raw.color !== '') {
      var color = text(raw.color).toLowerCase();
      if (PROJECT_COLORS.indexOf(color) > -1 || HEX.test(color)) out.color = color;
      else problems.push('color: must be one of ' + PROJECT_COLORS.join(', ') + ', or a hex colour such as #2f7d55.');
    }
    if (raw.icon !== undefined && raw.icon !== null && raw.icon !== '') {
      var icon = text(raw.icon);
      if (KEY.test(icon)) out.icon = icon;
      else if (IMAGE.test(icon) && icon.charAt(0) !== '/' && icon.indexOf('..') === -1 && !SCHEME.test(icon) && icon.indexOf('\\') === -1) out.image = icon;
      else problems.push('icon: must be a kebab-case Lucide icon name, or a project-relative .svg, .png, .jpg, .webp, or .gif file.');
    }
    return out;
  }

  /* Several projects in one file
     ----------------------------
     A file may list projects under `projects`, keyed by kebab-case id. Each
     is a config of its own — `name`, `color`, `icon`, `sections`, `previews`,
     `implementations` — plus an optional `root`, the folder it serves and
     resolves its paths against, relative to the file's folder. Every other
     top-level key is shared: a project starts from the top level and its own
     keys replace those, with `implementations` merged by name. A project's
     `name`, `color` and `icon` are its own and never inherited; its name
     defaults to its id. A file without `projects` is one project, as ever. */
  var PROJECT_OWN = ['name', 'color', 'icon', 'root'];

  function projectKeys(raw, problems) {
    if (!isMap(raw) || raw.projects === undefined || raw.projects === null) return [];
    if (!isMap(raw.projects)) {
      problems.push('projects: must be a map of project ids to projects.');
      return [];
    }
    return Object.keys(raw.projects).filter(function (key) {
      if (!KEY.test(key)) {
        problems.push('projects: “' + key + '” must be a kebab-case id.');
        return false;
      }
      if (!isMap(raw.projects[key])) {
        problems.push('projects › ' + key + ': must be a map.');
        return false;
      }
      return true;
    });
  }

  /* The config one project sees, and where it lives: { raw, key, root }.
     `key` null, or one the file doesn't list, picks the first project; a
     file without projects answers with itself. `root` is the project's own,
     as written, or null for the file's folder. */
  function selectProject(raw, key, problems) {
    var keys = projectKeys(raw, problems);
    if (!keys.length) {
      var whole = {};
      Object.keys(isMap(raw) ? raw : {}).forEach(function (name) { if (name !== 'projects') whole[name] = raw[name]; });
      return { raw: whole, key: null, root: null };
    }
    var picked = keys.indexOf(key) > -1 ? key : keys[0];
    if (key && picked !== key) problems.push('projects: there is no project “' + key + '”.');
    var shared = {};
    Object.keys(raw).forEach(function (name) {
      if (name !== 'projects' && PROJECT_OWN.indexOf(name) === -1) shared[name] = raw[name];
    });
    var entry = raw.projects[picked];
    var own = {};
    Object.keys(entry).forEach(function (name) { if (name !== 'root') own[name] = entry[name]; });
    var effective = merge(shared, own);
    if (!text(effective.name)) effective.name = labelOf(picked);
    var root = entry.root === undefined || entry.root === null || entry.root === '' ? null : text(entry.root);
    if (entry.root !== undefined && entry.root !== null && entry.root !== '' && (!root || SCHEME.test(root))) {
      problems.push('projects › ' + picked + ': root must be a folder path.');
      root = null;
    }
    return { raw: effective, key: picked, root: root };
  }

  return {
    merge: merge,
    streamed: streamed,
    projectKeys: projectKeys,
    selectProject: selectProject,
    projectMark: projectMark,
    PROJECT_COLORS: PROJECT_COLORS,
    implementations: implementations,
    previews: previews,
    screenViewports: screenViewports,
    viewportWidths: viewportWidths,
    screenLenses: screenLenses,
    screenCode: screenCode,
    labelOf: labelOf,
    storyState: storyState,
    address: address,
    mergeSections: mergeSections,
  };
});
