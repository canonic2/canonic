/* Hands the size rules to the canvas's classic scripts. workbench.js waits
   for this module before it applies the config, since every page it shows
   needs a size; see specs/sizes.md. */

import {
  choose,
  describe,
  dimensionsText,
  supported,
} from '../../../contracts/src/sizes/browser/choice.js';
import {
  accepts,
  clamp,
  dimensions,
  fills,
} from '../../../contracts/src/sizes/browser/geometry.js';
import { defaultSizes } from '../../../contracts/src/sizes/browser/size.js';

const sizes = {
  defaultSizes,
  supported,
  choose,
  describe,
  dimensionsText,
  dimensions,
  fills,
  accepts,
  clamp,
};

window.wbSizes = sizes;
window.dispatchEvent(new Event('wb-sizes-ready'));
