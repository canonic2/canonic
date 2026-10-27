/** @file Consumer type fixture: writing a key that is not in the state must fail. */
import { createStore } from '@canonic2/lattice';

const store = createStore({ count: 0 });
store.update({ missing: 1 });
