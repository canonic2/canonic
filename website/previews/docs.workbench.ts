import { definePreview } from '@canonic2/workbench';
import { docGroups } from '../src/lib/doc-groups.js';

// One state per guide in the docs sidebar, in its order. workbench.config.ts
// renders the guides; docs-page.astro shows the one a state's `id` names.
const ids = docGroups.flatMap(group => group.ids);
const label = (id: string) => id === 'index' ? 'Overview' : id[0].toUpperCase() + id.slice(1).replace(/-/g, ' ');
const route = (id: string) => id === 'index' ? '/workbench/docs/' : `/workbench/docs/${id}/`;

export default definePreview({
  id: 'website/docs',
  title: 'Pages/Docs',
  adapter: 'astro',
  source: { entry: './docs-page.astro' },
  states: Object.fromEntries(ids.map(id => [id, { label: label(id), inputs: { id } }])),
  links: {
    '/': 'website/overview',
    '/workbench/install/': 'website/install',
    '/workbench/changelog/': 'website/changelog',
    ...Object.fromEntries(ids.map(id => [route(id), { preview: 'website/docs', state: id }])),
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
