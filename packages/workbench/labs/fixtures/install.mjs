// Installs a directory's pinned dependencies for a lab run. npm's cache lives
// in a lab-owned temporary folder, never the user's, and install scripts do
// not run. A committed lockfile makes installs reproducible; without one, the
// resolved lockfile is copied back for review.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export const LAB_CACHE = path.join(os.tmpdir(), 'canonic-workbench-labs', 'npm-cache');

export function install(directory, lockfile) {
  fs.mkdirSync(LAB_CACHE, { recursive: true });
  const locked = lockfile && fs.existsSync(lockfile);
  if (locked) fs.copyFileSync(lockfile, path.join(directory, 'package-lock.json'));
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const args = [locked ? 'ci' : 'install', '--ignore-scripts', '--no-audit', '--no-fund', '--loglevel=error'];
  const result = spawnSync(npm, args, {
    cwd: directory, stdio: 'inherit',
    env: { ...process.env, npm_config_cache: LAB_CACHE, npm_config_update_notifier: 'false' },
  });
  if (result.status !== 0) throw new Error(`npm ${args[0]} failed in ${directory} (exit ${result.status})`);
  if (!locked && lockfile) {
    fs.mkdirSync(path.dirname(lockfile), { recursive: true });
    fs.copyFileSync(path.join(directory, 'package-lock.json'), lockfile);
  }
}
