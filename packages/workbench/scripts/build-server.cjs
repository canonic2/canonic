/* Builds one VSIX target's Rust server binary, or copies an explicitly supplied
   prebuilt executable. Cargo failures stop packaging before a VSIX is made. */
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const triples = {
  'darwin-arm64': 'aarch64-apple-darwin', 'darwin-x64': 'x86_64-apple-darwin',
  'linux-arm64': 'aarch64-unknown-linux-gnu', 'linux-x64': 'x86_64-unknown-linux-gnu',
  'win32-arm64': 'aarch64-pc-windows-msvc', 'win32-x64': 'x86_64-pc-windows-msvc',
};
function build(target = process.platform + '-' + process.arch, debug = false) {
  if (!triples[target]) throw new Error('Unsupported Workbench server target: ' + target);
  const root = path.resolve(__dirname, "../server");
  const name = 'workbench-server' + (target.startsWith('win32-') ? '.exe' : '');
  let source = process.env.WORKBENCH_SERVER_BINARY;
  if (!source) {
    const local = path.join(os.homedir(), '.cargo/bin/cargo' + (process.platform === 'win32' ? '.exe' : ''));
    const cargo = fs.existsSync(local) ? local : 'cargo';
    // server/rust-toolchain.toml pins Rspack's nightly toolchain. The host's
    // own target builds in Cargo's default folder, shared with the checks.
    const host = target === process.platform + '-' + process.arch;
    const args = ['build', '--locked'].concat(host ? [] : ['--target', triples[target]]);
    if (!debug) args.push('--release');
    const result = cp.spawnSync(cargo, args, { cwd: root, stdio: 'inherit' });
    if (result.error || result.status !== 0) throw new Error('Cannot build workbench-server for ' + target + '. Install the toolchain server/rust-toolchain.toml names, target ' + triples[target] + ' and its linker, or supply WORKBENCH_SERVER_BINARY for this target.', { cause: result.error });
    source = path.join(root, 'target', ...(host ? [] : [triples[target]]), debug ? 'debug' : 'release', name);
  }
  const bins = path.join(root, 'bin');
  fs.mkdirSync(bins, { recursive: true });
  // A VSIX carries only its target, even after packaging several targets.
  for (const entry of fs.readdirSync(bins)) if (entry !== target) fs.rmSync(path.join(bins, entry), { recursive: true, force: true });
  const destination = path.join(bins, target, name);
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.copyFileSync(source, destination);
  if (!target.startsWith('win32-')) fs.chmodSync(destination, 0o755);
  return destination;
}
if (require.main === module) build(undefined, process.argv.includes('--debug'));
module.exports = { build, triples };
