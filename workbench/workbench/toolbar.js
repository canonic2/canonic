/* Responsive top bar
   ------------------
   Keep the canvas context and viewport switch visible. When the three toolbar
   regions would collide, replace the middle tools and secondary right-side
   actions with a named overflow menu that invokes the original controls. */
(function (root) {
  function needsCompact(available, left, center, right, gap) {
    return 2 * Math.max(left, right) + center + 2 * gap > available;
  }

  function pixels(value) {
    var parsed = parseFloat(value);
    return Number.isFinite(parsed) ? parsed : 0;
  }

  if (!root.document) {
    root.wbToolbar = { needsCompact: needsCompact, pixels: pixels };
    return;
  }

  var document = root.document;
  var topbar = document.querySelector('.wb-topbar');
  var left = document.querySelector('.wb-topbar-left');
  var markup = document.querySelector('.wb-markup');
  var right = document.querySelector('.wb-topbar-right');
  var overflow = document.getElementById('toolbarOverflow');
  var button = document.getElementById('toolbarOverflowButton');
  var menu = document.getElementById('toolbarOverflowMenu');
  if (!topbar || !left || !markup || !right || !overflow || !button || !menu) return;

  var scheduled = false;
  var compact = false;

  function iconOf(source) {
    var icon = source && source.querySelector && source.querySelector('.wb-icon');
    return icon ? icon.cloneNode(true) : null;
  }

  function labelOf(source, fallback) {
    return fallback || source.getAttribute('aria-label') || source.title || source.textContent.trim();
  }

  function row(source, label, iconSource, sublabel) {
    if (!source || source.hidden || source.disabled) return;
    if (source.localName === 'a' && !source.getAttribute('href')) return;
    var proxy = document.createElement('button');
    proxy.className = 'wb-menu-row wb-toolbar-overflow-row';
    proxy.type = 'button';
    proxy.setAttribute('role', 'menuitem');
    var icon = iconOf(iconSource || source);
    if (icon) proxy.appendChild(icon);
    var copy = document.createElement('span');
    copy.className = 'wb-toolbar-overflow-copy';
    var text = document.createElement('span');
    text.textContent = labelOf(source, label);
    copy.appendChild(text);
    if (sublabel) {
      var sub = document.createElement('span');
      sub.className = 'wb-menu-sub';
      sub.textContent = sublabel;
      copy.appendChild(sub);
    }
    proxy.appendChild(copy);
    proxy.title = source.title || '';
    var current = source.getAttribute('aria-pressed') || source.getAttribute('aria-current');
    if (current != null) proxy.setAttribute('aria-current', current);
    proxy.addEventListener('click', function () {
      close();
      source.click();
    });
    menu.appendChild(proxy);
  }

  function heading(label) {
    var node = document.createElement('span');
    node.className = 'wb-toolbar-overflow-label';
    node.setAttribute('role', 'presentation');
    node.textContent = label;
    menu.appendChild(node);
  }

  function rowsFrom(selector, prefix) {
    Array.prototype.forEach.call(document.querySelectorAll(selector), function (source) {
      if (source.disabled) return;
      row(source, prefix + source.textContent.trim());
    });
  }

  function sourceRows() {
    Array.prototype.forEach.call(document.querySelectorAll('#sourceMenu .wb-menu-row'), function (source) {
      if (source.disabled) return;
      var sub = source.querySelector('.wb-menu-sub');
      var label = Array.prototype.map.call(source.childNodes, function (node) {
        return node.nodeType === 3 ? node.nodeValue : '';
      }).join('').trim();
      row(source, 'Open ' + label, null, sub && sub.textContent.trim());
    });
  }

  function rebuild() {
    menu.innerHTML = '';
    if (topbar.classList.contains('is-toolbar-tight')) {
      heading('Story');
      rowsFrom('#storyMenu .wb-menu-row', '');
    }
    heading('Markup');
    row(document.querySelector('[data-tool="pointer"]'));
    row(document.querySelector('[data-tool="draw"]'));
    row(document.querySelector('[data-tool="arrow"]'));
    rowsFrom('#shapeMenu [data-shape]', '');
    row(document.querySelector('[data-tool="text"]'));
    row(document.querySelector('[data-tool="comment"]'));
    row(document.getElementById('undo'));
    row(document.getElementById('clearMarkup'));
    row(document.getElementById('shoot'));

    var handoff = document.getElementById('handoff');
    if (handoff && !document.getElementById('handoffControl').hidden) {
      heading('Handoff');
      row(handoff);
    }

    heading('View');
    row(document.getElementById('reload'));
    sourceRows();
    row(document.getElementById('copyReference'));
    row(document.getElementById('configureProject'));
    row(document.getElementById('exportProject'));
    row(document.getElementById('open'));
  }

  function close() {
    menu.hidden = true;
    button.setAttribute('aria-expanded', 'false');
  }

  function open() {
    rebuild();
    menu.hidden = false;
    button.setAttribute('aria-expanded', 'true');
  }

  function measure() {
    scheduled = false;
    topbar.classList.remove('is-toolbar-compact', 'is-toolbar-tight');
    overflow.hidden = true;
    var styles = root.getComputedStyle(topbar);
    var available = topbar.clientWidth - pixels(styles.paddingLeft) - pixels(styles.paddingRight);
    var gap = pixels(styles.columnGap || styles.gap);
    function contentWidth(container) {
      var children = Array.prototype.filter.call(container.children, function (child) {
        return root.getComputedStyle(child).display !== 'none';
      });
      var innerStyles = root.getComputedStyle(container);
      var innerGap = pixels(innerStyles.columnGap || innerStyles.gap);
      return children.reduce(function (sum, child) { return sum + child.getBoundingClientRect().width; }, 0) +
        Math.max(0, children.length - 1) * innerGap;
    }
    compact = needsCompact(available, contentWidth(left), contentWidth(markup), contentWidth(right), gap);
    topbar.classList.toggle('is-toolbar-compact', compact);
    overflow.hidden = !compact;
    if (compact && needsCompact(available, contentWidth(left), contentWidth(overflow), contentWidth(right), gap)) {
      topbar.classList.add('is-toolbar-tight');
    }
    if (!compact) close();
  }

  function schedule() {
    if (scheduled) return;
    scheduled = true;
    root.requestAnimationFrame(measure);
  }

  button.addEventListener('click', function () {
    if (menu.hidden) open(); else close();
  });
  document.addEventListener('pointerdown', function (event) {
    if (!menu.hidden && !event.target.closest('.wb-toolbar-overflow')) close();
  });
  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape') close();
  });

  if (root.ResizeObserver) new root.ResizeObserver(schedule).observe(topbar);
  new MutationObserver(schedule).observe(left, { subtree: true, childList: true, attributes: true, characterData: true });
  new MutationObserver(schedule).observe(right, { subtree: true, childList: true, attributes: true, characterData: true });
  schedule();
  root.wbToolbar = { needsCompact: needsCompact, pixels: pixels, measure: measure };
})(typeof window === 'undefined' ? globalThis : window);
