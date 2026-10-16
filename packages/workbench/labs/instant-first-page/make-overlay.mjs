// Writes stage 2's overlay into a snapshot of the package: the shipping
// preview-service.js with one branch that starts the Rust preview host
// instead of the Node worker. Variant code stays in the lab; the shipping
// file is read, not changed.
//
//   import { writeOverlay } from './make-overlay.mjs'; writeOverlay(snapshotDir)
//
// The branch runs only when WORKBENCH_LAB_PREVIEW is set (rust, neighbours or
// cap). WORKBENCH_LAB_HOST names the host binary and WORKBENCH_LAB_EVALUATOR
// the Node evaluator.
import fs from 'node:fs';
import path from 'node:path';

const BRANCH = `    }).then(function (executable) {
      if (process.env.WORKBENCH_LAB_PREVIEW) return launchLabHost(executable, began);
      var spawned = Date.now();`;

const LAUNCH = `
  /* Instant first page lab, stage 2: the Rust preview host answers the
     worker's routes. It prints {"port":N} when it listens. */
  function launchLabHost(executable, began) {
    var spawned = Date.now();
    var refreshDir = path.dirname(require.resolve('@rspack/plugin-react-refresh/package.json'));
    var hostOptions = {
      root: root, preview_dir: path.resolve(__dirname, '../../ui/preview'),
      cache_dir: path.join(require('node:os').tmpdir(), 'canonic-workbench-lab-host', require('node:crypto').createHash('sha256').update(root).digest('hex').slice(0, 12)),
      refresh: {
        entry: path.join(refreshDir, 'client', 'reactRefreshEntry.js'), runtime: path.join(refreshDir, 'client', 'reactRefresh.js'),
        utils: path.join(refreshDir, 'client', 'refreshUtils.js'), runtime_dir: path.dirname(require.resolve('react-refresh/runtime')),
      },
      evaluator: process.env.WORKBENCH_LAB_EVALUATOR, node: executable,
      mode: process.env.WORKBENCH_LAB_PREVIEW === 'rust' ? 'plain' : process.env.WORKBENCH_LAB_PREVIEW,
      cap: Number(process.env.WORKBENCH_LAB_CAP || 20), neighbours: Number(process.env.WORKBENCH_LAB_NEIGHBOURS || 8),
      index_saved: path.join(require('node:os').tmpdir(), 'canonic-workbench-lab-host-index.bin'),
      include: (options.previews && options.previews.include) || [],
    };
    return new Promise(function (resolve, reject) {
      var host = cp.spawn(process.env.WORKBENCH_LAB_HOST, ['preview', '--options', JSON.stringify(hostOptions)], { stdio: ['pipe', 'pipe', 'pipe'] });
      child = host;
      var timer = setTimeout(function () { host.kill(); reject(new Error('Preview host startup timed out')); }, 30000);
      var buffer = '';
      host.stdout.on('data', function (chunk) {
        buffer += chunk;
        var at = buffer.indexOf('\\n');
        if (at < 0 || address) return;
        clearTimeout(timer);
        var port = JSON.parse(buffer.slice(0, at)).port;
        diagnostic('info', 'preview.worker.ready', { prepareMs: spawned - began, launchMs: Date.now() - spawned, elapsedMs: Date.now() - began, host: 'rust' });
        address = 'http://127.0.0.1:' + port;
        resolve(address);
      });
      host.stderr.on('data', function (chunk) { if (options.log) options.log(String(chunk).slice(0, 2000)); });
      host.once('error', function (error) { clearTimeout(timer); reject(error); });
      host.once('exit', function () {
        clearTimeout(timer);
        if (child === host) { child = null; starting = null; address = null; }
        reject(new Error('Preview host stopped'));
      });
    });
  }
  return {`;

export function writeOverlay(snapshotDir) {
  const file = path.join(snapshotDir, 'server/node/preview-service.js');
  let text = fs.readFileSync(file, 'utf8');
  const original = `    }).then(function (executable) {
      var spawned = Date.now();`;
  if (text.split(original).length !== 2) throw new Error('preview-service.js changed shape: the lab overlay needs updating');
  text = text.split(original).join(BRANCH);
  const ret = '\n  return {\n    index:';
  if (text.split(ret).length !== 2) throw new Error('preview-service.js changed shape (return)');
  text = text.split(ret).join(LAUNCH + '\n    index:');
  fs.writeFileSync(file, text);
}
