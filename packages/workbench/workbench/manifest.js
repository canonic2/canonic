/* Implementations — the part of the config that names them
   -------------------------------------------------------
   A design page can be seen through lenses onto its implementation: a
   Storybook story, a page on a dev server, the same page on staging. The
   config declares those once, at the top, and each page says which of them
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

     collections:
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
   disagree about which lens a page has. Pure functions, no DOM, no fs; the
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
  /* Characters that mark the state, the example, and the lens in the address
     (see address.js), so no src may contain them. */
  var RESERVED = [':', '!', '~'];
  var DOCS = /\.md$/i;

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

  /* A path inside the project, relative to its root: what docs pages and
     their example sources point at. */
  function projectPath(value) {
    var p = text(value);
    if (!p || p.charAt(0) === '/' || /^[a-z]:[\\/]/i.test(p) || SCHEME.test(p)) return null;
    if (p.split(/[\\/]/).indexOf('..') !== -1) return null;
    return p;
  }

  /* Why a src can't be used, or null when it can. */
  function srcProblem(src) {
    var found = RESERVED.filter(function (mark) { return src.indexOf(mark) !== -1; });
    if (!found.length) return null;
    return 'src “' + src + '” can’t contain ' + found.map(function (mark) { return '“' + mark + '”'; }).join(' or ') +
      ' — “:”, “!”, and “~” mark the state, the example, and the lens in the address.';
  }

  /* A page written in Markdown: its docs are its own view, so it has no
     design lens. See specs/docs-pages.md. */
  function isDocs(src) {
    return DOCS.test(text(src));
  }

  /* "local-dev" -> "Local dev": the button reads like a word, not a key. */
  function labelOf(key) {
    var words = String(key).split('-').join(' ');
    return words.charAt(0).toUpperCase() + words.slice(1);
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
    return Object.assign({ icon: fallback, icons: icons }, text(raw.icon) && KEY.test(text(raw.icon)) ? { iconExplicit: true } : {});
  }

  /* A local command may bring an implementation up before its catalog is
     read. The check is explicit so a busy port or a different health route
     can be chosen in the config rather than guessed from its URL. */
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
     the implementations and sizes maps, which merge one level deep so a local
     file can say `dev: { base: http://localhost:4000 }` and leave the rest
     alone. A key the local file adds goes at the end, keeping the order of
     the committed ones. */
  var BY_NAME = ['implementations', 'sizes'];

  function merge(base, local) {
    var out = {};
    var key;
    base = isMap(base) ? base : {};
    local = isMap(local) ? local : {};
    for (key in base) out[key] = base[key];
    for (key in local) {
      /* Spaces merge by key, each one the way the whole file does, so a
         local file can change one space's port and leave the rest. */
      if (key === 'spaces' && isMap(base.spaces) && isMap(local.spaces)) {
        var spaces = {};
        var id;
        for (id in base.spaces) spaces[id] = base.spaces[id];
        for (id in local.spaces) {
          spaces[id] = isMap(spaces[id]) && isMap(local.spaces[id])
            ? merge(spaces[id], local.spaces[id])
            : local.spaces[id];
        }
        out.spaces = spaces;
        continue;
      }
      if (BY_NAME.indexOf(key) === -1 || !isMap(base[key]) || !isMap(local[key])) {
        out[key] = local[key];
        continue;
      }
      var named = {};
      var name;
      for (name in base[key]) named[name] = base[key][name];
      for (name in local[key]) {
        named[name] = isMap(named[name]) && isMap(local[key][name])
          ? Object.assign({}, named[name], local[key][name])
          : local[key][name];
      }
      out[key] = named;
    }
    return out;
  }

  /* The declared implementations, keyed by name. Anything malformed is named
     in `problems` and left out; the rest still work. */
  function previews(raw, problems) {
    if (raw === false) return false;
    if (raw === undefined || raw === null) return {};
    if (!isMap(raw)) { problems.push('previews: must be false or a map with include, config, and icon settings.'); return false; }
    var result = {};
    var lensLabel = readLensLabel(raw.lensLabel, 'previews.lensLabel', problems);
    if (lensLabel) result.lensLabel = lensLabel;
    if (raw.icon !== undefined) {
      if (!KEY.test(text(raw.icon))) problems.push('previews.icon: must be a kebab-case Lucide icon name.');
      else result.icon = text(raw.icon);
    }
    if (raw.icons !== undefined) {
      result.icons = {};
      if (!isMap(raw.icons)) problems.push('previews.icons: must map title prefixes to Lucide icon names.');
      else Object.keys(raw.icons).forEach(function (prefix) {
        var normalized = text(prefix).replace(/\/+$/, '');
        var icon = text(raw.icons[prefix]);
        if (!normalized || !KEY.test(icon)) problems.push('previews.icons: icon for “' + prefix + '” must be a kebab-case Lucide icon name.');
        else result.icons[normalized] = icon;
      });
    }
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

  function readLensLabel(raw, where, problems) {
    if (raw === undefined) return null;
    if (typeof raw !== 'string' || !raw.trim()) {
      problems.push(where + ': must be a nonempty string.');
      return null;
    }
    return raw.trim();
  }

  function pageLensLabel(raw, item, where, problems) {
    var label = readLensLabel(raw, where + ': lensLabel', problems);
    if (!label) return;
    if (isDocs(item.src)) {
      problems.push(where + ': lensLabel is only available for authored pages; docs lenses use implementation labels.');
      return;
    }
    item.lensLabel = label;
  }

  function authoredLensLabel(item) {
    return text(item && item.lensLabel) || (item && item.workbench ? 'Workbench' : 'Design');
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
      if (kind !== 'url' && kind !== 'storybook' && kind !== 'workbench' && kind !== 'docs' && kind !== 'ios-simulator' && kind !== 'window') {
        problems.push(where + ': kind must be url, storybook, workbench, docs, ios-simulator, or window.');
        return;
      }
      var impl = { key: key, label: text(spec.label) || labelOf(key), kind: kind, root: null };
      if (kind === 'workbench') {
        impl.root = '.';
      } else if (kind === 'docs') {
        /* A docs lens: the page's Markdown, its examples rendered with an
           adapter, like a preview. */
        var adapter = text(spec.adapter);
        if (!KEY.test(adapter)) {
          problems.push(where + ': needs an adapter — html, react, vue, react-native-web, or one registered in workbench.config.ts.');
          return;
        }
        impl.root = '.';
        impl.adapter = adapter;
        impl.styles = [];
        var styles = spec.styles === undefined || spec.styles === null || spec.styles === '' ? [] : isList(spec.styles) ? spec.styles : [spec.styles];
        styles.forEach(function (style) {
          var stylePath = projectPath(style);
          if (stylePath) impl.styles.push(stylePath);
          else problems.push(where + ': styles must be paths inside the project, relative to its root.');
        });
        if (spec.environment !== undefined && spec.environment !== null && spec.environment !== '') {
          var environment = projectPath(spec.environment);
          if (environment) impl.environment = environment;
          else problems.push(where + ': environment must be a path inside the project, relative to its root.');
        }
        ['base', 'url'].forEach(function (field) {
          if (spec[field] !== undefined) problems.push(where + ': ' + field + ' doesn’t apply to docs, whose examples Workbench compiles itself.');
        });
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
          if (imported.iconExplicit) impl.catalogIconExplicit = true;
        }
      }

      if (spec.render !== undefined) {
        problems.push(where + ': render is no longer used; remove it. URL and Storybook implementations use iframes.');
      }

      if (spec.start !== undefined) {
        if (kind === 'ios-simulator' || kind === 'window' || kind === 'workbench' || kind === 'docs') {
          problems.push(where + ': start is available for URL and Storybook implementations.');
        } else {
          impl.start = startup(spec.start, where, problems);
        }
      }

      var root = text(spec.root);
      if (root) {
        if ((kind === 'workbench' || kind === 'docs') && root !== '.') {
          problems.push(where + ': ' + (kind === 'workbench' ? 'Workbench previews' : 'Docs examples') + ' use this project root.');
          return;
        }
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

  /* Manual and imported collections share one list. Matching names are one
     collection, not duplicate buttons that both claim the same selection. */
  /* Match complete title segments; a more specific prefix wins. */
  function titleIcon(settings, title, fallback) {
    var prefix = Object.keys(settings.icons || {}).filter(function (prefix) {
      return title === prefix || title.indexOf(prefix + '/') === 0;
    }).sort(function (a, b) { return b.length - a.length; })[0];
    return { icon: prefix ? settings.icons[prefix] : settings.icon || fallback, mapped: !!prefix };
  }

  function mergeCollectionIcon(target, incoming) {
    // 7 authored, 6 preview map, 5 catalog map, 4 preview fallback,
    // 3 catalog fallback, 2 authored default, 1 preview default, 0 catalog default.
    var priority = target.iconPriority === undefined ? 7 : target.iconPriority;
    var next = incoming.iconPriority === undefined ? 7 : incoming.iconPriority;
    // Equal imported priorities use lexical icon order, independent of import timing.
    if (next > priority || (next === priority && next < 7 && incoming.icon < target.icon)) {
      target.icon = incoming.icon;
      target.iconPriority = next;
    }
  }

  function mergeCollections(base, imported) {
    function clone(items) { return (items || []).map(function (item) { return item.group ? Object.assign({}, item, { items: clone(item.items) }) : Object.assign({}, item); }); }
    function mergeItems(target, items) {
      items.forEach(function (item) {
        var group = item.group && target.find(function (candidate) { return candidate.group === item.group; });
        if (group) mergeItems(group.items, item.items || []);
        else target.push(item);
      });
      return target;
    }
    var out = (base || []).map(function (collection) {
      return Object.assign({}, collection, { items: clone(collection.items) });
    });
    function existing(src) {
      var found = null;
      function visit(items) { items.forEach(function (item) { if (item.group) visit(item.items); else if (item.src === src) found = item; }); }
      out.forEach(function (collection) { visit(collection.items); });
      return found;
    }
    (imported || []).forEach(function (collection) {
      function remaining(items) { return (items || []).map(function (item) {
        if (item.group) { var children = remaining(item.items); return children.length ? Object.assign({}, item, { items: children }) : null; }
        var original = item.workbench && existing(item.src);
        if (original) { original.states = item.states; original.workbench = true; if (!original.lensLabel && item.lensLabel) original.lensLabel = item.lensLabel; if (!original.icon && item.icon) original.icon = item.icon; return null; }
        return item;
      }).filter(Boolean); }
      var items = remaining(clone(collection.items));
      var found = out.find(function (candidate) { return candidate.name === collection.name; });
      if (found) { mergeCollectionIcon(found, collection); mergeItems(found.items, items); }
      else out.push(Object.assign({}, collection, { items: mergeItems([], items) }));
    });
    return out.filter(function (collection) { return collection.items.length; });
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

  /* The lenses one page has, checked against what's declared and against
     the page's own states. Answers with the map, or undefined when nothing
     survived — a page without lenses carries no key at all. */
  function pageLenses(raw, item, impls, where, problems) {
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

      /* A docs lens renders the page's Markdown, so only a page with
         Markdown has one. Call after pageMarkdown. */
      if (impl.kind === 'docs') {
        if (!item || !item.markdown) {
          problems.push(where + ': docs lens “' + key + '” needs the page’s Markdown — a .md src, or docs: with a .md file.');
          return;
        }
        var source = projectPath(value);
        if (!source) {
          problems.push(where + ': docs lens “' + key + '” needs an example folder (ending in /) or file, relative to the project root.');
          return;
        }
        out[key] = { examples: source };
        count += 1;
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
          where + ': implementation “' + key + '” needs a path, or a map of this page’s state ids to paths.'
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
            where + ': implementation “' + key + '” maps state “' + stateId + '”, which this page doesn’t declare.'
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

  /* The page's Markdown, which its docs lenses render: its src for a
     Markdown page, or `docs` for any other. Call before pageLenses. */
  function pageMarkdown(raw, item, where, problems) {
    var given = raw && raw.docs !== undefined && raw.docs !== null && raw.docs !== '';
    if (isDocs(item.src)) {
      item.markdown = item.src;
      if (given) problems.push(where + ': docs doesn’t apply to a Markdown page, which is its own docs.');
      return;
    }
    if (!given) return;
    var file = projectPath(raw.docs);
    if (!file || !isDocs(file) || srcProblem(file)) {
      problems.push(where + ': docs must be a .md file inside the project, relative to its root.');
      return;
    }
    item.markdown = file;
  }

  /* What only a Markdown page has: the docs lens it opens with, and no
     artboard sizes. A page with Markdown and no docs lens of its own gets
     the built-in Docs lens, keyed `docs`, so no other lens may take that
     key. Call after pageLenses. */
  function docsPageEntry(raw, item, impls, where, problems) {
    var lens = text(raw && raw.lens);
    var mapped = Object.keys(item.implementations || {});
    var docsKeys = mapped.filter(function (key) { return impls[key] && impls[key].kind === 'docs'; });
    if (item.markdown && !docsKeys.length && mapped.indexOf('docs') !== -1) {
      problems.push(where + ': “docs” names this page’s built-in Docs lens; give implementation “docs” another name.');
    }
    if (!isDocs(item.src)) {
      if (lens) problems.push(where + ': lens is only for Markdown pages (a .md src).');
      return;
    }
    if (raw && raw.sizes !== undefined && raw.sizes !== null && raw.sizes !== '') {
      problems.push(where + ': sizes don’t apply to a Markdown page, which uses the whole canvas.');
    }
    if (lens && docsKeys.indexOf(lens) === -1) {
      problems.push(where + ': lens “' + lens + '” isn’t one of this page’s docs lenses' + (docsKeys.length ? ' (' + docsKeys.join(', ') + ').' : '.'));
    } else if (lens) {
      item.lens = lens;
    }
    if (!item.lens && docsKeys.length) item.lens = docsKeys[0];
  }

  /* Where a page's implementation code lives, by implementation. Paths are
     relative to that implementation's root — or absolute, for a machine that
     keeps them somewhere else, which is what workbench.local.yaml is for. */
  function pageCode(raw, impls, where, problems) {
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

  /* How the space is marked in the space switcher: `color`, one of the
     named colours or a hex value, and `icon`, a Lucide icon name or an image
     file in the project. Both are optional; without them the mark is the
     name's first letter on a colour chosen from the space's root folder. A value
     that can't be used is named in `problems` and left out. */
  var SPACE_COLORS = ['blue', 'green', 'orange', 'purple', 'pink', 'teal', 'red', 'yellow', 'gray'];
  var HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;
  var IMAGE = /\.(?:svg|png|jpe?g|webp|gif)$/i;

  function spaceMark(raw, problems) {
    var out = { color: null, icon: null, image: null };
    raw = isMap(raw) ? raw : {};
    if (raw.color !== undefined && raw.color !== null && raw.color !== '') {
      var color = text(raw.color).toLowerCase();
      if (SPACE_COLORS.indexOf(color) > -1 || HEX.test(color)) out.color = color;
      else problems.push('color: must be one of ' + SPACE_COLORS.join(', ') + ', or a hex colour such as #2f7d55.');
    }
    if (raw.icon !== undefined && raw.icon !== null && raw.icon !== '') {
      var icon = text(raw.icon);
      if (KEY.test(icon)) out.icon = icon;
      else if (IMAGE.test(icon) && icon.charAt(0) !== '/' && icon.indexOf('..') === -1 && !SCHEME.test(icon) && icon.indexOf('\\') === -1) out.image = icon;
      else problems.push('icon: must be a kebab-case Lucide icon name, or a project-relative .svg, .png, .jpg, .webp, or .gif file.');
    }
    return out;
  }

  /* Several spaces in one file
     --------------------------
     A file may list spaces under `spaces`, keyed by kebab-case id. Each
     is a config of its own — `name`, `color`, `icon`, `collections`, `previews`,
     `implementations` — plus an optional `root`, the folder it serves and
     resolves its paths against, relative to the file's folder. Every other
     top-level key is shared: a space starts from the top level and its own
     keys replace those, with `implementations` merged by name. A space's
     `name`, `color` and `icon` are its own and never inherited; its name
     defaults to its id. A file without `spaces` is one space. */
  var SPACE_OWN = ['name', 'color', 'icon', 'root', 'sizes'];

  function spaceKeys(raw, problems) {
    if (!isMap(raw) || raw.spaces === undefined || raw.spaces === null) return [];
    if (!isMap(raw.spaces)) {
      problems.push('spaces: must be a map of space ids to spaces.');
      return [];
    }
    return Object.keys(raw.spaces).filter(function (key) {
      if (!KEY.test(key)) {
        problems.push('spaces: “' + key + '” must be a kebab-case id.');
        return false;
      }
      if (!isMap(raw.spaces[key])) {
        problems.push('spaces › ' + key + ': must be a map.');
        return false;
      }
      return true;
    });
  }

  /* The config one space sees, and where it lives: { raw, key, root }.
     `key` null, or one the file doesn't list, picks the first space; a
     file without spaces answers with itself. `root` is the space's own,
     as written, or null for the file's folder. */
  function selectSpace(raw, key, problems) {
    var keys = spaceKeys(raw, problems);
    if (!keys.length) {
      var whole = {};
      Object.keys(isMap(raw) ? raw : {}).forEach(function (name) { if (name !== 'spaces') whole[name] = raw[name]; });
      return { raw: whole, key: null, root: null };
    }
    var picked = keys.indexOf(key) > -1 ? key : keys[0];
    if (key && picked !== key) problems.push('spaces: there is no space “' + key + '”.');
    var shared = {};
    Object.keys(raw).forEach(function (name) {
      if (name !== 'spaces' && SPACE_OWN.indexOf(name) === -1) shared[name] = raw[name];
    });
    var entry = raw.spaces[picked];
    var own = {};
    Object.keys(entry).forEach(function (name) { if (name !== 'root') own[name] = entry[name]; });
    var effective = merge(shared, own);
    if (!text(effective.name)) effective.name = labelOf(picked);
    var root = entry.root === undefined || entry.root === null || entry.root === '' ? null : text(entry.root);
    if (entry.root !== undefined && entry.root !== null && entry.root !== '' && (!root || SCHEME.test(root))) {
      problems.push('spaces › ' + picked + ': root must be a folder path.');
      root = null;
    }
    return { raw: effective, key: picked, root: root };
  }

  return {
    merge: merge,
    streamed: streamed,
    spaceKeys: spaceKeys,
    selectSpace: selectSpace,
    spaceMark: spaceMark,
    SPACE_COLORS: SPACE_COLORS,
    implementations: implementations,
    previews: previews,
    pageLensLabel: pageLensLabel,
    authoredLensLabel: authoredLensLabel,
    pageLenses: pageLenses,
    pageMarkdown: pageMarkdown,
    docsPageEntry: docsPageEntry,
    isDocs: isDocs,
    srcProblem: srcProblem,
    projectPath: projectPath,
    pageCode: pageCode,
    labelOf: labelOf,
    storyState: storyState,
    address: address,
    mergeCollections: mergeCollections,
    titleIcon: titleIcon,
    mergeCollectionIcon: mergeCollectionIcon,
  };
});
