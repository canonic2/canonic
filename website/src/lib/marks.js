import workbench from '../../public/icons/workbench-blue.svg?raw';
import sandbox from '../../public/icons/sandbox-blue.svg?raw';
import playground from '../../public/icons/playground-blue.svg?raw';
import shield from '../../public/icons/shield-blue.svg?raw';
const productIcons = { workbench, sandbox, playground, shield };

/* The Canonic wordmark and the four product marks, as SVG paths from the
   design. Each mark has a base shape and a blue accent; `module` names the
   product's page when it has one. Keep the products in this order. */

export const logo = {
  viewBox: '-2 -12 193 42',
  base: 'M0 0L24 0L24 7L7 7L7 21L24 21L24 28L0 28ZM30 0L44 0L44 7L37 7L37 21L47 21L47 0L54 0L54 28L30 28ZM60 0L83.96 0L96.08 21L109 21L109 7L96.08 7L93.77 11L89.73 4L92.04 0L116 0L116 28L92.04 28L79.92 7L67 7L67 28L60 28ZM82.23 17L86.27 24L83.96 28L75.88 28ZM122 0L146 0L146 28L139 28L139 7L129 7L129 28L122 28ZM152 0L159 0L159 28L152 28ZM152 -10L159 -10L159 -3L152 -3ZM165 0L189 0L189 7L172 7L172 21L189 21L189 28L165 28Z',
  accent: 'M99 11L105 11L105 17L99 17Z',
};

// Read the supplied exports so the website always uses the stored artwork.
function productMark(name) {
  const svg = productIcons[name.toLowerCase()];
  const paths = [...svg.matchAll(/<path fill="([^"]+)" d="([^"]+)"\s*\/>/g)];
  if (paths.length !== 2) throw new Error(`Expected base and accent paths for ${name}`);
  return { base: paths[0][2], accent: paths[1][2] };
}

export const products = [
  { name: 'Workbench', tagline: 'Design tool', module: true },
  { name: 'Sandbox', tagline: 'QA', module: false },
  { name: 'Playground', tagline: 'API playground', module: false },
  { name: 'Shield', tagline: 'Agent safeguards & skills', module: false },
].map(product => ({ ...product, ...productMark(product.name) }));
