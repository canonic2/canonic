import { WbSpaceMark } from '../components/space-mark/element.js';
import { WbSpaceSwitcher } from '../components/space-switcher/element.js';
import { WbSizeSwitcher } from '../components/size-switcher/element.js';
import { WbSizeDialog } from '../components/size-dialog/element.js';
import { WbSizesEditor } from '../components/sizes-editor/element.js';
import { WbActionsSwitch } from '../components/actions-switch/element.js';
import { WbCanvas } from '../components/canvas/element.js';
import { WbExportDialog } from '../components/export-dialog/element.js';

function register(registry, elements) {
  for (const [name, element] of elements) {
    const existing = registry.get(name);
    if (existing && existing !== element)
      throw new Error(`Incompatible component already registered: ${name}`);
    if (!existing) registry.define(name, element);
  }
}

export function registerSpaceComponents(registry = customElements) {
  register(registry, [
    ['wb-space-mark', WbSpaceMark],
    ['wb-space-switcher', WbSpaceSwitcher],
  ]);
}

/** The size switcher and its two dialogs, for the canvas's top bar. */
export function registerSizeComponents(registry = customElements) {
  register(registry, [
    ['wb-size-switcher', WbSizeSwitcher],
    ['wb-size-dialog', WbSizeDialog],
    ['wb-sizes-editor', WbSizesEditor],
  ]);
}

/** The Actions switch, beside the size switcher in the canvas's top bar. */
export function registerActionsComponents(registry = customElements) {
  register(registry, [['wb-actions-switch', WbActionsSwitch]]);
}

export function registerCanvasComponents(registry = customElements) {
  register(registry, [
    ['wb-canvas', WbCanvas],
    ['wb-export-dialog', WbExportDialog],
  ]);
}
