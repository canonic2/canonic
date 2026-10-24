/* The canvas's components: the sidebar's space controls, the size switcher
   with its dialogs, and the Actions switch. The sidebar webview loads only
   sidebar.js and the space components. */
import {
  registerActionsComponents,
  registerSizeComponents,
  registerSpaceComponents,
  registerCanvasComponents,
} from './register.js';
import { initialize } from './canvas.js';

registerSpaceComponents();
registerSizeComponents();
registerActionsComponents();
registerCanvasComponents();
initialize();
