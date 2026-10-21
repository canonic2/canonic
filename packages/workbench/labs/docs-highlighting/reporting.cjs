// Publish a bounded, neutral projection while keeping complete traces in the
// owned temporary run. Assert that projection preserves every numeric aggregate.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { summarize } = require('./summarize.cjs');
const sha = value => crypto.createHash('sha256').update(value).digest('hex');
function publishPlan(source, destination) {
  const original = fs.readFileSync(source, 'utf8');
  const folder = path.dirname(destination), provenance = path.join(folder, 'plan-provenance.json');
  assert.equal(fs.existsSync(provenance), false, 'Plan links have already been rebased');
  const reviewed = original.replace(/(\[[^\]]*\]\()([^)]+)(\))/g, (match, before, target, after) => {
    if (/^(?:[a-z][a-z\d+.-]*:|#)/i.test(target)) return match;
    const [file, ...fragment] = target.split('#');
    const relative = path.relative(folder, path.resolve(__dirname, file)).split(path.sep).join('/');
    return before + relative + (fragment.length ? '#' + fragment.join('#') : '') + after;
  });
  fs.writeFileSync(destination, reviewed);
  fs.writeFileSync(provenance, JSON.stringify({ originalSha256: sha(original), reviewedSha256: sha(reviewed),
    transformation: 'Only relative Markdown link targets are rebased from the lab README location to this review copy. The original frozen plan remains in the owned raw run; method text and measurements are unchanged.' }, null, 2));
}
function project(report, replacements) {
  const copy = structuredClone(report);
  for (const sample of Array.isArray(copy.samples) ? copy.samples : []) {
    if (sample.correctness)
      sample.correctness.eligible = sample.status === 'passed' && sample.checks.tokenRoles === true && sample.checks.renderParity?.pass === true;
    delete sample.logs;
    delete sample.processMemory;
    const memory = sample.memoryAfterReturns;
    if (memory?.samples) {
      const seen = new Map();
      for (const moment of memory.samples) for (const process of moment.processes)
        seen.set(`${process.pid}|${process.cpu}`, { pid: process.pid, cpu: process.cpu });
      memory.cpuObservations = [...seen.values()];
      delete memory.samples;
    }
  }
  return JSON.parse(JSON.stringify(copy, (key, value) => {
    if (typeof value !== 'string') return value;
    for (const [from, to] of replacements) {
      value = value.replaceAll(from, to);
      for (const encoded of [encodeURIComponent(from), from.replaceAll('/', '_'), from.replace(/^\//, '').replaceAll('/', '_')])
        value = value.replaceAll(encoded, to.replace(/[<>]/g, ''));
    }
    if (key === 'text' || (value.length > 1024 && key !== 'error'))
      return { sha256: sha(value), utf16Length: value.length, utf8Bytes: Buffer.byteLength(value) };
    return value;
  }));
}
function publish(file, destination, replacements) {
  const input = fs.readFileSync(file), raw = JSON.parse(input);
  const report = project(raw, replacements);
  if (raw.samples) assert.deepEqual(summarize(report).metrics, summarize(raw).metrics);
  fs.mkdirSync(destination, { recursive: false });
  fs.writeFileSync(path.join(destination, 'report.json'), JSON.stringify(report, null, 2));
  if (raw.samples) {
    const summary = summarize(report);
    fs.writeFileSync(path.join(destination, 'summary.json'), JSON.stringify(summary, null, 2));
    const tables = new Map();
    for (const row of summary.metrics) {
      if (!tables.has(row.metric)) tables.set(row.metric, []);
      tables.get(row.metric).push(row);
    }
    const number = value => Number(value.toFixed(3));
    fs.writeFileSync(path.join(destination, 'metrics.md'), [
      '# Observed metrics', '', summary.qualification, '',
      'Nearest rank; display rounded to three decimals. JSON retains full precision. No pooling across fixtures, cache conditions or reports. With n=1 all quantiles equal the single observation.', '',
      ...[...tables].flatMap(([metric, rows]) => [
        `## ${metric}`, '',
        '| Variant | Fixture | Cache condition | n | p50 | p95 | Maximum |',
        '| --- | --- | --- | ---: | ---: | ---: | ---: |',
        ...rows.map(row => `| ${row.variant} | ${row.shape} | ${row.cache} | ${row.n} | ${number(row.p50)} | ${number(row.p95)} | ${number(row.max)} |`), '',
      ]),
    ].join('\n'));
  }
  const folder = path.dirname(file), traces = [];
  for (const sample of raw.samples || []) {
    if (!sample.rawSample) continue;
    const bytes = fs.readFileSync(path.join(folder, sample.rawSample));
    traces.push({ file: sample.rawSample, bytes: bytes.length, sha256: sha(bytes) });
  }
  fs.writeFileSync(path.join(destination, 'provenance.json'), JSON.stringify({
    originalReportSha256: sha(input), originalReportBytes: input.length,
    analysisTools: Object.fromEntries([__filename, require.resolve('./summarize.cjs')].map(file => [path.basename(file), sha(fs.readFileSync(file))])),
    projection: 'Exact latency/resource/worker values, correctness checks and file identities; source text replaced by digests. Eligibility recomputed from execution, token roles and final group parity, correcting pre-group flags. Duplicate CPU observations deduplicated without changing values. Full logs, process traces and screenshots remain in the owned temporary run.',
    traces,
  }, null, 2));
  if (fs.existsSync(path.join(folder, 'plan.md')))
    publishPlan(path.join(folder, 'plan.md'), path.join(destination, 'plan.md'));
  return { samples: raw.samples?.length, variants: raw.variants?.length };
}
module.exports = { project, publish, publishPlan };
