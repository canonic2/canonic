import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'website/shield',
  title: 'Shield/Overview',
  adapter: 'astro',
  source: { entry: '../src/pages/shield/index.astro' },
  links: {
    '/shield/': 'website/shield',
    '/shield/docs/': 'website/shield-docs',
    '/shield/docs/getting-started/': { preview: 'website/shield-docs', scenario: 'getting-started' },
    '/workbench/': 'website/overview',
  },
  sizes: ['laptop', 'mobile', 'resizable'],
});
