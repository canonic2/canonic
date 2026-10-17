// Recomputes stage 2's aggregates from a report's raw samples and writes the
// summary beside it.
//
//   node labs/instant-first-page/summarize2.mjs <report.json>
//
// Warmups are left out; samples without a time are counted per cell as
// missing (timeouts and errors). Percentiles are by nearest rank.
import fs from 'node:fs';
import path from 'node:path';

const file = path.resolve(process.argv[2]);
const report = JSON.parse(fs.readFileSync(file, 'utf8'));
const rank = (values, p) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(p / 100 * sorted.length) - 1)]; };
const round = value => value == null ? '—' : value >= 100 ? String(Math.round(value)) : value.toFixed(1);
const timed = new Set(['S1', 'S2', 'S2i', 'S3', 'S4', 'S5', 'S5r', 'S6', 'S7', 'S8', 'S9-S4', 'S9-S6', 'own']);

const cells = new Map();
for (const item of report.results) {
  if (item.warmup || !timed.has(item.sequence)) continue;
  const key = [item.fixture, item.sequence, item.cell, item.variant].join('|');
  if (!cells.has(key)) cells.set(key, { ms: [], missing: 0, errors: new Set(), load: [] });
  const cell = cells.get(key);
  if (typeof item.ms === 'number') cell.ms.push(item.ms); else { cell.missing++; if (item.error) cell.errors.add(String(item.error).slice(0, 80)); }
  cell.load.push(item.load);
}
const fixtures = ['small', 'large-system', 'giant', 'colossal', 'formats'];
const sequences = [...timed];
const rows = [...cells].map(([key, cell]) => {
  const [fixture, sequence, name, variant] = key.split('|');
  return { fixture, sequence, cell: name, variant, n: cell.ms.length, missing: cell.missing, errors: [...cell.errors], p50: cell.ms.length ? rank(cell.ms, 50) : null, p95: cell.ms.length ? rank(cell.ms, 95) : null, max: cell.ms.length ? Math.max(...cell.ms) : null, load: Math.max(...cell.load) };
}).sort((a, b) => fixtures.indexOf(a.fixture) - fixtures.indexOf(b.fixture) || sequences.indexOf(a.sequence) - sequences.indexOf(b.sequence) || a.cell.localeCompare(b.cell) || a.variant.localeCompare(b.variant));

const lines = [`# Instant first page lab, stage 2, ${report.settings.set}`, '',
  report.smoke ? 'Smoke run: not evidence.' : `Samples per start cell: ${report.settings.samples} (warmups ${report.settings.warmups}); per session step: ${report.settings.session}. Times in ms from the step's own start (first page and page list from the canvas's navigation), p50 / p95 / max by nearest rank. Missing counts samples with no time: timeouts and errors.`, '',
  '| Fixture | Sequence | Cell | Variant | n | missing | p50 | p95 | max | max load |', '| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |'];
for (const row of rows) lines.push(`| ${row.fixture} | ${row.sequence} | ${row.cell} | ${row.variant} | ${row.n} | ${row.missing} | ${round(row.p50)} | ${round(row.p95)} | ${round(row.max)} | ${row.load.toFixed(1)} |`);
lines.push('', '## Checks', '');
const grouped = new Map();
for (const item of report.checks) { const key = [item.check, item.variant, item.fixture].join(' '); const entry = grouped.get(key) || { pass: 0, fail: 0 }; entry[item.ok ? 'pass' : 'fail']++; grouped.set(key, entry); }
for (const [key, value] of [...grouped].sort()) lines.push(`- ${key}: ${value.pass} pass, ${value.fail} fail`);
const errors = rows.filter(row => row.errors.length).map(row => `- ${row.fixture} ${row.sequence} ${row.cell} ${row.variant}: ${row.errors.join('; ')}`);
if (errors.length) lines.push('', '## Errors', '', ...errors);
const loud = report.results.filter(item => item.loud).length;
const retries = report.results.filter(item => item.sequence === 'browser-retry').length;
lines.push('', `Samples started above the load threshold after waiting: ${loud}. Browser launch retries: ${retries}.`, `Load at start ${report.host.atStart.loadAverage[0]}, at end ${report.host.atEnd ? report.host.atEnd.loadAverage[0] : '—'}.`, report.failure ? 'Run failure: ' + report.failure.split('\n')[0] : '');
fs.writeFileSync(path.join(path.dirname(file), 'summary.md'), lines.join('\n') + '\n');
console.log(lines.join('\n'));
