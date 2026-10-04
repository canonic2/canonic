import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'website/install',
  title: 'Pages/Install',
  adapter: 'astro',
  source: { entry: '../src/pages/workbench/install.astro' },
  links: {
    '/': 'website/overview',
    '/workbench/install/': 'website/install',
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
