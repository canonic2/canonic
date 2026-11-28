// The real transport must stay responsive when the fake native executor is blocked.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { connect } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';

async function fixture(run) {
  const child = spawn(process.argv[2], [], { stdio: ['ignore', 'pipe', 'inherit'] });
  const exited = once(child, 'exit');
  const lines = createInterface({ input: child.stdout });
  const timeout = setTimeout(() => child.kill(), 20000);
  try {
    const [line] = await once(lines, 'line');
    await run(JSON.parse(line).port, exited);
  } finally {
    clearTimeout(timeout);
    lines.close();
    if (child.exitCode === null) child.kill();
    await exited;
  }
}
let serial = 0;
function call(port, action, args = {}, extra = {}) {
  const socket = connect({ host: '127.0.0.1', port });
  const request = Buffer.from(JSON.stringify({ version: 1, sessionId: 'Acme',
    token: 'a'.repeat(64), requestId: String(++serial), action, arguments: args, ...extra }));
  const header = Buffer.alloc(4);
  header.writeUInt32BE(request.length);
  const reply = new Promise((resolve, reject) => {
    let bytes = Buffer.alloc(0);
    socket.setTimeout(10000, () => socket.destroy(new Error('Fixture request timed out')));
    socket.on('error', reject);
    socket.on('data', data => {
      bytes = Buffer.concat([bytes, data]);
      if (bytes.length >= 4 && bytes.length >= 4 + bytes.readUInt32BE()) {
        resolve(JSON.parse(bytes.subarray(4, 4 + bytes.readUInt32BE()).toString()));
        socket.end();
      }
    });
    socket.on('end', () => reject(new Error('Runner ended before reply')));
    socket.on('close', () => reject(new Error('Caller or runner closed before reply')));
    socket.on('connect', () => socket.write(Buffer.concat([header, request])));
  });
  return { reply, cancel: () => socket.destroy() };
}
async function until(port, state) {
  for (let attempt = 0; attempt < 100; attempt++) {
    const reply = await call(port, 'hello').reply;
    if (reply.data.inspection.state === state) return;
    await delay(10);
  }
  assert.fail(`Runner never reported ${state}`);
}
const busy = port => until(port, 'busy');
await fixture(async (port, exited) => {
  assert.equal((await call(port, 'hello', {}, { token: 'b'.repeat(64) }).reply).code, 'UNAUTHORIZED');
  assert.equal((await call(port, 'unknown').reply).code, 'CAPABILITY_UNAVAILABLE');
  const read = call(port, 'ui_snapshot', { fixtureDelay: 6 });
  await busy(port);
  const readingHealth = (await call(port, 'hello').reply).data.inspection;
  assert.equal(readingHealth.activeAction, 'ui_snapshot');
  assert.ok(readingHealth.activeElapsedMs >= 0);
  assert.equal((await call(port, 'ui_snapshot').reply).code, 'RUNNER_BUSY');
  assert.equal((await call(port, 'app_launch', {}, { commandId: 'not-queued' }).reply).code, 'RUNNER_BUSY');
  await delay(4100);
  const health = await call(port, 'hello').reply;
  assert.equal(health.data.inspection.inspectionExpired, true);
  assert.equal((await read.reply).code, 'INSPECTION_TIMEOUT');
  assert.ok((await call(port, 'ui_snapshot').reply).data.snapshotId);
  const action = await call(port, 'app_launch', {}, { commandId: 'one' }).reply;
  assert.equal(action.data.result.mutations, 1);
  assert.deepEqual((await call(port, 'app_launch', {}, { commandId: 'one' }).reply).data, action.data);
  assert.equal((await call(port, 'app_launch', { different: true }, { commandId: 'one' }).reply).code, 'COMMAND_ID_CONFLICT');
  const cancelled = call(port, 'ui_snapshot', { fixtureDelay: 1 });
  const cancelledReply = cancelled.reply.catch(() => null);
  await busy(port);
  cancelled.cancel();
  assert.equal((await call(port, 'app_launch', {}, { commandId: 'also-not-queued' }).reply).code, 'RUNNER_BUSY');
  await delay(1100);
  assert.ok((await call(port, 'ui_snapshot').reply).data.snapshotId);
  await cancelledReply;
  assert.equal((await call(port, 'app_launch', {}, { commandId: 'two' }).reply).data.result.mutations, 2);
  const holding = call(port, 'app_launch', { fixtureDelay: 1 }, { commandId: 'holding' });
  await busy(port);
  assert.equal((await call(port, 'hello').reply).data.inspection.activeAction, 'app_launch');
  // Stop refuses new work at once and lets input inside its grace finish normally.
  const stopping = call(port, 'stop');
  await until(port, 'closed');
  assert.equal((await call(port, 'app_launch', {}, { commandId: 'after-stop' }).reply).code, 'SESSION_CLOSED');
  assert.equal((await holding.reply).data.result.mutations, 3);
  assert.deepEqual((await stopping.reply).data, { stopped: true, abortedAction: null });
  assert.deepEqual(await exited, [0, null]);
});
await fixture(async (port, exited) => {
  // Input wedged past its grace cannot hold cleanup: stop ends the owned runner.
  const wedged = call(port, 'app_launch', { fixtureDelay: 30 }, { commandId: 'wedged' });
  const abandoned = wedged.reply.catch(() => null);
  await busy(port);
  const started = Date.now();
  assert.deepEqual((await call(port, 'stop').reply).data, { stopped: true, abortedAction: 'app_launch' });
  assert.ok(Date.now() - started < 5000);
  assert.deepEqual(await exited, [0, null]);
  await abandoned;
});
await fixture(async (port, exited) => {
  const read = call(port, 'ui_snapshot', { fixtureDelay: 10 });
  const interrupted = read.reply.catch(() => null);
  await busy(port);
  assert.deepEqual((await call(port, 'stop').reply).data, { stopped: true, abortedAction: 'ui_snapshot' });
  assert.deepEqual(await exited, [0, null]);
  await interrupted;
});
console.log('Runner transport: timeout, heartbeat, no queued input, cancellation, recovery, deduplication, owned stop and wedged-input release passed.');
