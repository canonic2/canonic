import { WbSpaceMark } from './space-mark/element.ts';
import { WbSpaceSwitcher } from './space-switcher/element.ts';
import { WbSizeSwitcher } from './size-switcher/element.ts';
import { WbSizeDialog } from './size-dialog/element.ts';
import { WbSizesEditor } from './sizes-editor/element.ts';
import { WbActionsSwitch } from './actions-switch/element.ts';

function register(registry: CustomElementRegistry, elements: readonly (readonly [string, CustomElementConstructor])[]) {
  for (const [name, element] of elements) {
    const existing = registry.get(name);
    if (existing && existing !== element) throw new Error(`Incompatible component already registered: ${name}`);
    if (!existing) registry.define(name, element);
  }
}

export function registerSpaceComponents(registry = customElements) {
  register(registry, [['wb-space-mark', WbSpaceMark], ['wb-space-switcher', WbSpaceSwitcher]]);
}

/** The size switcher and its two dialogs, for the canvas's top bar. */
export function registerSizeComponents(registry = customElements) {
  register(registry, [['wb-size-switcher', WbSizeSwitcher], ['wb-size-dialog', WbSizeDialog], ['wb-sizes-editor', WbSizesEditor]]);
}

/** The Actions switch, beside the size switcher in the canvas's top bar. */
export function registerActionsComponents(registry = customElements) {
  register(registry, [['wb-actions-switch', WbActionsSwitch]]);
}
