import { definePreview } from '@canonic2/workbench';
import { docGroups, searchIndex, styles, withScripts } from './fixtures';

// Search fetches the docs' index; here it answers with the sample guides.
export default definePreview({
  id: 'website/docs-sidebar',
  title: 'Components/Docs sidebar',
  adapter: 'astro',
  source: { entry: '../../src/components/DocsSidebar.astro' },
  styles: [...styles, '../../src/styles/docs.css'],
  setup: withScripts,
  inputs: { groups: docGroups, current: 'getting-started' },
  controls: { current: { type: 'select', options: docGroups.flatMap(group => group.entries.map(entry => entry.id)) } },
  requests: { 'GET /workbench/docs/search.json': { body: searchIndex } },
  states: {
    default: { label: 'Getting started open' },
    unavailable: { label: 'Search unavailable', requests: { 'GET /workbench/docs/search.json': { status: 503 } } },
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
