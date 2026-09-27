/* Transfer the live document, not an image and not computed styles per node.
   The receiver keeps an inert DOM: application code runs only in the preview.
   Shared by the visible workbench, the capture surface, and native smoke tests. */
(function (root) {
  var ID = 'data-wb-mirror';
  var HTML = 'http://www.w3.org/1999/xhtml';

  function create(doc, changed) {
    var ids = new WeakMap();
    var cached = new WeakMap();
    var touched = new WeakSet();
    var reattached = new WeakSet();
    var next = 0;
    var sequence = 0;
    var latest;
    var acknowledged;
    var snapshots = new WeakMap();
    var observers = new Map();
    var stopped = false;
    var win = doc.defaultView;
    var session = Date.now().toString(36) + '-' + Math.random().toString(36).slice(2);
    function id(node) {
      if (!ids.has(node)) ids.set(node, String(++next));
      return ids.get(node);
    }
    function notify(records) {
      if (Array.isArray(records)) records.forEach(mark);
      if (!stopped && changed) changed();
    }
    function mark(record) {
      touched.add(record.target);
      Array.from(record.addedNodes || []).forEach(function (node) { reattached.add(node); });
    }
    function observe(tree) {
      if (observers.has(tree)) return;
      var observer = new win.MutationObserver(notify);
      observer.observe(tree, { subtree: true, childList: true, attributes: true, characterData: true });
      observers.set(tree, observer);
      tree.addEventListener('scroll', notify, true);
    }
    var events = ['input', 'change', 'click', 'keydown', 'toggle', 'focusin', 'focusout', 'load'];
    observe(doc);
    events.forEach(function (event) { doc.addEventListener(event, notify, true); });

    function equal(a, b) { return a === b || JSON.stringify(a) === JSON.stringify(b); }
    function sameIds(a, b) { return a === b || (!!a && !!b && a.length === b.length && a.every(function (key, index) { return key === b[index]; })); }
    function wire(data, base) {
      var result = { version: 1, revision: data.revision, base: base ? base.revision : null,
        root: data.root, nodes: [], removed: [], states: [], cleared: [], defined: data.defined, animations: data.animations };
      if (!base) {
        result.nodes = Array.from(data.nodes.values()); result.states = Array.from(data.states.values());
      } else {
        data.nodeVersions.forEach(function (version, key) {
          if (version > base.sequence) {
            if (data.nodes.has(key)) result.nodes.push(data.nodes.get(key)); else result.removed.push(key);
          }
        });
        data.stateVersions.forEach(function (version, key) {
          if (version > base.sequence) {
            if (data.states.has(key)) result.states.push(data.states.get(key)); else result.cleared.push(key);
          }
        });
      }
      snapshots.set(result, data);
      return result;
    }
    function read() {
      observers.forEach(function (observer) {
        observer.takeRecords().forEach(mark);
      });
      var nodes = new Map();
      var states = new Map();
      var roots = [doc];
      var defined = new Set();
      function keep(record) {
        var previous = latest && latest.nodes.get(record.id);
        if (previous && equal(previous, record)) record = previous;
        nodes.set(record.id, record);
        return record.id;
      }
      function sheets(tree, destination) {
        Array.from(tree.adoptedStyleSheets || []).forEach(function (sheet, index) {
          var key = id(tree) + '-sheet-' + index;
          var text = Array.from(sheet.cssRules).map(function (rule) { return rule.cssText; }).join('\n');
          var child = keep({ id: key + '-text', type: 3, text: text });
          destination.push(keep({ id: key, type: 1, tag: 'style', ns: HTML,
            attrs: sheet.disabled ? [['media', 'not all', null]] : [], children: [child] }));
        });
      }
      function visit(node, refresh) {
        if (![1, 3, 8].includes(node.nodeType)) return null;
        refresh = refresh || reattached.has(node);
        var key = id(node);
        var previous = cached.get(node);
        if (node.nodeType !== 1) {
          var textRecord = { id: key, type: node.nodeType, text: node.nodeValue };
          if (previous && previous.text === textRecord.text) textRecord = previous;
          cached.set(node, textRecord); nodes.set(key, textRecord); return key;
        }
        var tag = node.localName;
        if (tag === 'script' || tag === 'base' || (tag === 'meta' && node.hasAttribute('http-equiv'))) return null;
        if (tag === 'iframe' || tag === 'object' || tag === 'embed') {
          throw new Error('Live capture cannot yet mirror nested frames or embedded documents');
        }
        var attrs = previous && !refresh && !touched.has(node) ? previous.sourceAttrs :
          Array.from(node.attributes).filter(function (attribute) {
            return !/^on/i.test(attribute.name) && attribute.name !== 'autofocus' && attribute.name !== ID;
          }).map(function (attribute) { return [attribute.name, attribute.value, attribute.namespaceURI || null]; });
        var rendered = attrs;
        function set(name, value) {
          rendered = rendered.filter(function (attribute) { return attribute[0] !== name; });
          if (value != null) rendered.push([name, value, null]);
        }
        if (tag.indexOf('-') !== -1 && win.customElements.get(tag)) defined.add(tag);
        if (node.sheet && node.sheet.disabled) set('media', 'not all');
        var state = { id: key };
        if (node === node.getRootNode().activeElement && tag !== 'body' && tag !== 'html' &&
            (!node.shadowRoot || !node.shadowRoot.activeElement)) state.focused = true;
        if (tag === 'input' && node.type !== 'file') {
          state.value = node.value; state.checked = node.checked; state.indeterminate = node.indeterminate;
        }
        if (tag === 'textarea' || tag === 'select') state.value = node.value;
        if (tag === 'option') state.selected = node.selected;
        if (node.scrollLeft || node.scrollTop) state.scroll = [node.scrollLeft, node.scrollTop];
        if (tag === 'img' && node.currentSrc) {
          set('src', node.currentSrc); set('srcset', null); set('sizes', null); set('loading', 'eager');
        }
        if (tag === 'canvas' && node.width && node.height) state.bitmap = node.toDataURL('image/png');
        if (tag === 'video' && node.readyState >= 2) {
          var canvas = doc.createElement('canvas');
          canvas.width = node.videoWidth; canvas.height = node.videoHeight;
          canvas.getContext('2d').drawImage(node, 0, 0);
          state.poster = canvas.toDataURL('image/png');
          set('src', null); set('autoplay', null);
        }
        if (tag === 'dialog') { state.modal = node.matches(':modal'); state.open = node.open; }
        if (node.hasAttribute('popover')) state.popover = node.matches(':popover-open');
        if (Object.keys(state).length > 1) {
          var oldState = latest && latest.states.get(key);
          states.set(key, oldState && equal(state, oldState) ? oldState : state);
        }
        var children = [];
        if (tag === 'head') children.push(keep({ id: 'base', type: 1, tag: 'base', ns: HTML,
          attrs: [['href', doc.baseURI, null]], children: [] }));
        if (tag === 'style' && node.sheet) {
          children.push(keep({ id: key + '-css', type: 3,
            text: Array.from(node.sheet.cssRules).map(function (rule) { return rule.cssText; }).join('\n') }));
        } else {
          Array.from(tag === 'template' ? node.content.childNodes : node.childNodes).forEach(function (child) {
            var childId = visit(child, refresh || tag === 'template'); if (childId) children.push(childId);
          });
        }
        if (tag === 'head') sheets(doc, children);
        var shadow;
        if (node.shadowRoot) {
          roots.push(node.shadowRoot); observe(node.shadowRoot);
          shadow = [];
          Array.from(node.shadowRoot.childNodes).forEach(function (child) {
            var childId = visit(child, refresh); if (childId) shadow.push(childId);
          });
          sheets(node.shadowRoot, shadow);
        }
        var record = { id: key, type: 1, tag: tag, ns: node.namespaceURI, attrs: rendered, children: children };
        if (shadow) record.shadow = shadow;
        var oldRecord = previous && previous.record;
        if (oldRecord && equal(attrs, previous.sourceAttrs) &&
            sameIds(oldRecord.children, children) && sameIds(oldRecord.shadow, shadow) && equal(oldRecord.attrs, rendered)) record = oldRecord;
        nodes.set(key, record); cached.set(node, { sourceAttrs: attrs, record: record });
        return key;
      }
      var rootId = visit(doc.documentElement);
      var animations = [];
      var seenAnimations = new Set();
      roots.forEach(function (tree) {
        if (!tree.getAnimations) return;
        tree.getAnimations().forEach(function (animation) {
          if (seenAnimations.has(animation)) return;
          seenAnimations.add(animation);
          var effect = animation.effect;
          if (!effect || !effect.target || !ids.has(effect.target) || animation.currentTime == null) return;
          var timing = effect.getTiming();
          if (timing.iterations === Infinity) timing.iterations = -1;
          animations.push({ id: id(effect.target), frames: effect.getKeyframes(), timing: timing,
            time: Number(animation.currentTime), pseudo: effect.pseudoElement || null });
        });
      });
      observers.forEach(function (observer, tree) {
        if (tree.host && !tree.host.isConnected) {
          observer.disconnect(); tree.removeEventListener('scroll', notify, true); observers.delete(tree);
        }
      });
      var data = { nodes: nodes, states: states, root: rootId, defined: Array.from(defined), animations: animations };
      function sameMap(a, b) {
        if (a.size !== b.size) return false;
        for (var entry of a) if (b.get(entry[0]) !== entry[1]) return false;
        return true;
      }
      if (latest && sameMap(nodes, latest.nodes) && sameMap(states, latest.states) &&
          equal(data.defined, latest.defined) && equal(animations, latest.animations)) data = latest;
      else {
        data.sequence = ++sequence; data.revision = session + '-' + sequence;
        function versions(current, before, priorVersions) {
          var result = new Map(priorVersions);
          current.forEach(function (value, key) { if (!before || before.get(key) !== value) result.set(key, sequence); });
          if (before) before.forEach(function (_, key) { if (!current.has(key)) result.set(key, sequence); });
          // Retain deletion tombstones until the receiver acknowledges them.
          result.forEach(function (version, key) {
            if (!current.has(key) && acknowledged && version <= acknowledged.sequence) result.delete(key);
          });
          return result;
        }
        data.nodeVersions = versions(nodes, latest && latest.nodes, latest && latest.nodeVersions);
        data.stateVersions = versions(states, latest && latest.states, latest && latest.stateVersions);
        latest = data;
      }
      touched = new WeakSet();
      reattached = new WeakSet();
      return wire(data, acknowledged);
    }
    return { read: read,
      acknowledge: function (snapshot) {
        var data = snapshots.get(snapshot);
        if (data && (!acknowledged || data.sequence > acknowledged.sequence)) acknowledged = data;
      },
      full: function (snapshot) {
        var data = snapshots.get(snapshot);
        if (!data) throw new Error('Unknown mirror snapshot');
        return wire(data, null);
      },
      stop: function () {
        stopped = true;
        observers.forEach(function (observer, tree) { observer.disconnect(); tree.removeEventListener('scroll', notify, true); });
        observers.clear();
        events.forEach(function (event) { doc.removeEventListener(event, notify, true); });
      }
    };
  }

  function attributes(source, target) {
    Array.from(target.attributes).forEach(function (attribute) {
      if (!source.hasAttribute(attribute.name)) target.removeAttribute(attribute.name);
    });
    Array.from(source.attributes).forEach(function (attribute) {
      if (target.getAttribute(attribute.name) !== attribute.value) {
        if (attribute.namespaceURI) target.setAttributeNS(attribute.namespaceURI, attribute.name, attribute.value);
        else target.setAttribute(attribute.name, attribute.value);
      }
    });
  }

  // Preserve unchanged elements, stylesheets and decoded images between updates.
  function reconcile(source, target, doc, nodes) {
    var existing = new Map();
    Array.from(target.childNodes).forEach(function (node) {
      if (node.nodeType === 1 && node.hasAttribute(ID)) existing.set(node.getAttribute(ID), node);
    });
    var cursor = target.firstChild;
    Array.from(source.childNodes).forEach(function (wanted) {
      if (wanted.nodeType === 1 && wanted.localName === 'template' && wanted.hasAttribute('shadowrootmode')) {
        var shadow = target.shadowRoot || target.attachShadow({ mode: 'open' });
        reconcile(wanted.content, shadow, doc, nodes);
        return;
      }
      var key = wanted.nodeType === 1 && wanted.getAttribute(ID);
      var actual = key ? existing.get(key) : cursor;
      if (!actual || actual.nodeType !== wanted.nodeType || actual.nodeName !== wanted.nodeName ||
          (actual.nodeType === 1 && actual.getAttribute(ID) !== wanted.getAttribute(ID))) {
        actual = doc.importNode(wanted, false);
      }
      if (actual !== cursor) target.insertBefore(actual, cursor);
      if (wanted.nodeType === 1) {
        attributes(wanted, actual);
        if (key) nodes.set(key, actual);
        if (wanted.localName === 'template') reconcile(wanted.content, actual.content, doc, nodes);
        else reconcile(wanted, actual, doc, nodes);
      } else if (actual.nodeValue !== wanted.nodeValue) actual.nodeValue = wanted.nodeValue;
      cursor = actual.nextSibling;
    });
    while (cursor) { var next = cursor.nextSibling; cursor.remove(); cursor = next; }
  }

  var receivers = new WeakMap();

  function patchDocument(doc, snapshot) {
    var receiver = receivers.get(doc);
    function version(revision) {
      var split = (revision || '').lastIndexOf('-');
      return { session: (revision || '').slice(0, split), sequence: Number((revision || '').slice(split + 1)) };
    }
    if (snapshot.base) {
      var base = version(snapshot.base); var current = version(receiver && receiver.revision); var target = version(snapshot.revision);
      if (!receiver || current.session !== base.session || target.session !== base.session ||
          current.sequence < base.sequence || current.sequence > target.sequence) throw new Error('MIRROR_RESYNC: missing base revision');
    }
    if (!snapshot.base) receiver = { nodes: new Map(), states: new Map() };
    receivers.set(doc, receiver);
    var nodes = receiver.nodes;
    snapshot.nodes.forEach(function (record) {
      var node = nodes.get(record.id);
      if (!node) {
        node = record.id === snapshot.root ? doc.documentElement :
          record.type === 1 ? doc.createElementNS(record.ns, record.tag) :
          record.type === 3 ? doc.createTextNode(record.text) : doc.createComment(record.text);
        nodes.set(record.id, node);
      }
      if (record.type === 1) {
        var wanted = new Set([ID]);
        record.attrs.forEach(function (attribute) {
          if (/^on/i.test(attribute[0]) || attribute[0] === 'autofocus') return;
          wanted.add(attribute[0]);
          if (node.getAttribute(attribute[0]) !== attribute[1]) {
            if (attribute[2]) node.setAttributeNS(attribute[2], attribute[0], attribute[1]);
            else node.setAttribute(attribute[0], attribute[1]);
          }
        });
        Array.from(node.attributes).forEach(function (attribute) { if (!wanted.has(attribute.name)) node.removeAttribute(attribute.name); });
        node.setAttribute(ID, record.id);
      } else if (node.nodeValue !== record.text) node.nodeValue = record.text;
    });
    // Detach moves first, including ancestor/descendant reversals. Applying
    // parent lists in an arbitrary patch order must not create a temporary cycle.
    snapshot.nodes.forEach(function (record) {
      if (record.type !== 1) return;
      var node = nodes.get(record.id);
      function detach(parent, ids) {
        ids.forEach(function (key) {
          var child = nodes.get(key);
          if (child && child.parentNode && child.parentNode !== parent) child.remove();
        });
      }
      detach(record.tag === 'template' ? node.content : node, record.children);
      if (record.shadow) detach(node.shadowRoot || node.attachShadow({ mode: 'open' }), record.shadow);
    });
    function children(parent, ids) {
      var cursor = parent.firstChild;
      ids.forEach(function (key) {
        var node = nodes.get(key);
        if (!node) throw new Error('MIRROR_RESYNC: missing child');
        if (node !== cursor) parent.insertBefore(node, cursor);
        cursor = node.nextSibling;
      });
      while (cursor) { var next = cursor.nextSibling; cursor.remove(); cursor = next; }
    }
    snapshot.nodes.forEach(function (record) {
      if (record.type !== 1) return;
      var node = nodes.get(record.id);
      children(record.tag === 'template' ? node.content : node, record.children);
      if (record.shadow) children(node.shadowRoot || node.attachShadow({ mode: 'open' }), record.shadow);
    });
    snapshot.removed.forEach(function (key) { var node = nodes.get(key); if (node) node.remove(); nodes.delete(key); });
    snapshot.cleared.forEach(function (key) { receiver.states.delete(key); });
    snapshot.states.forEach(function (state) { receiver.states.set(state.id, state); });
    var elements = new Map();
    nodes.forEach(function (node, key) { if (node.nodeType === 1) elements.set(key, node); });
    return { nodes: elements, states: Array.from(receiver.states.values()) };
  }

  async function applyState(doc, snapshot) {
    var win = doc.defaultView;
    var stateChanges = snapshot.states || [];
    var scrollIds = new Set();
    var previousReceiver = receivers.get(doc);
    if (previousReceiver) previousReceiver.states.forEach(function (state) { if (state.scroll) scrollIds.add(state.id); });
    var parsed = snapshot.version === 1 ? null : new win.DOMParser().parseFromString(snapshot.html, 'text/html');
    // The receiver is sandboxed without allow-scripts. Still remove scripts
    // defensively, including ones inside declarative shadow templates.
    function strip(tree) {
      tree.querySelectorAll('script,meta[http-equiv]').forEach(function (node) { node.remove(); });
      tree.querySelectorAll('template').forEach(function (template) { strip(template.content); });
    }
    if (parsed) strip(parsed);
    (snapshot.defined || []).forEach(function (name) {
      if (!win.customElements.get(name)) win.customElements.define(name, class extends win.HTMLElement {});
    });
    var nodes = new Map();
    if (snapshot.version === 1) {
      var patched = patchDocument(doc, snapshot);
      nodes = patched.nodes;
      snapshot = Object.assign({}, snapshot, { states: patched.states });
    } else {
      attributes(parsed.documentElement, doc.documentElement);
      nodes.set(parsed.documentElement.getAttribute(ID), doc.documentElement);
      reconcile(parsed.documentElement, doc.documentElement, doc, nodes);
    }
    var pending = [];
    nodes.forEach(function (node) {
      if (node.localName === 'link' && node.rel === 'stylesheet' && !node.disabled &&
          win.matchMedia(node.media || 'all').matches && !node.sheet) {
        pending.push(new Promise(function (resolve, reject) {
          var timer = setTimeout(function () { clean(); reject(new Error('Mirror stylesheet did not load')); }, 5000);
          function clean() { clearTimeout(timer); node.removeEventListener('load', done); node.removeEventListener('error', done); }
          function done() { clean(); resolve(); }
          node.addEventListener('load', done); node.addEventListener('error', done);
        }));
      }
    });
    await Promise.all(pending);
    var focused = (snapshot.states || []).filter(function (state) { return state.focused; }).pop();
    var active = doc.activeElement;
    while (active && active.shadowRoot && active.shadowRoot.activeElement) active = active.shadowRoot.activeElement;
    if (active && active !== nodes.get(focused && focused.id) && active.blur) active.blur();
    stateChanges.forEach(function (state) {
      var node = nodes.get(state.id); if (!node) return;
      ['value', 'checked', 'indeterminate', 'selected'].forEach(function (key) {
        if (key in state) node[key] = state[key];
      });
      if (state.poster) node.poster = state.poster;
      if (state.bitmap) pending.push(new Promise(function (resolve, reject) {
        var image = new win.Image();
        image.onload = function () {
          var context = node.getContext('2d');
          context.clearRect(0, 0, node.width, node.height); context.drawImage(image, 0, 0); resolve();
        };
        image.onerror = function () { reject(new Error('Canvas state could not be mirrored')); };
        image.src = state.bitmap;
      }));
      if (state.modal && !node.matches(':modal')) { node.removeAttribute('open'); node.showModal(); }
      if (state.modal === false && node.matches(':modal')) {
        // Reconciliation may already have removed open. close() needs it to
        // clear the dialog's modal/top-layer state as well as the attribute.
        node.setAttribute('open', ''); node.close();
        if (state.open) node.setAttribute('open', '');
      }
      if (state.popover && !node.matches(':popover-open')) node.showPopover();
      if (!state.popover && node.matches(':popover-open')) node.hidePopover();
      if (state.focused) node.focus({ preventScroll: true });
    });
    if (focused && nodes.has(focused.id)) nodes.get(focused.id).focus({ preventScroll: true });
    await Promise.all(pending);
    if (doc.fonts) await doc.fonts.ready;
    function scroll() {
      var positions = new Map();
      (snapshot.states || []).forEach(function (state) {
        if (state.scroll) { positions.set(state.id, state.scroll); scrollIds.add(state.id); }
      });
      var changes = [];
      // Read first, then write only differences. Resetting every scroller to
      // zero dirties layout between reads and needlessly moves settled views.
      scrollIds.forEach(function (id) {
        var node = nodes.get(id); if (!node) return;
        var position = positions.get(id) || [0, 0];
        if (node.scrollLeft !== position[0] || node.scrollTop !== position[1]) changes.push([node, position]);
      });
      changes.forEach(function (change) { change[0].scrollLeft = change[1][0]; change[0].scrollTop = change[1][1]; });
    }
    scroll();
    var images = Array.from(nodes.values()).filter(function (node) {
      if (node.localName !== 'img') return false;
      var box = node.getBoundingClientRect();
      return box.width && box.height && box.right > 0 && box.bottom > 0 && box.left < win.innerWidth && box.top < win.innerHeight;
    });
    await Promise.all(images.map(function (image) { return image.decode().catch(function () {}); }));
    scroll();
    // Preserve the visible animation phase rather than starting it again.
    var trees = [doc]; nodes.forEach(function (node) { if (node.shadowRoot) trees.push(node.shadowRoot); });
    trees.forEach(function (tree) { if (tree.getAnimations) tree.getAnimations().forEach(function (animation) { animation.cancel(); }); });
    (snapshot.animations || []).forEach(function (state) {
      var node = nodes.get(state.id); if (!node) return;
      var timing = Object.assign({}, state.timing);
      if (timing.iterations === -1) timing.iterations = Infinity;
      if (state.pseudo) timing.pseudoElement = state.pseudo;
      var animation = node.animate(state.frames, timing);
      animation.pause(); animation.currentTime = state.time;
    });
    return { nodes: nodes.size };
  }

  async function apply(doc, snapshot) {
    var receiver = receivers.get(doc);
    if (snapshot.version === 1 && receiver && receiver.revision === snapshot.revision) return { nodes: receiver.nodes.size };
    try {
      var result = await applyState(doc, snapshot);
      if (snapshot.version === 1) receivers.get(doc).revision = snapshot.revision;
      return result;
    } catch (error) {
      receivers.delete(doc);
      throw error;
    }
  }

  root.wbDOMMirror = { create: create, apply: apply };
})(typeof window === 'undefined' ? globalThis : window);
