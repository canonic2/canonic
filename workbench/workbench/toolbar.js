/* Responsive top bar
   ------------------
   Keep where you are, the lens, and the viewport switch visible. The More
   menu always holds the project-wide actions; when the three toolbar regions
   would collide, the secondary view actions fold into it too, and on a very
   narrow bar the labels beside the breadcrumb and the Actions switch go. The
   menu's rows invoke the original controls. */
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
  var center = document.querySelector('.wb-topbar-center');
  var right = document.querySelector('.wb-topbar-right');
  var overflow = document.getElementById('toolbarOverflow');
  var button = document.getElementById('toolbarOverflowButton');
  var menu = document.getElementById('toolbarOverflowMenu');
  if (!topbar || !left || !center || !right || !overflow || !button || !menu) return;
  /* Configure and export only ever appear as rows of this menu. */
  var project = [document.getElementById('configureProject'), document.getElementById('exportProject')];

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
    if (!source || source.hidden || source.disabled) return false;
    if (source.localName === 'a' && !source.getAttribute('href')) return false;
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
    return true;
  }

  function heading(label) {
    var node = document.createElement('span');
    node.className = 'wb-toolbar-overflow-label';
    node.setAttribute('role', 'presentation');
    node.textContent = label;
    menu.appendChild(node);
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

  function projectActions() {
    return project.filter(function (source) { return source && !source.hidden; });
  }

  function rebuild() {
    menu.innerHTML = '';
    if (compact) {
      heading('View');
      row(document.getElementById('reload'));
      sourceRows();
      row(document.getElementById('copyReference'));
      row(document.getElementById('open'));
      var controls = document.getElementById('previewControls');
      if (controls && !controls.hidden) row(controls);
    }
    var project = projectActions();
    if (project.length) {
      if (compact) heading('Project');
      project.forEach(function (source) { row(source); });
    }
  }

  function close() {
    if (menu.hidden) return; /* no attribute writes, no mutation, no re-measure */
    menu.hidden = true;
    button.setAttribute('aria-expanded', 'false');
  }

  function open() {
    rebuild();
    menu.hidden = false;
    button.setAttribute('aria-expanded', 'true');
  }

  function contentWidth(container) {
    var children = Array.prototype.filter.call(container.children, function (child) {
      return root.getComputedStyle(child).display !== 'none';
    });
    var innerStyles = root.getComputedStyle(container);
    var innerGap = pixels(innerStyles.columnGap || innerStyles.gap);
    return children.reduce(function (sum, child) { return sum + child.getBoundingClientRect().width; }, 0) +
      Math.max(0, children.length - 1) * innerGap;
  }

  function measure() {
    scheduled = false;
    topbar.classList.remove('is-toolbar-compact', 'is-toolbar-tight');
    /* Measure the breadcrumb at its natural width: it ellipsizes to fit
       otherwise, and a clipped name would never ask for the room. */
    topbar.classList.add('is-toolbar-measuring');
    var styles = root.getComputedStyle(topbar);
    var available = topbar.clientWidth - pixels(styles.paddingLeft) - pixels(styles.paddingRight);
    var gap = pixels(styles.columnGap || styles.gap);
    compact = needsCompact(available, contentWidth(left), contentWidth(center), contentWidth(right), gap);
    topbar.classList.toggle('is-toolbar-compact', compact);
    if (compact && needsCompact(available, contentWidth(left), contentWidth(center), contentWidth(right), gap)) {
      topbar.classList.add('is-toolbar-tight');
    }
    topbar.classList.remove('is-toolbar-measuring');
    /* Nothing to offer: no project actions, and the bar has room. Assigned
       only on a change — the button sits in an observed region, and setting
       `hidden` again would queue another mutation and another measure. */
    var empty = !compact && !projectActions().length;
    if (overflow.hidden !== empty) overflow.hidden = empty;
    if (empty) close();
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

  var watch = { subtree: true, childList: true, attributes: true, characterData: true };
  if (root.ResizeObserver) new root.ResizeObserver(schedule).observe(topbar);
  [left, center, right].forEach(function (region) {
    new MutationObserver(function (records) {
      /* The menu's own rows are rebuilt on open; they don't move the bar. */
      if (records.every(function (r) { return menu.contains(r.target); })) return;
      schedule();
    }).observe(region, watch);
  });
  schedule();
  root.wbToolbar = { needsCompact: needsCompact, pixels: pixels, measure: measure };
})(typeof window === 'undefined' ? globalThis : window);
