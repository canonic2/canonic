import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'website/overview',
  title: 'Pages/Overview',
  adapter: 'astro',
  source: { entry: '../src/pages/index.astro' },
  links: {
    '/': 'website/overview',
    '/workbench/install/': 'website/install',
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
