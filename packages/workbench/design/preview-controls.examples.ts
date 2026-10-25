import { registerPreviewControls } from '../ui/src/components/preview-controls/register.js';
import type { WbPreviewControls } from '../ui/src/components/preview-controls/element.js';
registerPreviewControls();

const model = {
  id: 'acme', scenario: 'default', docs: 'Inputs are temporary experiments.', actions: [] as { name: string; values: unknown[] }[],
  controls: { label: { type: 'text' }, count: { type: 'number', min: 0, step: 1 },
    enabled: { type: 'boolean' }, tone: { type: 'select', options: ['info', 'warning'] },
    data: { type: 'json' } },
  inputs: { label: 'Continue', count: 2, enabled: true, tone: 'info', data: { title: 'Acme' } },
};
function show(canvas: HTMLElement, value: typeof model) {
  const panel = document.createElement('wb-preview-controls') as WbPreviewControls;
  panel.style.position = 'static'; panel.style.maxHeight = 'none'; panel.model = value;
  panel.addEventListener('wb-preview-input', event => {
    const { detail } = event as CustomEvent;
    panel.model = { ...panel.model, inputs: { ...panel.model.inputs, [detail.name]: detail.value } };
  });
  panel.addEventListener('wb-preview-reset', () => { panel.model = value; });
  canvas.append(panel);
}
export const inputs = (canvas: HTMLElement) => show(canvas, model);
export const actionLog = (canvas: HTMLElement) => show(canvas, { ...model,
  actions: [{ name: 'Continue', values: ['Acme'] }] });
