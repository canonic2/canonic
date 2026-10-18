/* Lucide icon bridge. The pinned Lucide library ships locally with Workbench.
   Design examples import icons from the same dependency. */

/* Returns an <svg> that inherits color and sizes off the CSS below.
   Decorative by default — the control around it carries the label. */
window.wbIcon = function (name, size) {
  var icons = window.lucide && window.lucide.icons;
  if (!icons) throw new Error("Workbench could not load its bundled Lucide icon library.");

  var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('class', 'wb-icon');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', size || 16);
  svg.setAttribute('height', size || 16);
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  var exportName = String(name || '')
    .split('-')
    .filter(Boolean)
    .map(function (part) {
      return part.charAt(0).toUpperCase() + part.slice(1);
    })
    .join('');
  var nodes = icons[exportName];

  if (nodes) {
    nodes.forEach(function (node) {
      var child = document.createElementNS('http://www.w3.org/2000/svg', node[0]);
      Object.keys(node[1] || {}).forEach(function (attribute) {
        child.setAttribute(attribute, node[1][attribute]);
      });
      svg.appendChild(child);
    });
  }
  return svg;
};
