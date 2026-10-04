import { definePreview } from '@canonic2/workbench';
import { styles } from './fixtures';

export default definePreview({
  id: 'website/section-head',
  title: 'Components/Section head',
  adapter: 'astro',
  source: { entry: './SectionHead.example.astro' },
  styles,
  inputs: { kicker: 'Interface', title: 'Layout', lede: true },
  controls: { kicker: { type: 'text' }, title: { type: 'text' }, lede: { type: 'boolean' } },
  states: {
    default: { label: 'With a lede' },
    'no-lede': { label: 'Without a lede', inputs: { lede: false } },
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
