import { WbSpaceMark } from '../components/space-mark/element.js';
import { WbSpaceSwitcher } from '../components/space-switcher/element.js';
import { register } from './register-elements.js';

export function registerSpaceComponents(registry = customElements) {
  register(registry, [
    ['wb-space-mark', WbSpaceMark],
    ['wb-space-switcher', WbSpaceSwitcher],
  ]);
}
