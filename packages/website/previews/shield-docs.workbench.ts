import { definePreview } from '@canonic2/workbench';
import { shieldDocGroups } from '../src/lib/doc-groups.js';

const ids = shieldDocGroups.flatMap(group => group.ids);
const route = (id: string) => id === 'index' ? '/shield/docs/' : `/shield/docs/${id}/`;

export default definePreview({
  id: 'website/shield-docs',
  title: 'Shield/Docs',
  adapter: 'astro',
  source: { entry: './docs-page.astro' },
  scenarios: Object.fromEntries(ids.map(id => [id, {
    label: id === 'index' ? 'Overview' : id[0].toUpperCase() + id.slice(1).replace(/-/g, ' '),
    inputs: { id, product: 'Shield' },
  }])),
  links: {
    '/shield/': 'website/shield',
    '/workbench/': 'website/overview',
    ...Object.fromEntries(ids.map(id => [route(id), { preview: 'website/shield-docs', scenario: id }])),
  },
  sizes: ['laptop', 'mobile', 'resizable'],
});
