/* The canvas's components: the sidebar's space controls, the size switcher
   with its dialogs, and the Actions switch. The sidebar webview loads only
   its own entry and the space components. */
import {
  registerActionsComponents,
  registerSizeComponents,
  registerSpaceComponents,
  registerCanvasComponents,
} from '../../bootstrap/register.js';
import { initialize } from '../../bootstrap/canvas.js';

registerSpaceComponents();
registerSizeComponents();
registerActionsComponents();
registerCanvasComponents();
initialize();
