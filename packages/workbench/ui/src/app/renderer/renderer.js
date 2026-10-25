/** Composes one content viewport. Never imports the Workbench application bootstrap. */
import { createStore, shallow } from '../../../../node_modules/@canonic2/lattice/src/index.js';
import {
  captureInputs,
  replayInputs,
  skipUpgradeAttribute,
  defineOnce,
} from '../../../../node_modules/@canonic2/lattice/src/modules/elements/elements.js';
void [createStore, shallow, captureInputs, replayInputs, skipUpgradeAttribute, defineOnce];
import { createContentController } from '../../modules/preview-sessions/content.js';

import { initializeRenderer } from '../../modules/preview-sessions/renderer-bridge.js';

import { connectStoryCatalog } from '../../modules/preview-sessions/story-catalog.js';

const services = window;
const params = new URLSearchParams(location.search);
const channel = params.get('artboard-runtime');
const origin = params.get('canvas-origin');
if (!channel || !origin || window.parent === window)
  throw new Error('Content renderer requires an owning canvas.');
const catalog = connectStoryCatalog(window.parent, origin, channel);
const surface = document.getElementById('artboard');
const viewport = document.getElementById('artboardContent');
const rootMeta = document.querySelector('meta[name="canonic-root"]')?.content;
const frames = services.wbContentFrames.create({
  surface,
  viewport,
  frame: document.getElementById('frame'),
  buffer: document.getElementById('frameBuffer'),
  changed() {
    controller.settled();
  },
  failed(error, view) {
    controller.renderFailed(view, error);
  },
  reveal() {
    controller.reveal();
  },
});
const controller = createContentController({
  root: rootMeta ? new URL(rootMeta, location.href).href : new URL('/', location.href).href,
  surface,
  viewport,
  frames,
  services: {
    mergeCollections: services.wbManifest.mergeCollections,
    address: services.wbAddress,
    lenses: services.wbLenses,
    native: services.wbSimulator,
  },
  stories: catalog.stories,
  async loadConfig() {
    const response = await fetch('/_workbench/config');
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || 'Could not load project data.');
    return result;
  },
  held: () => !!ports?.held,
});
const annotations = services.wbCreateAnnotations({
  surface,
  view: controller.view,
  target: controller.target,
  item: controller.item,
  size: controller.size,
  docs: controller.docs,
});
const ports = {
  content: controller.content,
  annotations,
  view: controller.view,
  reference: services.wbReference.text,
  keyboard(event) {
    const lens = controller.view()?.lens;
    return lens?.kind === 'storybook' && lens.url
      ? services.wbKeys.storybook(event, frames.frame(), lens.url)
      : null;
  },
};
services.wbKeys.relay(window.parent);
initializeRenderer(ports);
window.addEventListener('wb-frame-change', () => controller.settled());
window.addEventListener(
  'pagehide',
  () => {
    catalog.dispose();
    controller.dispose();
  },
  { once: true },
);
controller.start();
