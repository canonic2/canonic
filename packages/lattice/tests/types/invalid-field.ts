/** @file Consumer type fixture: a misspelled selection field must fail. */
import type { Selection } from '@canonic2/lattice';

interface CounterState {
  count: number;
}

export const label = {
  fields: ['cuont'],
  read: (state) => `${state.count} clicks`,
} satisfies Selection<CounterState, string>;
