import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { parseArgs } from 'node:util';

const require = createRequire(import.meta.url);
const root = fileURLToPath(new URL('../../../../', import.meta.url));

interface Actions {
  buildServer(): void;
  bundleRuntime(): Promise<void>;
  run(command: string, args: string[], cwd: string): void;
  log(message: string): void;
}

const actions: Actions = {
  buildServer() { require('../../../build-server.cjs').build(undefined, true); },
  async bundleRuntime() {
    await require('../../../bundle-runtime.cjs').bundle(process.platform + '-' + process.arch, { reuse: true });
  },
  run(command, args, cwd) { execFileSync(command, args, { cwd, stdio: 'inherit' }); },
  log: console.log,
};

/** Keep Cargo's freshness check, but do no release build or VSIX assembly. */
export async function prepare(io: Actions = actions): Promise<void> {
  io.buildServer();
  await io.bundleRuntime();
}

/** Explicit files keep the fast command from accidentally running the whole suite. */
export function focused(files: string[], pattern: string | undefined, io: Actions = actions, product = root): void {
  if (!files.length) throw new Error('Choose test files: pnpm run test:focus ui/src/path/example.test.js [--name pattern]');
  const selected = files.map(file => {
    const absolute = path.resolve(product, file);
    const relative = path.relative(product, absolute);
    if (relative === '..' || relative.startsWith('..' + path.sep) || path.isAbsolute(relative) || !/\.test\.[cm]?[jt]s$/.test(relative) || !fs.statSync(absolute).isFile()) {
      throw new Error('Expected a Workbench test file: ' + file);
    }
    return relative;
  });
  // Only UI files are known to be independent of the native server. Other
  // tests get Cargo's cheap no-op check rather than trusting a stale binary.
  if (selected.some(file => !file.startsWith('ui' + path.sep))) io.buildServer();
  const args = ['--test', '--test-concurrency=4'];
  if (pattern !== undefined) args.push('--test-name-pattern=' + pattern);
  io.run(process.execPath, [...args, ...selected], product);
}

export async function main(argv = process.argv.slice(2), io: Actions = actions, product = root): Promise<void> {
  const [command, ...rest] = argv.filter(arg => arg !== '--');
  if (command === 'prepare') {
    await prepare(io);
    io.log('Development files ready. Reload the Extension Development Host window after edits.');
  } else if (command === 'test') {
    const { values, positionals } = parseArgs({ args: rest, options: { name: { type: 'string' } }, allowPositionals: true });
    focused(positionals, values.name, io, product);
  } else if (command === 'launch') {
    if (rest.length > 1) throw new Error('Usage: pnpm run dev [project-folder]');
    const project = path.resolve(product, rest[0] || process.env.INIT_CWD || product);
    if (!fs.statSync(project).isDirectory()) throw new Error('Expected a project folder: ' + project);
    await prepare(io);
    io.run('code', ['--new-window', '--extensionDevelopmentPath=' + path.join(product, 'extension'), project], product);
    io.log('Opened the source extension. Reload this development window after JS/CSS edits; rerun dev:prepare after Rust or capture/app edits.');
  } else {
    throw new Error('Expected launch, prepare or test');
  }
}
