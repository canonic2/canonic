// Stage 2's server launcher: one Workbench server from a snapshot for a
// fixture, with screenshot capture stubbed out, printing JSON lines: first
// { url }, then every diagnostic. Stops on SIGTERM or a closed stdin.
//
//   node serve.cjs <workbench-snapshot> <fixture>
//
// WORKBENCH_LAB_STORAGE is the extension storage the product keeps across
// starts, where the bundled runtime is unpacked once. A cold start's caches
// are the system temporary folder's; the runtime is not one of them.
const path = require('node:path');

const [snapshot, fixture] = process.argv.slice(2).map(value => path.resolve(value));
const server = require(path.join(snapshot, 'server/node/server.js'));
const emit = value => process.stdout.write(JSON.stringify(value) + '\n');

server.start({
  root: fixture,
  config: { dir: fixture, key: null },
  eagerPreviews: true,
  captureStorage: process.env.WORKBENCH_LAB_STORAGE || undefined,
  capture: { warm() {}, prepare() {}, preparePage() {}, close() {} },
  onLog: record => emit({ log: record, at: Date.now() }),
}).then(running => {
  emit({ url: running.url });
  let closing;
  const close = () => { closing ||= running.close().finally(() => process.exit(0)); };
  process.on('SIGTERM', close);
  process.stdin.on('end', close);
  process.stdin.resume();
}, error => { emit({ error: String(error && error.stack || error) }); process.exit(1); });
