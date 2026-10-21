// Secondary phase evidence; never substitutes for selection/readability timers.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { aggregate } = require('./performance-report.cjs');
const sha = body => crypto.createHash('sha256').update(body).digest('hex');
const key = sample => [sample.variant, sample.shape, sample.cache, sample.index].join('|');

function observe(sample, measured) {
  const observations = {};
  const actions = [...sample.cells.map(cell => [cell.action, cell]),
    ...['sourceFirst', 'sourceReopen'].filter(action => sample[action]).map(action => [action, sample[action]])];
  for (const [action, cell] of actions) {
    if (!Object.hasOwn(measured, `${action}.visibleMs`) && !Object.hasOwn(measured, `${action}.readableMs`)) continue;
    if (['1', '2'].includes(sample.variant)) {
      const fields = ['totalWorkerMs', 'initializationMs', 'grammarInitializationMs', 'tokenizationMs'];
      const previous = action === 'sourceFirst' ? sample.cells.find(cell => cell.action === 'first')?.workers
        : ['sourceReopen', 'retained'].includes(action) ? sample.sourceFirst?.workers : [];
      const earlier = new Set((previous || []).map(job => job.id));
      const jobs = (cell.workers || []).filter(job => typeof job.startedAbs === 'number'
        ? job.startedAbs >= cell.startedAbs : !earlier.has(job.id));
      if (jobs.some(job => job.error || !fields.every(field => Number.isFinite(job[field])))) continue;
      const values = fields
        .map(metric => measured[`${action}.${metric}`]);
      if (values.every(Number.isFinite))
        observations[`${action}.workerOutsideEngineMs`] = values[0] - values[1] - values[2] - values[3];
      continue;
    }
    if (sample.variant !== '0') continue;
    const requests = (cell.resources || []).filter(resource => resource.name === '/_workbench/docs/highlight');
    if (!requests.length) continue;
    if (requests.every(resource => Number.isFinite(resource.duration)))
      observations[`${action}.nativeHighlightRequestMs`] = requests.reduce((sum, resource) => sum + resource.duration, 0);
    const timed = requests.map(resource => ({ resource,
      total: resource.serverTiming?.find(timing => timing.name === 'total')?.duration,
      highlight: resource.serverTiming?.find(timing => timing.name === 'highlight')?.duration }));
    if (!timed.every(({ resource, total, highlight }) => [resource.duration, total, highlight].every(Number.isFinite))) continue;
    observations[`${action}.nativeHighlightServerMs`] = timed.reduce((sum, row) => sum + row.highlight, 0);
    observations[`${action}.nativeOtherServerMs`] = timed.reduce((sum, row) => sum + row.total - row.highlight, 0);
    observations[`${action}.nativeOutsideServerMs`] = timed.reduce((sum, row) => sum + row.resource.duration - row.total, 0);
  }
  return observations;
}

function publish(destination, runs) {
  const samples = [], metrics = [], inputs = [];
  for (const [name, file] of runs) {
    const bytes = fs.readFileSync(file), raw = JSON.parse(bytes);
    const reviewed = JSON.parse(fs.readFileSync(path.join(destination, name, 'report.json')));
    assert.equal(sha(bytes), reviewed.originalReportSha256, 'Phase evidence must use the original reviewed report');
    const measured = new Map(reviewed.samples.map(sample => [key(sample), sample.observations]));
    const rows = raw.samples.map(sample => ({ set: name, variant: sample.variant, shape: sample.shape,
      cache: sample.cache, index: sample.index, observations: observe(sample, measured.get(key(sample))) }))
      .filter(sample => Object.keys(sample.observations).length);
    samples.push(...rows);
    metrics.push(...aggregate(rows).map(row => ({ set: name, ...row })));
    inputs.push({ set: name, originalReportSha256: sha(bytes),
      reviewedReportSha256: sha(fs.readFileSync(path.join(destination, name, 'report.json'))) });
  }
  const boundary = 'Native highlight Server-Timing includes first-use regex/syntax initialization and tokenization together. Request duration ends at resource response completion; outside-server time is request duration minus the total server phase, including transport/queueing and clock precision. Worker outside-engine time is request-to-reply duration minus initialization, grammar initialization and tokenization: it includes worker startup, queueing, structured cloning and message handling, not isolated transport alone. Sums describe work across requests, not additive wall-clock stages; negative residuals are retained. Worker engine phases and DOM measurements remain in each set. Show code resources did not record Server-Timing, so only request duration is available there. Resource transferSize counts response bytes; request payload and worker message bytes were not separately counted. Only independently measured actions are included; request completion alone does not prove successful colouring.';
  const output = path.join(destination, 'pipeline-phases.json');
  assert.equal(fs.existsSync(output), false, 'Refuse to overwrite phase evidence');
  fs.writeFileSync(output, JSON.stringify({ boundary, inputs,
    analysisToolSha256: sha(fs.readFileSync(__filename)), samples, metrics }, null, 2));
  fs.writeFileSync(path.join(destination, 'pipeline-phases.csv'), [
    'set,variant,fixture,cache,metric,n,p50,p95,min,max',
    ...metrics.map(row => [row.set, row.variant, row.shape, row.cache, row.metric,
      row.n, row.p50, row.p95, row.min, row.max].join(',')),
  ].join('\n') + '\n');
  return { samples: samples.length, metrics: metrics.length, boundary };
}
if (require.main === module) {
  const [destination, ...files] = process.argv.slice(2);
  assert.equal(files.length, 3);
  console.log(JSON.stringify(publish(destination, ['set1', 'set2', 'confirmation'].map((name, index) => [name, files[index]]))));
}
module.exports = { observe, publish };
