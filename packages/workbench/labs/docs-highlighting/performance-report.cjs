const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { summarize } = require('./summarize.cjs');
const { project, publishPlan } = require('./reporting.cjs');
const sha = body => crypto.createHash('sha256').update(body).digest('hex');
const metricKey = row => [row.variant, row.shape, row.cache, row.metric].join('|');

function aggregate(samples) {
  const groups = new Map();
  for (const sample of samples) for (const [metric, value] of Object.entries(sample.observations)) {
    const key = metricKey({ ...sample, metric });
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(value);
  }
  return [...groups].map(([key, values]) => {
    const [variant, shape, cache, metric] = key.split('|');
    values.sort((a, b) => a - b);
    return { variant, shape, cache, metric, n: values.length,
      p50: values[Math.ceil(values.length * .5) - 1],
      p95: values[Math.ceil(values.length * .95) - 1],
      min: values[0], max: values.at(-1) };
  });
}

function publish(file, destination, replacements = []) {
  const bytes = fs.readFileSync(file), raw = JSON.parse(bytes);
  const summary = summarize(raw), fixtures = {};
  const samples = raw.samples.map(sample => {
    if (fixtures[sample.shape]) assert.deepEqual(fixtures[sample.shape], sample.fixture);
    else fixtures[sample.shape] = sample.fixture;
    const one = summarize({ ...raw, samples: [sample] });
    const { corpus, ...checks } = sample.checks;
    const failedRoles = sample.variant === 'D' ? [] : one.correctness[0].failedRoles;
    const glyph = sample.visual?.glyphGeometry;
    return { variant: sample.variant, shape: sample.shape, cache: sample.cache,
      index: sample.index, status: sample.status, error: sample.error,
      browserVersion: sample.browserVersion,
      navigationMeasurementsComplete: sample.navigationMeasurementsComplete,
      loadBefore: sample.loadBefore, loadAfter: sample.loadAfter,
      checks, failedRoles, glyphGeometry: glyph && { characters: glyph.characters,
        differing: glyph.differing, maxDelta: glyph.maxDelta },
      observations: Object.fromEntries(one.metrics.map(metric => {
        assert.equal(metric.n, 1);
        return [metric.metric, metric.p50];
      })), actionOutcomes: Object.fromEntries([
        ...sample.cells.filter(cell => !['corpus', 'eviction-transit'].includes(cell.action)).map(cell => [cell.action, {
          visibilityObserved: !!cell.assets && cell.assets.fonts === 'loaded' && cell.assets.images,
          colouring: sample.variant === 'D' ? 'diagnostic' : !cell.blocks.length ? 'no-code' : cell.blocks.every(block => block.spans > 0) ? 'coloured' : 'plain-fallback',
          settlingMs: cell.colouredMs,
        }]),
        ...['sourceFirst', 'sourceReopen'].filter(key => sample[key]).map(key => [key, {
          readableObserved: Number.isFinite(sample[key].readableMs),
          colouring: sample.variant === 'D' ? 'diagnostic' : sample[key].spans > 0 ? 'coloured' : 'plain-fallback',
          settlingMs: sample[key].colouredMs,
        }]),
      ]), rawSample: sample.rawSample };
  });
  const sort = rows => rows.slice().sort((a, b) => metricKey(a).localeCompare(metricKey(b)));
  assert.deepEqual(sort(aggregate(samples)), sort(summary.metrics));
  const { samples: ignored, parity, ...metadata } = raw;
  const reviewed = project({ ...metadata, schema: 'workbench-highlighting-performance-v1',
    fixtures, samples, originalReportSha256: sha(bytes) }, replacements);
  const reviewedSummary = project(summary, replacements);
  for (const row of reviewedSummary.correctness) if (row.variant === 'D') row.failedRoles = [];
  fs.mkdirSync(destination, { recursive: false });
  fs.writeFileSync(path.join(destination, 'report.json'), JSON.stringify(reviewed, null, 2));
  fs.writeFileSync(path.join(destination, 'summary.json'), JSON.stringify(reviewedSummary, null, 2));
  fs.writeFileSync(path.join(destination, 'metrics.csv'), [
    'variant,fixture,cache,metric,n,p50,p95,min,max',
    ...summary.metrics.map(row => [row.variant, row.shape, row.cache, row.metric,
      row.n, row.p50, row.p95, row.min, row.max].join(',')),
  ].join('\n') + '\n');
  const tables = new Map();
  for (const row of summary.metrics) {
    if (!tables.has(row.metric)) tables.set(row.metric, []);
    tables.get(row.metric).push(row);
  }
  const number = value => Number(value.toFixed(3));
  fs.writeFileSync(path.join(destination, 'metrics.md'), [
    '# Performance observations', '', summary.qualification, '',
    'Nearest rank, rounded for display; JSON/CSV retains full precision. Values are kept separate by fixture, cache condition and run set.', '',
    ...[...tables].flatMap(([metric, rows]) => [
      `## ${metric}`, '', '| Variant | Fixture | Cache | n | p50 | p95 | Maximum |',
      '| --- | --- | --- | ---: | ---: | ---: | ---: |',
      ...rows.map(row => `| ${row.variant} | ${row.shape} | ${row.cache} | ${row.n} | ${number(row.p50)} | ${number(row.p95)} | ${number(row.max)} |`), '',
    ]),
  ].join('\n'));
  if (fs.existsSync(path.join(path.dirname(file), 'plan.md')))
    publishPlan(path.join(path.dirname(file), 'plan.md'), path.join(destination, 'plan.md'));
  fs.writeFileSync(path.join(destination, 'provenance.json'), JSON.stringify({
    originalReportSha256: sha(bytes), originalReportBytes: bytes.length,
    projection: 'Unrounded per-trial metric observations and correctness outcomes. Aggregate equality against the complete raw report is asserted before publication. Full resource/DOM/process traces and source text remain in the owned temporary run.',
    analysisTools: Object.fromEntries([__filename, require.resolve('./summarize.cjs'), require.resolve('./reporting.cjs')]
      .map(name => [path.basename(name), sha(fs.readFileSync(name))])),
    supplementalSamples: raw.samples.filter(sample => sample.rawSample).map(sample => {
      const body = fs.readFileSync(path.join(path.dirname(file), sample.rawSample));
      return { file: sample.rawSample, bytes: body.length, sha256: sha(body) };
    }),
  }, null, 2));
  return { report: path.join(destination, 'report.json'), summary: path.join(destination, 'summary.json'),
    samples: raw.samples.length, executionFailures: raw.failures.length,
    completeNavigation: raw.samples.filter(sample => sample.navigationMeasurementsComplete).length };
}
function campaignReport(destination, campaign) {
  const names = ['set1', 'set2', 'confirmation'];
  const summaries = names.map(name => JSON.parse(fs.readFileSync(path.join(destination, name, 'summary.json'))));
  const lines = ['# Highlighting performance campaign', '',
    'All three performance sets completed. These are standalone shipping-canvas measurements, with the release server and pinned local worker assets. Correctness findings remain separate; this is not an adoption decision or an installed-VS-Code latency claim.', '',
    '| Set | Attempted trials | Complete navigation sequences | Execution failures |',
    '| --- | ---: | ---: | ---: |',
    ...campaign.sets.map(set => `| ${set.name} | ${set.samples} | ${set.completeNavigation} | ${set.executionFailures} |`), '',
    `Performance confirmation repeated control 0 and worker ${campaign.leader}, selected by the predefined Code-heavy first-colour rule.`, '',
    'Values below are nearest-rank p50 in milliseconds, rounded for display. Counts are the actual observations per metric, not the attempted trial count. JSON and CSV retain full precision; p95 and maxima are in the linked tables. Failed/timed-out actions are not latencies.', '',
  ];
  const sections = [
    ['Initial docs', 'first.visibleMs', 'first.colouredMs', 'Visible', 'Coloured'],
    ['First Show code', 'sourceFirst.readableMs', 'sourceFirst.colouredMs', 'Readable', 'Coloured'],
    ['Retained and evicted returns', 'retained.visibleMs', 'evicted.visibleMs', 'Retained visible', 'Evicted visible'],
    ['Evicted colouring and cached Show code', 'evicted.colouredMs', 'sourceReopen.readableMs', 'Evicted coloured', 'Reopened readable'],
  ];
  for (const [title, left, right, leftLabel, rightLabel] of sections) {
    lines.push(`## ${title}`, '', `| Set | Fixture | Cache | Variant | ${leftLabel} n / p50 | ${rightLabel} n / p50 |`, '| --- | --- | --- | --- | ---: | ---: |');
    for (const [index, summary] of summaries.entries()) {
      const keys = new Map();
      for (const row of summary.metrics.filter(row => row.metric === left || row.metric === right))
        keys.set([row.shape, row.cache, row.variant].join('|'), row);
      for (const key of [...keys.keys()].sort()) {
        const [shape, cache, variant] = key.split('|');
        const format = metric => {
          const row = summary.metrics.find(row => row.shape === shape && row.cache === cache && row.variant === variant && row.metric === metric);
          return row ? `${row.n} / ${Number(row.p50.toFixed(1))}` : '—';
        };
        lines.push(`| ${names[index]} | ${shape} | ${cache === 'cold-startup' ? 'startup' : 'compiled first'} | ${variant} | ${format(left)} | ${format(right)} |`);
      }
    }
    lines.push('');
  }
  lines.push('## Complete measurements', '',
    ...names.map(name => `- ${name}: [all metric tables](${name}/metrics.md), [CSV](${name}/metrics.csv), [raw metric observations](${name}/report.json), [summary](${name}/summary.json).`), '',
    'These include initialization/tokenization, transport, DOM work, long tasks, edits, loader exposure, observed CPU, worker heaps, caches and peak/settled process RSS. Raw load observations and qualifications remain with each trial. Sources and fixtures are frozen from Set 1, and original complete reports/traces remain in the owned temporary run with hashes in provenance.json.', '',
    'Colour distributions contain completed colours only. Per-action plain fallback outcomes and settling times remain in report.json; do not interpret completed-only p95 as bounding those failures. Independently observed visibility/readability is retained even if colours fail. Edit visibleMs observes exact updated DOM; highlightSettledMs observes the completion/fallback promise and does not independently prove coloured pixels or a subsequent edit paint.', '',
    'Variant 0 is asynchronous Rust Syntect/Oniguruma; 1 is highlight.js; 2 is TextMate/Oniguruma WASM; D is the plain diagnostic. D and code-free docs have no successful colour-completion metric. See the retained preflight report for geometry/raster, token-role and freshness findings. Faster timing does not make those checks pass.', '',
  );
  fs.writeFileSync(path.join(destination, 'README.md'), lines.join('\n'));
}
module.exports = { aggregate, publish, campaignReport };
