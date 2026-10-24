import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'website/shell',
  title: 'Components/Page shell',
  adapter: 'astro',
  source: { entry: '../src/layouts/Base.astro' },
  inputs: {
    title: 'Acme · Workbench',
    description: 'A page using the website layout.',
    product: 'Workbench',
    version: '',
    nav: [{ label: 'Overview', href: '/workbench/', current: true }],
  },
  links: { '/workbench/': 'website/overview' },
  controls: { title: { type: 'text' }, version: { type: 'text' }, nav: { type: 'json' } },
  scenarios: {
    default: {},
    versioned: { inputs: { version: '1.0.0' } },
  },
  sizes: ['laptop', 'mobile', 'resizable'],
});
