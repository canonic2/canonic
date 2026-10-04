import { definePreview } from '@canonic/workbench';

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
    nav: [{ label: 'Overview', href: '/', current: true }],
  },
  links: { '/': 'website/overview' },
  controls: { title: { type: 'text' }, version: { type: 'text' }, nav: { type: 'json' } },
  states: {
    default: {},
    versioned: { inputs: { version: '1.0.0' } },
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
