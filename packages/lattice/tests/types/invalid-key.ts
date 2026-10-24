/** @file Consumer type fixture: writing a key that is not in the state must fail. */
import { createStore } from 'canonic-lattice';

const store = createStore({ count: 0 });
store.set({ missing: 1 });
