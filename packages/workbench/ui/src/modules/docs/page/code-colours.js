// Paint token ranges over the original text. No syntax spans are inserted, so
// text shaping, character positions, selection and copy remain unchanged.
let sequence = 0;
const groups = {
  comment: ['comment', 'quote'],
  keyword: ['keyword', 'selector-tag', 'literal', 'type'],
  string: ['string', 'regexp', 'addition'],
  value: ['number', 'attr', 'attribute', 'variable', 'template-variable', 'meta'],
  title: ['title', 'section'],
  name: ['name', 'tag', 'built_in', 'selector-class', 'selector-id'],
  deletion: ['deletion'],
};
const roles = new Map(
  Object.entries(groups).flatMap(([group, names]) => names.map((name) => ['hljs-' + name, group])),
);

export function createCodeColours(document) {
  const view = document.defaultView;
  const registry = view.CSS?.highlights;
  const prefix = 'wb-code-' + ++sequence + '-';
  const highlights = new Map();
  const records = new Map();
  let sheet;
  function clear(element) {
    for (const [role, range] of records.get(element) || []) highlights.get(role)?.delete(range);
    records.delete(element);
  }
  return {
    apply(element, text, html) {
      if (
        !registry ||
        !view.Highlight ||
        !element.isConnected ||
        element.textContent !== text ||
        typeof html !== 'string'
      )
        return;
      const template = document.createElement('template');
      template.innerHTML = html;
      if (template.content.textContent !== text) return;
      const original = document.createTreeWalker(element, 4);
      const nodes = [];
      let node,
        offset = 0;
      while ((node = original.nextNode())) {
        nodes.push({ node, start: offset, end: offset + node.length });
        offset += node.length;
      }
      const tokens = document.createTreeWalker(template.content, 4);
      const ranges = [];
      offset = 0;
      let cursor = 0;
      while ((node = tokens.nextNode())) {
        let role;
        for (let parent = node.parentElement; parent && !role; parent = parent.parentElement)
          for (const name of parent.classList)
            if (roles.has(name)) {
              role = roles.get(name);
              break;
            }
        const end = offset + node.length;
        if (role && node.length) {
          while (cursor < nodes.length && nodes[cursor].end <= offset) cursor++;
          let last = cursor;
          while (last < nodes.length && nodes[last].end < end) last++;
          if (!nodes[cursor] || !nodes[last]) return;
          const range = document.createRange();
          range.setStart(nodes[cursor].node, offset - nodes[cursor].start);
          range.setEnd(nodes[last].node, end - nodes[last].start);
          ranges.push([role, range]);
        }
        offset = end;
      }
      if (!sheet) {
        sheet = new view.CSSStyleSheet();
        sheet.replaceSync(
          Object.keys(groups)
            .map(
              (role) => `::highlight(${prefix + role}) { color: var(--wb-docs-syntax-${role}); }`,
            )
            .join('\n'),
        );
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      }
      clear(element);
      for (const [role, range] of ranges) {
        if (!highlights.has(role)) {
          const highlight = new view.Highlight();
          highlights.set(role, highlight);
          registry.set(prefix + role, highlight);
        }
        highlights.get(role).add(range);
      }
      records.set(element, ranges);
    },
    reset() {
      for (const highlight of highlights.values()) highlight.clear();
      records.clear();
    },
    dispose() {
      for (const role of highlights.keys()) registry.delete(prefix + role);
      records.clear();
      highlights.clear();
      if (sheet)
        document.adoptedStyleSheets = document.adoptedStyleSheets.filter(
          (value) => value !== sheet,
        );
    },
  };
}
