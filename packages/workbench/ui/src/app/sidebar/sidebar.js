import { registerSpaceComponents } from '../../components/register-space.js';
import { createStore, shallow } from '@canonic2/lattice';
import {
  captureInputs,
  replayInputs,
  skipUpgradeAttribute,
  defineOnce,
} from '@canonic2/lattice/elements';
void [createStore, shallow, captureInputs, replayInputs, skipUpgradeAttribute, defineOnce];

registerSpaceComponents();
window.wbProjectSidebar();
