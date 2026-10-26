/** Retained native controls. No capability creation, global lookup or host work. */
export function createTopBarPresentation({ lensesBox, scenario, source, document }) {
  const lenses = new Map(),
    scenarios = new Map(),
    sources = new Map();
  const text = (node, value) => {
    if (node.textContent !== value) node.textContent = value;
  };
  const attribute = (node, name, value) => {
    if (node.getAttribute(name) !== value) node.setAttribute(name, value);
  };
  function reconcile(container, rows, cache, create, update) {
    const wanted = new Set(rows.map((row) => row.key));
    for (const [key, node] of cache)
      if (!wanted.has(key)) {
        node.remove();
        cache.delete(key);
      }
    for (const [at, row] of rows.entries()) {
      let node = cache.get(row.key);
      if (!node) {
        node = create(row);
        cache.set(row.key, node);
      }
      update(node, row);
      const before = container.children[at];
      if (before !== node) container.insertBefore(node, before || null);
    }
  }
  const button = () => {
    const node = document.createElement('button');
    node.type = 'button';
    return node;
  };
  return {
    lenses({ choices, showing }) {
      if (lensesBox.hidden !== choices.length < 2) lensesBox.hidden = choices.length < 2;
      reconcile(
        lensesBox,
        (choices.length < 2 ? [] : choices).map((row) => ({ ...row, key: row.key })),
        lenses,
        () => {
          const node = button();
          node.className = 'wb-lens';
          return node;
        },
        (node, row) => {
          if (node.dataset.lens !== row.key) node.dataset.lens = row.key;
          text(node, row.label);
          if (node.title !== row.title) node.title = row.title;
          attribute(node, 'aria-pressed', String(showing === row.key));
        },
      );
    },
    scenarios({ rows, kind }) {
      if (scenario.control.hidden !== !rows) scenario.control.hidden = !rows;
      const picked = rows?.find((row) => row.current) || rows?.[0];
      text(scenario.name, picked?.label || '');
      if (picked)
        scenario.button.title =
          (kind === 'story' ? 'Story: ' : 'Scenario: ') + picked.label + ' — pick another';
      else scenario.button.removeAttribute('title');
      attribute(scenario.menu, 'aria-label', kind === 'story' ? 'Story' : 'Scenario');
      reconcile(
        scenario.menu,
        (rows || []).map((row) => ({ ...row, key: row.id })),
        scenarios,
        () => {
          const node = button();
          node.className = 'wb-menu-row';
          node.tabIndex = -1;
          node.setAttribute('role', 'menuitem');
          return node;
        },
        (node, row) => {
          if (node.dataset.scenario !== (row.target || ''))
            node.dataset.scenario = row.target || '';
          text(node, row.label);
          attribute(node, 'aria-current', String(row === picked || row.id === picked?.id));
        },
      );
    },
    sources({ rows, available }) {
      if (source.control.hidden !== !available) source.control.hidden = !available;
      reconcile(
        source.menu,
        rows.map((row) => ({ ...row, key: row.id })),
        sources,
        () => {
          const node = button();
          node.className = 'wb-menu-row';
          node.tabIndex = -1;
          node.setAttribute('role', 'menuitem');
          const label = document.createTextNode(''),
            sub = document.createElement('span');
          sub.className = 'wb-menu-sub';
          node.append(label, sub);
          return node;
        },
        (node, row) => {
          if (node.dataset.source !== row.id) node.dataset.source = row.id;
          if (node.firstChild.nodeValue !== row.label) node.firstChild.nodeValue = row.label;
          text(node.lastChild, row.sub || '');
          if (node.disabled !== !row.enabled) node.disabled = !row.enabled;
          if (node.title !== row.title) node.title = row.title;
        },
      );
    },
  };
}
