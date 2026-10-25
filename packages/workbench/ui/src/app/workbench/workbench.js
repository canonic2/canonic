/* The canvas's components: the sidebar's space controls, the size switcher
   with its dialogs, and the Actions switch. The sidebar webview loads only
   its own entry and the space components. */
import { createStore, shallow } from '../../../../node_modules/@canonic2/lattice/src/index.js';
import {
  captureInputs,
  replayInputs,
  skipUpgradeAttribute,
  defineOnce,
} from '../../../../node_modules/@canonic2/lattice/src/modules/elements/elements.js';
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
