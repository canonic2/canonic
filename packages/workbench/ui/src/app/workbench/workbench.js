/* The canvas's components: the sidebar's space controls, the size switcher
   with its dialogs, and the Actions switch. The sidebar webview loads only
   its own entry and the space components. */
import { createStore, shallow } from '@canonic2/lattice';
import {
  captureInputs,
  replayInputs,
  skipUpgradeAttribute,
  defineOnce,
} from '@canonic2/lattice/elements';
void [createStore, shallow, captureInputs, replayInputs, skipUpgradeAttribute, defineOnce];
import { registerSpaceComponents } from '../../components/register-space.js';
import { registerSizeComponents } from '../../views/top-bar/register-sizes.js';
import { registerActionsComponents } from '../../views/top-bar/register-actions.js';
import { registerCanvasComponents } from '../../views/canvas/register-canvas.js';
import { registerExportComponents } from '../../views/export/register-export.js';
import { initialize } from '../../views/canvas/bindings.js';

registerSpaceComponents();
registerSizeComponents();
registerActionsComponents();
registerCanvasComponents();
registerExportComponents();
initialize();
