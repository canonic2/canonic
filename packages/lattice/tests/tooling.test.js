/** @file Verifies that the configured checks reject missing and incorrect JSDoc. */
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

const root = fileURLToPath(new URL('../', import.meta.url));
const documented = `/** @file A typed fixture for checking the development tools. */
/**
 * Doubles the supplied width.
 * @param {number} width The input width.
 * @returns {number} Twice the width.
 */
export function resize(width) { return width * 2; }
`;

/**
 * Runs a pinned package executable without a shell and captures its diagnostics.
 * @param {string} executable The executable path relative to the package root.
 * @param {string[]} args Arguments forwarded to the executable.
 * @returns {import('node:child_process').SpawnSyncReturns<string>} The process result.
 */
function run(executable, args) {
  return spawnSync(process.execPath, [join(root, executable), ...args], {
    cwd: root,
    encoding: 'utf8',
    timeout: 15000,
  });
}

/**
 * Provides a temporary source fixture, deleting it even when a check fails.
 * @param {(path: string) => void} check The check that owns the fixture during this call.
 */
function withFixture(check) {
  const directory = mkdtempSync(join(tmpdir(), 'lattice-tooling-'));
  try {
    check(join(directory, 'fixture.js'));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

test('Oxlint accepts documented code and rejects undocumented declarations and private methods', () => {
  withFixture((path) => {
    writeFileSync(path, documented);
    const valid = run('node_modules/oxlint/bin/oxlint', ['-c', '.oxlintrc.json', path]);
    expect(valid.error).toBeUndefined();
    expect(valid.status, valid.stdout + valid.stderr).toBe(0);

    writeFileSync(
      path,
      '/** @file Undocumented function fixture. */\nexport const marker = 1;\nexport function resize(width) { return width * 2; }',
    );
    const missing = run('node_modules/oxlint/bin/oxlint', ['-c', '.oxlintrc.json', path]);
    expect(missing.status).toBe(1);
    expect(missing.stdout + missing.stderr).toContain('require-jsdoc');

    writeFileSync(
      path,
      '/** @file Private helper fixture. */\n/** A probe. */\nexport class Probe { #run() {} }',
    );
    const privateMethod = run('node_modules/oxlint/bin/oxlint', ['-c', '.oxlintrc.json', path]);
    expect(privateMethod.status).toBe(1);
    expect(privateMethod.stdout + privateMethod.stderr).toContain('require-jsdoc');

    writeFileSync(
      path,
      '/** @file Named callback fixture. */\nexport const marker = 1;\nexport const resize = (width) => width * 2;',
    );
    const namedArrow = run('node_modules/oxlint/bin/oxlint', ['-c', '.oxlintrc.json', path]);
    expect(namedArrow.status).toBe(1);
    expect(namedArrow.stdout + namedArrow.stderr).toContain('require-jsdoc');

    writeFileSync(path, 'export const width = 20;');
    const missingOverview = run('node_modules/oxlint/bin/oxlint', ['-c', '.oxlintrc.json', path]);
    expect(missingOverview.status).toBe(1);
    expect(missingOverview.stdout + missingOverview.stderr).toContain('require-file-overview');

    writeFileSync(path, documented.replace('@param {number} width', '@param {number} wrong'));
    const wrongName = run('node_modules/oxlint/bin/oxlint', ['-c', '.oxlintrc.json', path]);
    expect(wrongName.status).toBe(1);
    expect(wrongName.stdout + wrongName.stderr).toContain('check-param-names');
  });
}, 20000);

test('TypeScript checks JavaScript calls against their JSDoc types without emitting code', () => {
  withFixture((path) => {
    const args = [
      '--ignoreConfig',
      '--allowJs',
      '--checkJs',
      '--strict',
      '--noEmit',
      '--skipLibCheck',
      '--target',
      'ESNext',
      path,
    ];
    writeFileSync(path, documented + '\nresize(20);');
    const valid = run('node_modules/typescript/bin/tsc', args);
    expect(valid.error).toBeUndefined();
    expect(valid.status, valid.stdout + valid.stderr).toBe(0);

    writeFileSync(path, documented + "\nresize('wide');");
    const invalid = run('node_modules/typescript/bin/tsc', args);
    expect(invalid.status).toBe(2);
    expect(invalid.stdout + invalid.stderr).toContain('TS2345');
  });
}, 20000);
