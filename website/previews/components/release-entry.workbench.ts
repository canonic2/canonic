import { definePreview } from '@canonic2/workbench';
import { release, styles, withScripts } from './fixtures';

export default definePreview({
  id: 'website/release-entry',
  title: 'Components/Release entry',
  adapter: 'astro',
  source: { entry: './ReleaseEntry.example.astro' },
  styles,
  setup: withScripts,
  inputs: { release, latest: true },
  controls: { latest: { type: 'boolean' }, release: { type: 'json' } },
  states: {
    default: { label: 'Latest, with builds' },
    'notes-only': { label: 'Without builds', inputs: { latest: false, release: { ...release, version: '1.1.0', tag: 'workbench/v1.1.0', files: [] } } },
    'no-notes': { label: 'Without notes', inputs: { latest: false, release: { ...release, version: '1.0.0', tag: 'workbench/v1.0.0', notes: [], files: [] } } },
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
