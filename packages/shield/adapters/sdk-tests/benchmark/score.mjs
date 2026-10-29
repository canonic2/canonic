// Scores a saved run with its scenario's current checks; optionally records the result.
// Usage: node benchmark/score.mjs <run-folder> [--record [--role <role>] [--note <text>]]
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { evaluate, markdown } from './lib/checks.mjs';
import { record } from './lib/results.mjs';
import { loadScenario } from './lib/scenario.mjs';

/** Writes report.json and report.md beside the run; with `recording`, also its committed summary. */
export async function score(root, recording = null) {
  const run = JSON.parse(await readFile(join(root, 'run.json'), 'utf8'));
  const scenario = await loadScenario(run.scenario_directory);
  const result = evaluate(run, scenario);
  const text = markdown(run, result);
  await writeFile(join(root, 'report.json'), `${JSON.stringify(result, null, 2)}\n`);
  await writeFile(join(root, 'report.md'), text);
  const recorded = recording ? await record(run, result, scenario, recording) : null;
  return { result, text: recorded ? `${text}\nRecorded ${recorded}` : text };
}

if (import.meta.main) {
  const { values, positionals } = parseArgs({ allowPositionals: true,
    options: { record: { type: 'boolean', default: false }, role: { type: 'string' }, note: { type: 'string' } } });
  if (!positionals[0]) throw new Error('usage: node benchmark/score.mjs <run-folder> [--record [--role <role>] [--note <text>]]');
  process.stdout.write(`${(await score(positionals[0], values.record ? values : null)).text}\n`);
}
