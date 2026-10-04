import { spawn } from 'node:child_process';
import path from 'node:path';
import { packageRoot } from '../../src/platform/paths.ts';
const child = spawn(
  process.execPath,
  [
    path.join(packageRoot, 'node_modules/electron/cli.js'),
    path.join(packageRoot, 'dist/test/integration/desktop.js'),
    ...process.argv.slice(2),
  ],
  { cwd: packageRoot, stdio: 'inherit' },
);
child.once('error', (error) => {
  console.error(error);
  process.exitCode = 1;
});
child.once('exit', (code) => {
  process.exitCode = code ?? 1;
});
