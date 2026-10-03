import { definePreview } from '@canonic/workbench';

export default definePreview({
  id: 'website/overview',
  title: 'Website/Pages/Overview',
  adapter: 'astro',
  source: { entry: '../src/pages/index.astro' },
  viewports: ['desktop', 'mobile', 'responsive'],
});
