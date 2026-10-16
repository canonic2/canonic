// Runs Cargo for the lab's Rust host with lab-owned caches.
//
//   node labs/instant-first-page/cargo.mjs <cargo arguments…>
//
// Crate downloads (CARGO_HOME), compiled output (CARGO_TARGET_DIR) and
// Rspack's pinned nightly toolchain (RUSTUP_HOME) live in one cache under the
// system temporary folder, kept between this lab's runs and never in the
// user's ~/.cargo or ~/.rustup. Prints the cache path on stderr.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
export const cacheDir = path.join(os.tmpdir(), 'canonic-workbench-labs', 'instant-first-page', 'cargo');
export const targetDir = path.join(cacheDir, 'target');
export const hostDir = path.join(here, 'host');

export function cargo(args, options = {}) {
  fs.mkdirSync(cacheDir, { recursive: true });
  // rustup's proxies (cargo, rustc) stay in the user's ~/.cargo/bin; the
  // registry, toolchain and build output move. The proxies install the
  // toolchain host/rust-toolchain.toml names on first use.
  const bin = path.join(os.homedir(), '.cargo', 'bin');
  const env = { ...process.env, CARGO_HOME: path.join(cacheDir, 'home'), RUSTUP_HOME: path.join(cacheDir, 'rustup'), RUSTUP_AUTO_INSTALL: '1',
    CARGO_TARGET_DIR: targetDir, PATH: bin + path.delimiter + process.env.PATH };
  return spawnSync(path.join(bin, 'cargo'), args, { cwd: hostDir, env, stdio: 'inherit', ...options });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  console.error('Cargo cache: ' + cacheDir);
  const result = cargo(process.argv.slice(2));
  if (result.error) { console.error(result.error.message); process.exit(1); }
  process.exit(result.status ?? 1);
}
