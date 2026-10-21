// Separate host process keeps all Rust/Node/native caches under this sample's TMPDIR.
const path = require('node:path');
const [product, fixture, storage] = process.argv.slice(2);
const host = require(path.join(product, 'server/node/server.js'));
const emit = (value) => process.stdout.write(JSON.stringify(value) + '\n');
let running, closing;
const close = () =>
  (closing ||= (running ? running.close() : Promise.resolve()).then(() => process.exit(0)));
process.on('SIGTERM', close);
process.on('SIGINT', close);
process.stdin.on('end', close);
process.stdin.resume();
host
  .start({
    root: fixture,
    config: { dir: fixture, key: null },
    isTrusted: true,
    eagerPreviews: true,
    eagerCapture: false,
    trace: true,
    captureStorage: storage,
    onLog: (log) => emit({ log }),
  })
  .then(
    (value) => {
      running = value;
      emit({ url: value.url });
    },
    (error) => {
      emit({ error: String(error.stack || error) });
      process.exit(1);
    },
  );
