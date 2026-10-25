/* Pause an executable publication after its copy destination has been opened,
   so the parent can request catalog work during that copy window. */
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { workerData, parentPort } = require('node:worker_threads');
const { builder, root, source, release } = workerData;
const moduleUnderTest = { exports: {} };
const copying = { ...fs, copyFileSync(from, to) {
  fs.closeSync(fs.openSync(to, 'w'));
  parentPort.postMessage('copy-opened');
  Atomics.wait(new Int32Array(release), 0, 0);
  fs.copyFileSync(from, to);
} };
vm.runInNewContext(fs.readFileSync(builder, 'utf8'), {
  require: name => name === 'node:fs' ? copying : require(name),
  module: moduleUnderTest, __dirname: path.join(root, 'scripts'),
  process: { ...process, env: { ...process.env, WORKBENCH_SERVER_BINARY: source } },
});
moduleUnderTest.exports.build();
