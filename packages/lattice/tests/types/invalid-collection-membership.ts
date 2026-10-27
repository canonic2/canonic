/** @file Consumer fixture: bulk insertion must preserve generic item inference. */
import { createCollection } from '@canonic2/lattice/collection';

const tasks = createCollection([['a', { label: 'A', done: false }]]);
tasks.updateMembership({ insert: [['b', { label: 42, done: false }]] });
