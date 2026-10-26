/**
 * @file Packs Lattice, installs the archive into an isolated consumer outside
 * the package, imports every entry point there and runs the consumer type
 * fixtures from `tests/types/` against the installed package.
 */
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, test } from 'vitest';

const root = fileURLToPath(new URL('../', import.meta.url));
const templates = join(root, 'tests', 'types');
const tsc = join(root, 'node_modules', 'typescript', 'bin', 'tsc');

/** @type {string} */
let workspace;
/** @type {string} */
let consumer;
/** @type {string[]} */
let archived;

/**
 * Runs a command without a shell and fails with its output when it fails.
 * @param {string} command The executable.
 * @param {string[]} args Its arguments.
 * @param {string} cwd The working directory.
 * @returns {string} Its standard output.
 */
function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', timeout: 60000 });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed:\n${result.stdout}${result.stderr}`);
  }
  return result.stdout;
}

/**
 * Type-checks one fixture in the consumer with a configuration written for it.
 * @param {string} file The fixture file name in `tests/types/`.
 * @param {'dom' | 'node'} environment The DOM library, or Node types without it.
 * @returns {{ status: number | null, output: string }} The checker's exit status and diagnostics.
 */
function typecheck(file, environment) {
  const config = join(consumer, `tsconfig.${file}.${environment}.json`);
  const compilerOptions = {
    strict: true,
    noEmit: true,
    target: 'ES2024',
    module: 'NodeNext',
    moduleResolution: 'NodeNext',
    skipLibCheck: true,
    // Lattice ships JavaScript with JSDoc types; consumers read them this way.
    // The depth must reach the modules behind each entry point.
    allowJs: true,
    maxNodeModuleJsDepth: 2,
    ...(environment === 'dom'
      ? { lib: ['ES2024', 'DOM', 'DOM.Iterable'], types: [] }
      : { lib: ['ES2024'], types: ['node'], typeRoots: [join(root, 'node_modules', '@types')] }),
  };
  writeFileSync(config, JSON.stringify({ compilerOptions, files: [file] }));
  const result = spawnSync(process.execPath, [tsc, '--project', config], {
    cwd: consumer,
    encoding: 'utf8',
    timeout: 60000,
  });
  if (result.error) throw result.error;
  return { status: result.status, output: result.stdout + result.stderr };
}

beforeAll(() => {
  workspace = mkdtempSync(join(tmpdir(), 'lattice-consumer-'));
  const packed = JSON.parse(run('npm', ['pack', '--json', '--pack-destination', workspace], root));
  const archive = join(workspace, packed[0].filename);
  archived = run('tar', ['-tzf', archive], workspace).trim().split('\n');

  consumer = join(workspace, 'acme-consumer');
  const installed = join(consumer, 'node_modules', '@canonic2/lattice');
  mkdirSync(installed, { recursive: true });
  run('tar', ['-xzf', archive, '-C', installed, '--strip-components=1'], workspace);
  writeFileSync(
    join(consumer, 'package.json'),
    JSON.stringify({ name: 'acme-consumer', private: true, type: 'module' }),
  );
  for (const file of readdirSync(templates)) copyFileSync(join(templates, file), join(consumer, file));
}, 120000);

afterAll(() => {
  if (workspace) rmSync(workspace, { recursive: true, force: true });
});

