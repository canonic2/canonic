import { definePreview } from '@canonic/workbench';

export default definePreview({
  id: 'website/install',
  title: 'Website/Pages/Install',
  adapter: 'astro',
  source: { entry: '../src/pages/workbench/install.astro' },
  viewports: ['desktop', 'mobile', 'responsive'],
});
