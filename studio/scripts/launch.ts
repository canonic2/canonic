import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const child = spawn(
  process.execPath,
  [path.join(root, 'node_modules/electron/cli.js'), root, ...process.argv.slice(2)],
  {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, ELECTRON_CACHE: path.join(root, '.runtime/electron-cache') },
  },
);
child.once('error', (error) => {
  console.error(error);
  process.exitCode = 1;
});
child.once('exit', (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
});
