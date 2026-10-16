// Starts one Workbench server from a snapshot directory for a fixture, with
// screenshot capture stubbed out, and prints JSON lines: first { url }, then
// every diagnostic the server logs. Stops on SIGTERM or a closed stdin.
// Usage: node serve.cjs <workbench-snapshot> <fixture>
const path = require('node:path');

const [snapshot, fixture] = process.argv.slice(2).map(value => path.resolve(value));
const server = require(path.join(snapshot, 'server/node/server.js'));
const emit = value => process.stdout.write(JSON.stringify(value) + '\n');

server.start({
  root: fixture,
  config: { dir: fixture, key: null },
  eagerPreviews: true,
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
