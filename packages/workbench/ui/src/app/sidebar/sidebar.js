import { registerSpaceComponents } from '../../components/register-space.js';
import { createStore, shallow } from '../../../../node_modules/@canonic2/lattice/src/index.js';
import {
  captureInputs,
  replayInputs,
  skipUpgradeAttribute,
  defineOnce,
} from '../../../../node_modules/@canonic2/lattice/src/modules/elements/elements.js';
void [createStore, shallow, captureInputs, replayInputs, skipUpgradeAttribute, defineOnce];

registerSpaceComponents();
window.wbProjectSidebar();
