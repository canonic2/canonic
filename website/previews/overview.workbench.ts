import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'website/overview',
  title: 'Pages/Overview',
  adapter: 'astro',
  source: { entry: '../src/pages/workbench/index.astro' },
  links: {
    '/workbench/': 'website/overview',
    '/workbench/install/': 'website/install',
    '/workbench/changelog/': 'website/changelog',
    '/workbench/docs/': 'website/docs',
  },
  sizes: ['laptop', 'mobile', 'resizable'],
});
