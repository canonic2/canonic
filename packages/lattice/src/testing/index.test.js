/** @file Verifies the selection declaration check against its contract. */
import { expect, test } from 'vitest';
import { checkSelection } from './index.js';

/** @typedef {{ muted: boolean, unread: number, label: string }} State */

/** @type {Record<string, State>} */
const cases = {
  muted: { muted: true, unread: 3, label: 'Acme' },
  active: { muted: false, unread: 3, label: 'Acme' },
};

/** @type {import('../modules/store/store.js').Selection<State, string>} */
const status = {
  fields: ['muted', 'unread'],
  read: (state) => (state.muted ? 'Muted' : `${state.unread} unread`),
};

test('passes a selection that accesses only declared fields, reporting each case', () => {
  expect(checkSelection('status', status, cases)).toEqual({
    name: 'status',
    declared: ['muted', 'unread'],
    cases: [
      { name: 'muted', accessed: ['muted'] },
      { name: 'active', accessed: ['muted', 'unread'] },
    ],
    accessed: ['muted', 'unread'],
    unobserved: [],
  });
});

test('reports a declared field no case reached as a diagnostic, not a failure', () => {
  const result = checkSelection('status', status, { muted: /** @type {State} */ (cases.muted) });
  expect(result.unobserved).toEqual(['unread']);
  expect(result.accessed).toEqual(['muted']);
});

test('orders accessed and unobserved fields by declaration, not by access', () => {
  /** @type {import('../modules/store/store.js').Selection<State, string>} */
  const reversed = {
    fields: ['muted', 'unread', 'label'],
    read: (state) => `${state.label} ${state.unread}`,
  };
  const result = checkSelection('reversed', reversed, cases);
  expect(result.cases[0]).toEqual({ name: 'muted', accessed: ['unread', 'label'] });
  expect(result.accessed).toEqual(['unread', 'label']);
  expect(result.unobserved).toEqual(['muted']);
});

test('lists several unexpected fields alphabetically and reports the first failing case', () => {
  /** @type {import('../modules/store/store.js').Selection<State, string>} */
  const leaky = {
    fields: ['muted'],
    read: (state) => `${state.unread} ${state.label} ${String(state.muted)}`,
  };
  expect(() => checkSelection('leaky', leaky, cases)).toThrow(
    'Selection "leaky", case "muted" (declares muted): read accessed undeclared fields: label, unread.',
  );
});

test('judges unobserved fields across all cases together', () => {
  expect(checkSelection('status', status, cases).unobserved).toEqual([]);
});

test('fails an undeclared access in any case, naming selection, case, fields and declarations', () => {
  /** @type {import('../modules/store/store.js').Selection<State, string>} */
  const leaky = { fields: ['muted'], read: (state) => (state.muted ? 'Muted' : state.label) };
  expect(() => checkSelection('leaky', leaky, cases)).toThrow(
    'Selection "leaky", case "active" (declares muted): read accessed an undeclared field: label.',
  );
});

test('records destructuring and in as access', () => {
  /** @type {import('../modules/store/store.js').Selection<State, boolean>} */
  const destructured = {
    fields: ['muted'],
    read: ({ muted, unread }) => muted && unread > 0,
  };
  expect(() => checkSelection('destructured', destructured, cases)).toThrow(
    'undeclared field: unread',
  );
  /** @type {import('../modules/store/store.js').Selection<State, boolean>} */
  const tested = { fields: ['muted'], read: (state) => 'label' in state };
  expect(() => checkSelection('tested', tested, cases)).toThrow('undeclared field: label');
});

test('fails key enumeration and descriptor reads as unsupported, even when read catches the error', () => {
  /** @type {import('../modules/store/store.js').Selection<State, number>[]} */
  const unsupported = [
    { fields: ['muted'], read: (state) => Object.keys(state).length },
    { fields: ['muted'], read: (state) => ({ ...state }).unread },
    {
      fields: ['muted'],
      read: (state) => {
        for (const key in state) return key.length;
        return 0;
      },
    },
    {
      fields: ['muted'],
      read: (state) => (Object.getOwnPropertyDescriptor(state, 'muted') ? 1 : 0),
    },
    {
      fields: ['muted'],
      read: (state) => {
        try {
          return Object.keys(state).length;
        } catch {
          return 0;
        }
      },
    },
  ];
  for (const selection of unsupported) {
    expect(() => checkSelection('enumerating', selection, cases)).toThrow(/cannot follow/);
  }
});

test('fails a read that writes to the state', () => {
  /** @type {import('../modules/store/store.js').Selection<State, number>} */
  const writing = {
    fields: ['unread'],
    read: (state) => {
      /** @type {any} */ (state).unread = 0;
      return state.unread;
    },
  };
  expect(() => checkSelection('writing', writing, cases)).toThrow('writes to the state');
});

test('fails an access to a field the state does not have', () => {
  /** @type {import('../modules/store/store.js').Selection<State, unknown>} */
  const missing = { fields: ['muted'], read: (state) => /** @type {any} */ (state).missing };
  expect(() => checkSelection('missing', missing, cases)).toThrow('undeclared field: missing');
});

test('ignores inherited members and symbols', () => {
  /** @type {import('../modules/store/store.js').Selection<State, string>} */
  const formatted = {
    fields: ['unread'],
    read: (state) => state.unread.toString() + String(state.hasOwnProperty),
  };
  expect(checkSelection('formatted', formatted, cases).unobserved).toEqual([]);
});

test('wraps an error read throws with its case, and needs at least one case', () => {
  /** @type {import('../modules/store/store.js').Selection<State, string>} */
  const failing = {
    fields: ['label'],
    read: () => {
      throw new Error('boom');
    },
  };
  expect(() => checkSelection('failing', failing, cases)).toThrow(
    'Selection "failing", case "muted" (declares label): read threw.',
  );
  expect(() => checkSelection('status', status, {})).toThrow('at least one snapshot case');
});
