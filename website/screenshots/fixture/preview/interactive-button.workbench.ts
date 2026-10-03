import { definePreview } from '@canonic/workbench';
export default definePreview({
  id: 'components/interactive-button', title: 'Components/Interactive button',
  adapter: 'html', source: { entry: './interactive-button.ts' }, styles: ['../acme.css'],
  inputs: { label: 'Continue', variant: 'primary', disabled: false },
  controls: {
    label: { type: 'text' },
    variant: { type: 'select', options: ['primary', 'secondary', 'danger'] },
    disabled: { type: 'boolean' },
  },
  states: { default: { label: 'Primary' }, secondary: { inputs: { variant: 'secondary' } }, disabled: { inputs: { disabled: true } } },
});
