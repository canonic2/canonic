// Sample data for the component previews: a release's downloads, changelog
// entries, and docs navigation, shaped like what src/lib builds from GitHub
// and the guides.
import { TARGETS } from '../../src/lib/releases.js';

const repo = 'https://github.com/canonic2/canonic';

export const downloads = {
  version: '1.2.0',
  url: `${repo}/releases/tag/workbench/v1.2.0`,
  files: TARGETS.map(({ target, computer, short }) => ({
    target, computer, short,
    name: `canonic-workbench-${target}.vsix`,
    url: `${repo}/releases/download/workbench/v1.2.0/canonic-workbench-${target}.vsix`,
  })),
};

export const release = {
  version: '1.2.0',
  tag: 'workbench/v1.2.0',
  url: `${repo}/releases/tag/workbench/v1.2.0`,
  published: '2026-10-02T12:00:00Z',
  changes: `${repo}/compare/workbench/v1.1.0...workbench/v1.2.0`,
  notes: [{ items: ['Add a Reset button to preview controls.', 'Fix the zoom menu in narrow windows.'] }],
  files: downloads.files,
};

export const docGroups = [
  { title: 'Getting started', entries: [
    { id: 'index', body: '# Workbench documentation' },
    { id: 'getting-started', body: '# Getting started' },
    { id: 'canvas', body: '# Using the canvas' },
  ] },
  { title: 'Previews', entries: [
    { id: 'workbench-previews', body: '# TypeScript Workbench previews' },
    { id: 'astro', body: '# Astro previews' },
  ] },
];

// The docs search index the sidebar fetches, for the guides above.
export const searchIndex = docGroups.flatMap(group => group.entries).map(entry => ({
  id: entry.id,
  text: entry.body.replace(/^#\s+/, '').toLowerCase(),
}));

export const styles = ['../../src/styles/global.css'];

// A component's docs page has one lens: its folder of .astro examples, with
// the site's styles and environment.ts around them.
export const lens = (examples: string, extra: string[] = []) => ({
  astro: { label: 'Astro', adapter: 'astro', examples, styles: [...styles, ...extra], environment: './environment.ts' },
});
