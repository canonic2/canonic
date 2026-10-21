const { test } = require('node:test');
const assert = require('node:assert/strict');
const { summarize } = require('./summarize.cjs');

test('report aggregates keep cells separate and exclude unsuccessful executions', () => {
  const base = {
    variant: '1',
    shape: 'Overview',
    cache: 'cold-startup',
    checks: { tokenRoles: true },
    sourceFirst: {},
    sourceReopen: {},
  };
  const samples = Array.from({ length: 20 }, (_, i) => ({
    ...base,
    index: i,
    status: 'passed',
    cells: [
      {
        action: 'first',
        visibleMs: i + 1,
        colouredMs: 100 + i,
        blocks: [{ spans: 1 }],
        longTasks: [],
        resources: [],
      },
    ],
  }));
  samples.push({
    ...base,
    index: 20,
    status: 'failed',
    cells: [
      {
        action: 'first',
        visibleMs: 99999,
        colouredMs: 99999,
        blocks: [{ spans: 1 }],
        longTasks: [],
        resources: [],
      },
    ],
  });
  const result = summarize({ mode: 'test', samples, failures: [{}], cleanupFailures: [] });
  const visible = result.metrics.find((row) => row.metric === 'first.visibleMs');
  assert.deepEqual(
    { n: visible.n, p50: visible.p50, p95: visible.p95, max: visible.max },
    { n: 20, p50: 10, p95: 19, max: 20 },
  );
  const coloured = result.metrics.find((row) => row.metric === 'first.colouredMs');
  assert.equal(coloured.p50, 109);
  assert.equal(result.correctness.length, 21);
  assert.equal(result.failures, 1);
});

test('plain diagnostic and code-free pages have no coloured success metric', () => {
  const sample = {
    variant: 'D',
    shape: 'Small',
    cache: 'cold',
    status: 'passed',
    index: 0,
    checks: { tokenRoles: 'diagnostic' },
    sourceFirst: { readableMs: 2, colouredMs: 3 },
    sourceReopen: {},
    cells: [
      { action: 'first', visibleMs: 1, colouredMs: 1, blocks: [], longTasks: [], resources: [] },
    ],
  };
  const result = summarize({ mode: 'test', samples: [sample], failures: [], cleanupFailures: [] });
  assert.ok(!result.metrics.some((row) => row.metric.endsWith('.colouredMs')));
  assert.equal(result.metrics.find((row) => row.metric === 'sourceFirst.readableMs').p50, 2);
});

test('cached actions do not inherit initialization or tokenization from prior jobs', () => {
  const docs = { id: 1, initializationMs: 20, tokenizationMs: 5 };
  const source = { id: 2, initializationMs: 0, tokenizationMs: 10 };
  const cell = { visibleMs: 4, colouredMs: 8, blocks: [{ spans: 1 }], longTasks: [], resources: [] };
  const sample = {
    variant: '1', shape: 'Small', cache: 'cold', status: 'passed', checks: {},
    cells: [{ ...cell, action: 'first', workers: [docs] }, { ...cell, action: 'retained', workers: [docs, source] }],
    sourceFirst: { workers: [docs, source] }, sourceReopen: { workers: [docs, source] },
  };
  const metrics = summarize({ mode: 'test', samples: [sample], failures: [], cleanupFailures: [] }).metrics;
  const value = metric => metrics.find(row => row.metric === metric)?.p50;
  assert.equal(value('first.initializationMs'), 20);
  assert.equal(value('sourceFirst.initializationMs'), 0);
  assert.equal(value('sourceFirst.tokenizationMs'), 10);
  assert.equal(value('sourceReopen.tokenizationMs'), 0);
  assert.equal(value('retained.initializationMs'), 0);
  assert.equal(value('retained.tokenizationMs'), 0);
});

test('plain fallback retains independently observed visibility without a coloured success', () => {
  const sample = {
    variant: '1', shape: 'Code-heavy', cache: 'cold', status: 'failed', checks: {},
    cells: [{ action: 'first', startedAbs: 1, visibleMs: 20, colouredMs: 5000,
      visibleAndColouredMs: 5000, blocks: [{ spans: 0 }], longTasks: [], resources: [],
      assets: { fonts: 'loaded', images: true, examples: [{ status: 'ready' }] } }],
    sourceFirst: { startedAbs: 2, readableMs: 10, colouredMs: 5000, spans: 0 },
  };
  const result = summarize({ mode: 'test', samples: [sample], failures: [{}], cleanupFailures: [] });
  assert.equal(result.metrics.find(row => row.metric === 'first.visibleMs').p50, 20);
  assert.equal(result.metrics.find(row => row.metric === 'sourceFirst.readableMs').p50, 10);
  assert.ok(!result.metrics.some(row => row.metric.endsWith('.colouredMs') || row.metric.endsWith('.visibleAndColouredMs')));
  assert.equal(result.correctness[0].status, 'failed');
});

