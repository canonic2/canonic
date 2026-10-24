/** @file Consumer type fixture: a read returning a different type than the annotation must fail. */
import type { Selection } from '@canonic2/lattice';

interface CounterState {
  count: number;
}

export const label = {
  fields: ['count'],
  read: (state) => state.count,
} satisfies Selection<CounterState, string>;
