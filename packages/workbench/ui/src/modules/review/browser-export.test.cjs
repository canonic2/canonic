const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

function delivery(responses) {
  const requests = [],
    links = [],
    timers = [];
  const context = {
    window: {},
    Date,
    Promise,
    Error,
    encodeURIComponent,
    fetch: async (...args) => {
      requests.push(args);
      return responses.shift();
    },
    URL: { createObjectURL: () => 'blob:acme', revokeObjectURL: (value) => links.push(value) },
    document: {
      createElement: () => ({
        click() {
          links.push(this.download);
        },
        remove() {},
      }),
      body: { appendChild() {} },
    },
    setTimeout: (callback, delay) => {
      timers.push({ callback, delay });
    },
  };
  vm.runInNewContext(fs.readFileSync(__dirname + '/browser-export.js', 'utf8'), context);
  return { api: context.window.wbExport, requests, links, timers };
}
const json = (value) => ({ ok: true, json: async () => value });

test('export delivery preserves request options, progress and delayed blob handoff', async () => {
  const done = { ok: true, id: 'acme/job', status: 'complete', completed: 2, total: 2 };
  const h = delivery([
    json({ ok: true, id: done.id }),
    json(done),
    { ok: true, headers: { get: () => 'attachment; filename="acme.zip"' }, blob: async () => ({}) },
  ]);
  const detail = { format: 'zip', current: { page: 'account', iteration: 'bold' } };
  const job = await h.api.start(detail);
  const progress = [];
  const status = await h.api.poll(job, 100, (value, started) => progress.push([value, started]));
  assert.equal(status, done);
  assert.deepEqual(progress, [[done, 100]]);
  assert.equal(await h.api.download(status), 'acme.zip');
  assert.deepEqual(JSON.parse(h.requests[0][1].body), detail);
  assert.equal(h.requests[1][0], '/_workbench/export?job=acme%2Fjob');
  assert.equal(h.requests[1][1].cache, 'no-store');
  assert.equal(h.requests[2][0], '/_workbench/export?job=acme%2Fjob&download=1');
  assert.deepEqual(h.links, ['acme.zip']);
  assert.equal(h.timers[0].delay, 60000);
  h.timers[0].callback();
  assert.deepEqual(h.links, ['acme.zip', 'blob:acme']);
});

test('export delivery surfaces malformed responses and failed jobs', async () => {
  const h = delivery([
    {
      status: 502,
      json: async () => {
        throw Error('not JSON');
      },
    },
    json({ ok: true, status: 'failed', error: 'Acme capture failed' }),
  ]);
  await assert.rejects(h.api.start({}), /The export server answered 502/);
  await assert.rejects(
    h.api.poll({ id: 'acme' }, 0, () => {}),
    /Acme capture failed/,
  );
  assert.deepEqual(h.links, []);
});
