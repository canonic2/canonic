import { createMenuBehavior } from '../../components/shared/menu-behavior.js';

export function needsCompact(available, left, center, right, gap) {
  return 2 * Math.max(left, right) + center + 2 * gap > available;
}

export function pixels(value) {
  var parsed = parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function bindResponsiveTopBar(
  { topbar, left, center, right, overflow, button, menu, sourceMenu, controls },
  { signal: connection },
) {
  if (connection.aborted) return;
  const failed = new AbortController(),
    signal = AbortSignal.any([connection, failed.signal]);
  const document = topbar.ownerDocument,
    root = document.defaultView;
  const control = (id) => controls[id];
  const space = [controls.configureSpace, controls.exportSpace];
  var scheduled = 0;
  var compact = false;

  function iconOf(source) {
    var icon = source && source.querySelector && source.querySelector('.wb-icon');
    return icon ? icon.cloneNode(true) : null;
  }

  function labelOf(source, fallback) {
    return (
      fallback || source.getAttribute('aria-label') || source.title || source.textContent.trim()
    );
  }

  function row(source, label, iconSource, sublabel) {
    if (!source || source.hidden || source.disabled) return false;
    if (source.localName === 'a' && !source.getAttribute('href')) return false;
    var proxy = document.createElement('button');
    proxy.className = 'wb-menu-row wb-topbar-overflow-row';
    proxy.type = 'button';
    proxy.setAttribute('role', 'menuitem');
    var icon = iconOf(iconSource || source);
    if (icon) proxy.appendChild(icon);
    var copy = document.createElement('span');
    copy.className = 'wb-topbar-overflow-copy';
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
      close(true);
      source.click();
    });
    menu.appendChild(proxy);
    return true;
  }

  function heading(label) {
    var node = document.createElement('span');
    node.className = 'wb-topbar-overflow-label';
    node.setAttribute('role', 'presentation');
    node.textContent = label;
    menu.appendChild(node);
  }

  function sourceRows() {
    Array.prototype.forEach.call(sourceMenu.querySelectorAll('.wb-menu-row'), function (source) {
      if (source.disabled) return;
      var sub = source.querySelector('.wb-menu-sub');
      var label = Array.prototype.map
        .call(source.childNodes, function (node) {
          return node.nodeType === 3 ? node.nodeValue : '';
        })
        .join('')
        .trim();
      row(source, 'Open ' + label, null, sub && sub.textContent.trim());
    });
  }

  function spaceActions() {
    return space.filter(function (source) {
      return source && !source.hidden;
    });
  }

  function rebuild() {
    menu.innerHTML = '';
    if (compact) {
      heading('View');
      row(control('reload'));
      sourceRows();
      row(control('copyReference'));
      row(control('open'));
      var controls = control('previewControls');
      if (controls && !controls.hidden) row(controls);
    }
    var actions = spaceActions();
    if (actions.length) {
      if (compact) heading('Space');
      actions.forEach(function (source) {
        row(source);
      });
    }
  }

  function close(focus = false) {
    if (menu.hidden) return; /* no attribute writes, no mutation, no re-measure */
    interaction.setOpen(false, false, focus);
  }

  function contentWidth(container) {
    var children = Array.prototype.filter.call(container.children, function (child) {
      return root.getComputedStyle(child).display !== 'none';
    });
    var innerStyles = root.getComputedStyle(container);
    var innerGap = pixels(innerStyles.columnGap || innerStyles.gap);
    return (
      children.reduce(function (sum, child) {
        return sum + child.getBoundingClientRect().width;
      }, 0) +
      Math.max(0, children.length - 1) * innerGap
    );
  }

  function measure() {
    if (signal.aborted) return;
    scheduled = 0;
    topbar.classList.remove('is-topbar-compact', 'is-topbar-tight');
    /* Measure the breadcrumb at its natural width: it ellipsizes to fit
       otherwise, and a clipped name would never ask for the room. */
    topbar.classList.add('is-topbar-measuring');
    var styles = root.getComputedStyle(topbar);
    var available = topbar.clientWidth - pixels(styles.paddingLeft) - pixels(styles.paddingRight);
    var gap = pixels(styles.columnGap || styles.gap);
    compact = needsCompact(
      available,
      contentWidth(left),
      contentWidth(center),
      contentWidth(right),
      gap,
    );
    topbar.classList.toggle('is-topbar-compact', compact);
    if (
      compact &&
      needsCompact(available, contentWidth(left), contentWidth(center), contentWidth(right), gap)
    ) {
      topbar.classList.add('is-topbar-tight');
    }
    topbar.classList.remove('is-topbar-measuring');
    /* Nothing to offer: no space actions, and the bar has room. Assigned
       only on a change — the button sits in an observed region, and setting
       `hidden` again would queue another mutation and another measure. */
    var empty = !compact && !spaceActions().length;
    if (overflow.hidden !== empty) overflow.hidden = empty;
    if (empty) close();
  }

  function schedule() {
    if (signal.aborted || scheduled) return;
    scheduled = root.requestAnimationFrame(measure);
  }

  const interaction = createMenuBehavior({
    host: overflow,
    root: document,
    trigger: button,
    menu,
    disabled: () => button.disabled,
    items: () => [...menu.querySelectorAll('button')].filter((node) => !node.disabled),
    preferred: () => menu.querySelector('[aria-current=true]'),
    toggled() {},
  });
  button.addEventListener(
    'keydown',
    (event) => {
      if (!interaction.open && ['ArrowDown', 'ArrowUp'].includes(event.key)) rebuild();
    },
    { signal },
  );
  let resize;
  const observers = [];
  signal.addEventListener(
    'abort',
    () => {
      interaction.setOpen(false, false, menu.contains(document.activeElement));
      resize?.disconnect();
      observers.forEach((observer) => observer.disconnect());
      if (scheduled) root.cancelAnimationFrame(scheduled);
    },
    { once: true },
  );
  try {
    interaction.connect(signal);
    button.addEventListener(
      'click',
      function (event) {
        if (interaction.open) interaction.setOpen(false, true, event.detail === 0);
        else {
          rebuild();
          interaction.setOpen(true, true, event.detail === 0);
        }
      },
      { signal },
    );
    var watch = { subtree: true, childList: true, attributes: true, characterData: true };
    resize = new root.ResizeObserver(schedule);
    resize.observe(topbar);
    [left, center, right].forEach(function (region) {
      const observer = new root.MutationObserver(function (records) {
        /* The menu's own rows are rebuilt on open; they don't move the bar. */
        if (
          records.every(function (r) {
            return menu.contains(r.target);
          })
        )
          return;
        schedule();
      });
      observers.push(observer);
      observer.observe(region, watch);
    });
    schedule();
    return { measure };
  } catch (error) {
    failed.abort();
    throw error;
  }
}
