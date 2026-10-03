import workbench from '../../public/icons/workbench-blue.svg?raw';
import sandbox from '../../public/icons/sandbox-blue.svg?raw';
import playground from '../../public/icons/playground-blue.svg?raw';
import shield from '../../public/icons/shield-blue.svg?raw';
import lockup from '../../public/logos/canonic-lockup.svg?raw';
const productIcons = { workbench, sandbox, playground, shield };

/* The Canonic lockup and the four product marks, as SVG artwork from the
   design. Each mark has a base shape and a blue accent; `module` names the
   product's page when it has one. Keep the products in this order. */

// Preserve the lockup's group transforms when adapting its colors to the theme.
export const logo = {
  viewBox: /viewBox="([^"]+)"/.exec(lockup)[1],
  body: lockup.replace(/^[\s\S]*?<svg\b[^>]*>/, '').replace(/<\/svg>\s*$/, '')
    .replace(/fill="#141414"/g, 'fill="currentColor"')
    .replace(/fill="#2457FF"/g, 'class="accent"'),
};

// Read the supplied exports so the website always uses the stored artwork.
function mark(svg, name) {
  const paths = [...svg.matchAll(/<path fill="([^"]+)" d="([^"]+)"\s*\/>/g)];
  if (paths.length !== 2) throw new Error(`Expected base and accent paths for ${name}`);
  const viewBox = /viewBox="([^"]+)"/.exec(svg)?.[1];
  if (!viewBox) throw new Error(`Expected a viewBox for ${name}`);
  return { viewBox, base: paths[0][2], accent: paths[1][2] };
}

export const products = [
  { name: 'Workbench', tagline: 'Design tool', module: true },
  { name: 'Sandbox', tagline: 'QA', module: false },
  { name: 'Playground', tagline: 'API playground', module: false },
  { name: 'Shield', tagline: 'Agent safeguards & skills', module: false },
].map(product => ({ ...product, ...mark(productIcons[product.name.toLowerCase()], product.name) }));