describe('packed archive', () => {
  test('contains runtime source and no tests or development files', () => {
    expect(archived).toContain('package/src/index.js');
    expect(archived).toContain('package/src/modules/store/store.js');
    expect(archived).toContain('package/src/modules/elements/elements.js');
    expect(archived).toContain('package/src/modules/collection/collection.js');
    expect(archived).not.toContain('package/src/modules/collection/experimental.js');
    expect(archived).toContain('package/src/report.js');
    expect(archived.filter((file) => file.endsWith('.test.js'))).toEqual([]);
    expect(archived.filter((file) => file.startsWith('package/types/'))).toEqual([]);
    expect(archived.filter((file) => file.startsWith('package/tests/'))).toEqual([]);
  });

  test('imports every entry point in the consumer and refuses internal paths', () => {
    writeFileSync(
      join(consumer, 'check.js'),
      `import * as root from '@canonic2/lattice';
import * as collection from '@canonic2/lattice/collection';
import * as elements from '@canonic2/lattice/elements';
import * as testing from '@canonic2/lattice/testing';
const store = root.createStore({ count: 0 });
const seen = [];
store.watch({ fields: ['count'], read: (state) => state.count }, (value) => seen.push(value));
store.set({ count: 1 });
const items = collection.createCollection([['a', { count: 0 }]]);
const counts = [];
items.watchItem('a', { fields: ['count'], read: (item) => item.count }, (value) => counts.push(value));
items.update('a', { count: 1 });
let internal = 'imported';
try {
  await import('@canonic2/lattice/src/modules/store/store.js');
} catch (error) {
  internal = error.code;
}
let unsupported = 'imported';
try {
  await import('@canonic2/lattice/experimental/collection');
} catch (error) {
  unsupported = error.code;
}
const checked = testing.checkSelection(
  'count',
  { fields: ['count'], read: (state) => state.count },
  { zero: { count: 0 } },
);
console.log(JSON.stringify({
  root: Object.keys(root).sort(),
  collection: Object.keys(collection).sort(),
  unsupported,
  shippingHasBulk: 'updateMembership' in items,
  counts,
  elements: Object.keys(elements).sort(),
  testing: Object.keys(testing).sort(),
  unobserved: checked.unobserved,
  seen,
  shallow: root.shallow([1], [1]),
  internal,
}));
`,
    );
    const output = JSON.parse(run(process.execPath, ['check.js'], consumer));
    expect(output).toEqual({
      root: ['createStore', 'shallow'],
      collection: ['createCollection'],
      unsupported: 'ERR_PACKAGE_PATH_NOT_EXPORTED',
      shippingHasBulk: false,
      counts: [0, 1],
      elements: ['captureInputs', 'defineOnce', 'replayInputs', 'skipUpgradeAttribute'],
      testing: ['checkSelection'],
      unobserved: [],
      seen: [0, 1],
      shallow: true,
      internal: 'ERR_PACKAGE_PATH_NOT_EXPORTED',
    });
  });
});

describe('TypeScript consumer types', () => {
  for (const environment of /** @type {const} */ (['dom', 'node'])) {
    test(`valid root uses type-check with ${environment} types`, () => {
      const { status, output } = typecheck('valid.ts', environment);
      expect(output).toBe('');
      expect(status).toBe(0);
    }, 60000);

    // A run expecting another type than read returns is reported on read's
    // result, because TypeScript infers the value type from run's parameter.
    test(`valid collection uses type-check with ${environment} types`, () => {
      const { status, output } = typecheck('valid-collection.ts', environment);
      expect(output).toBe('');
      expect(status).toBe(0);
    }, 60000);

    for (const [file, code] of [
      ['invalid-collection-field.ts', 'TS2322'],
      ['invalid-key.ts', 'TS2353'],
      ['invalid-field.ts', 'TS2322'],
      ['invalid-run.ts', "TS2322: Type 'string' is not assignable to type 'number'"],
      ['invalid-read.ts', 'TS2322'],
    ]) {
      test(`${file} fails with ${code} under ${environment} types`, () => {
        const { status, output } = typecheck(/** @type {string} */ (file), environment);
        expect(status).toBe(2);
        expect(output).toContain(/** @type {string} */ (code));
      }, 60000);
    }
  }

  test('valid element helper uses type-check with the DOM library', () => {
    const { status, output } = typecheck('valid-dom.ts', 'dom');
    expect(output).toBe('');
    expect(status).toBe(0);
  }, 60000);
});
