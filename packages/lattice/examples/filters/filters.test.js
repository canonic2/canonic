/** @file Checks the filters capability's selection declarations in Node. */
import { checkSelection } from '@canonic2/lattice/testing';
import { expect, test } from 'vitest';
import { createFilters } from './filters.js';

/** @type {Record<string, import('./filters.js').FiltersState>} */
const cases = {
  empty: { query: '', locked: false, revision: 0 },
  searching: { query: 'acme', locked: true, revision: 3 },
};

test('every selection reads exactly the fields it declares', () => {
  const { select } = createFilters({ save: () => {} });
  const results = [
    checkSelection('query', select.query, cases),
    checkSelection('locked', select.locked, cases),
    checkSelection('summary', select.summary, cases),
  ];
  for (const result of results) {
    expect(result.unobserved, `${result.name} has declared fields no case reached`).toEqual([]);
  }
});
