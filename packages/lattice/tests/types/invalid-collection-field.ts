/** @file Consumer type fixture: an item selection naming a field the item lacks must fail. */
import type { ItemSelection } from '@canonic2/lattice/collection';

interface Task {
  label: string;
  done: boolean;
}

export const colour = {
  fields: ['colour'],
  read: (task) => task.label,
} satisfies ItemSelection<Task, string>;
