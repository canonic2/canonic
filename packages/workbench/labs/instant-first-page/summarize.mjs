// Recomputes stage 1's aggregates from a report's raw samples and writes the
// summary beside it.
//
//   node labs/instant-first-page/summarize.mjs <report.json>
//
// Warmups and samples without a time are left out of the aggregates and
// counted. Percentiles are by nearest rank.
import fs from 'node:fs';
import path from 'node:path';

const file = path.resolve(process.argv[2]);
const report = JSON.parse(fs.readFileSync(file, 'utf8'));
const rank = (values, p) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(p / 100 * sorted.length) - 1)]; };
const round = value => value == null ? '—' : value >= 100 ? String(Math.round(value)) : value.toFixed(1);

const cells = new Map();
const missing = new Map();
for (const item of report.results) {
  if (item.warmup) continue;
  // B3 and B4 start from 1, 20 or 100 entries and grow by one per B3 sample.
  const held = item.held == null ? '' : String([100, 20, 1].find(base => item.held >= base));
  const key = [item.group, item.fixture, item.cell, item.mode || '', held, item.host].join('|');
  if (typeof item.ms !== 'number') { missing.set(key, (missing.get(key) || 0) + 1); continue; }
  if (!cells.has(key)) cells.set(key, { ms: [], wall: [], load: [] });
  const cell = cells.get(key);
  cell.ms.push(item.ms);
  if (typeof item.wall === 'number') cell.wall.push(item.wall);
  cell.load.push(item.load);
}
const order = ['small', 'large-system', 'giant', 'colossal'];
const rows = [...cells].map(([key, cell]) => {
  const [group, fixture, name, mode, held, host] = key.split('|');
  return { group, fixture, cell: name, mode, held, host, n: cell.ms.length, p50: rank(cell.ms, 50), p95: rank(cell.ms, 95), max: Math.max(...cell.ms),
    wall50: cell.wall.length ? rank(cell.wall, 50) : null, loadMax: Math.max(...cell.load), missing: missing.get(key) || 0 };
}).sort((a, b) => a.group.localeCompare(b.group) || order.indexOf(a.fixture) - order.indexOf(b.fixture) || a.cell.localeCompare(b.cell) || a.mode.localeCompare(b.mode) || a.held.localeCompare(b.held, undefined, { numeric: true }) || a.host.localeCompare(b.host));

const lines = [`# Instant first page lab, stage 1, ${report.settings.set}`, '', report.smoke ? 'Smoke run: not evidence.' : `Samples per cell: ${report.settings.samples}, warmups: ${report.settings.warmups}. Times in ms, p50 / p95 / max by nearest rank; wall is from spawning the process.`, '',
  '| Group | Fixture | Cell | Mode | Held | Host | n | p50 | p95 | max | wall p50 | max load | missing |', '| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |'];
for (const row of rows) lines.push(`| ${row.group} | ${row.fixture} | ${row.cell} | ${row.mode} | ${row.held} | ${row.host} | ${row.n} | ${round(row.p50)} | ${round(row.p95)} | ${round(row.max)} | ${round(row.wall50)} | ${row.loadMax.toFixed(1)} | ${row.missing} |`);
lines.push('', '## Checks', '');
const grouped = new Map();
for (const item of report.checks) { const key = [item.check, item.host || '', item.fixture, item.page || ''].join(' ').replace(/\s+/g, ' ').trim(); const entry = grouped.get(key) || { pass: 0, fail: 0 }; entry[item.ok ? 'pass' : 'fail']++; grouped.set(key, entry); }
for (const [key, value] of [...grouped].sort()) lines.push(`- ${key}: ${value.pass} pass, ${value.fail} fail`);
const loud = report.results.filter(item => item.loud).length;
lines.push('', `Samples started above the load threshold after waiting: ${loud}.`, `Load at start ${report.host.atStart.loadAverage[0]}, at end ${report.host.atEnd ? report.host.atEnd.loadAverage[0] : '—'}.`, report.failure ? 'Run failure: ' + report.failure.split('\n')[0] : '');
fs.writeFileSync(path.join(path.dirname(file), 'summary.md'), lines.join('\n') + '\n');
console.log(lines.join('\n'));
