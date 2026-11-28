import { base } from './base.js';

export const overview = `${base}testrun/`;
export const docs = `${overview}docs/`;
export const download = { href: `${overview}#install`, label: 'Install' };

export function testrunNav(current, hasDocs = true) {
  return [
    { href: overview, label: 'Overview', current: current === 'Overview' },
    ...(hasDocs ? [{ href: docs, label: 'Docs', current: current === 'Docs' }] : []),
    ...(current === 'Overview' ? [download] : []),
  ];
}