test('native phase evidence keeps request/server boundaries and missing headers distinct', () => {
  const { observe } = require('./pipeline-phases.cjs');
  const request = (duration, total, highlight) => ({ name: '/_workbench/docs/highlight', duration,
    serverTiming: [{ name: 'total', duration: total }, { name: 'highlight', duration: highlight }] });
  const sample = { variant: '0', cells: [
    { action: 'first', resources: [request(12, 10, 8), request(4.9, 5, 4)] },
    { action: 'unobserved', resources: [request(99, 90, 80)] },
    { action: 'retained', resources: [] },
  ], sourceFirst: { resources: [{ name: '/_workbench/docs/highlight', duration: 5 }] } };
  const measured = { 'first.visibleMs': 20, 'retained.visibleMs': 1, 'sourceFirst.readableMs': 2 };
  const values = observe(sample, measured);
  assert.equal(values['first.nativeHighlightRequestMs'], 16.9);
  assert.equal(values['first.nativeHighlightServerMs'], 12);
  assert.equal(values['first.nativeOtherServerMs'], 3);
  assert.ok(Math.abs(values['first.nativeOutsideServerMs'] - 1.9) < 1e-8);
  assert.equal(values['sourceFirst.nativeHighlightRequestMs'], 5);
  assert.ok(!Object.hasOwn(values, 'sourceFirst.nativeHighlightServerMs'));
  assert.ok(!Object.keys(values).some(key => /^(unobserved|retained)\./.test(key)));
  assert.deepEqual(observe({ ...sample, variant: '1' }, measured), {});
});

test('worker outside-engine work uses per-action round trips including startup and messaging', () => {
  const { observe } = require('./pipeline-phases.cjs');
  const job = { id: 1, startedAbs: 100, totalWorkerMs: 30, initializationMs: 4,
    grammarInitializationMs: 3, tokenizationMs: 20 };
  const sample = { variant: '1', cells: [{ action: 'first', startedAbs: 90, workers: [job] },
    { action: 'retained', startedAbs: 150, workers: [job] }] };
  const measured = { 'first.visibleMs': 40, 'first.totalWorkerMs': 30,
    'first.initializationMs': 4, 'first.grammarInitializationMs': 3, 'first.tokenizationMs': 20,
    'retained.visibleMs': 2, 'retained.totalWorkerMs': 0, 'retained.initializationMs': 0,
    'retained.grammarInitializationMs': 0, 'retained.tokenizationMs': 0 };
  const values = observe(sample, measured);
  assert.deepEqual(values, { 'first.workerOutsideEngineMs': 3, 'retained.workerOutsideEngineMs': 0 });
  sample.cells[0].workers = [{ id: 2, startedAbs: 100, error: 'Failed', totalWorkerMs: 30 }];
  assert.ok(!Object.hasOwn(observe(sample, measured), 'first.workerOutsideEngineMs'));
});

test('reviewed plan links resolve after copying while preserving original provenance', () => {
  const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
  const crypto = require('node:crypto');
  const { publishPlan } = require('./reporting.cjs');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'workbench-plan-links-'));
  try {
    const source = path.join(dir, 'source.md'), destination = path.join(dir, 'reviewed', 'plan.md');
    fs.mkdirSync(path.dirname(destination));
    const original = '[Specs](../../specs/labs.md#inputs) [Web](https://example.com) [Here](#local)\n';
    fs.writeFileSync(source, original);
    publishPlan(source, destination);
    const reviewed = fs.readFileSync(destination, 'utf8');
    const link = reviewed.match(/\[Specs\]\(([^#]+)#inputs\)/)[1];
    assert.equal(path.resolve(path.dirname(destination), link), path.resolve(__dirname, '../../specs/labs.md'));
    assert.ok(reviewed.includes('[Web](https://example.com) [Here](#local)'));
    const provenance = JSON.parse(fs.readFileSync(path.join(path.dirname(destination), 'plan-provenance.json')));
    assert.equal(provenance.originalSha256, crypto.createHash('sha256').update(original).digest('hex'));
    assert.equal(provenance.reviewedSha256, crypto.createHash('sha256').update(reviewed).digest('hex'));
    assert.throws(() => publishPlan(source, destination), /already been rebased/);
    assert.equal(fs.readFileSync(source, 'utf8'), original);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
