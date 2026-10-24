/** @file Consumer type fixture: a run expecting a different type than read returns must fail. */
import { createStore } from '@canonic2/lattice';

const store = createStore({ count: 0 });
store.watch({ fields: ['count'], read: (state) => `${state.count}` }, (value: number) => {
  void value;
});
