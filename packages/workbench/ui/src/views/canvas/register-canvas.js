import { WbCanvas } from './components/canvas/element.js';
import { register } from '../../components/register-elements.js';

export function registerCanvasComponents(registry = customElements) {
  register(registry, [['wb-canvas', WbCanvas]]);
}
