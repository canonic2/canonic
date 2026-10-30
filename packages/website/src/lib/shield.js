import { base } from './base.js';

export const github = 'https://github.com/canonic2/canonic';
export const pages = {
  overview: `${base}shield/`,
  docs: `${base}shield/docs/`,
  install: `${base}shield/#install`,
};
export const download = { href: pages.install, label: 'Build from source' };

export function shieldNav(current) {
  return [
    { href: pages.overview, label: 'Overview' },
    { href: pages.docs, label: 'Docs' },
    { href: pages.install, label: 'Install' },
    { href: `${github}/tree/main/packages/shield`, label: 'GitHub', external: true },
  ].map(item => ({ ...item, current: item.label === current }));
}
