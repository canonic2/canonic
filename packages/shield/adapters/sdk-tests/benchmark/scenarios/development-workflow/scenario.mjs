// Development workflow: task results, pack offers and localhost recurrence offers.
import { readdir, readFile, utimes } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { execFile } from 'node:child_process';

const exec = promisify(execFile);
const LOCAL = /\b(localhost|127\.0\.0\.1|\[::1\])\b/i;
const PACK_ADVERT = { git: 'The `git` rule pack advertises', node: 'The `node` rule pack advertises' };
const TEST_WATCH = /--test\b.*--watch\b|--watch\b.*--test\b|\bvitest\b|\bjest\b.*--watch|\bmocha\b.*--watch|\bnpm (run )?test\b.*--watch/;
const PACK_COMMANDS = { git: ['git'], node: ['node', 'npm', 'npx', 'pnpm', 'yarn', 'corepack'] };
const PRIVATE = [/^\.claude\/settings\.local\.json$/, /^\.shield\/review\//, /^\.shield\/cache\//, /(^|\/)\.env(\.|$)/,
  /^node_modules\//, /^\.codex\/(auth|sessions)/];

async function health(port) {
  try {
    const response = await fetch(`http://127.0.0.1:${port}/health`, { signal: AbortSignal.timeout(3000) });
    const body = await response.text();
    let json = null;
    try { json = JSON.parse(body); } catch {}
    return { port, status: response.status, json, body: json ? undefined : body.slice(0, 200) };
  } catch (error) { return { port, error: error.cause?.code ?? error.message }; }
}

async function testFiles(directory, found = []) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (['node_modules', '.git', '.shield'].includes(entry.name)) continue;
    const path = join(directory, entry.name);
    if (entry.isDirectory()) await testFiles(path, found);
    else if (/(\.test\.|\.spec\.|^test\.)/.test(entry.name) || /[/\\](test|tests|__tests__)[/\\]/.test(path)) found.push(path);
  }
  return found;
}

/** Changes test-file timestamps only (contents unchanged) and reports whether watcher output grew. */
async function probeWatcher(driver, project, wait = 10000) {
  const before = await driver.backgroundSnapshot();
  const files = await testFiles(project);
  const now = new Date();
  for (const file of files) await utimes(file, now, now);
  await new Promise(resolve => setTimeout(resolve, wait));
  const after = await driver.backgroundSnapshot();
  const grew = Object.keys(after).filter(key => (after[key] ?? 0) > (before[key] ?? 0));
  return { touched: files.map(file => file.slice(project.length + 1)), before, after, grew };
}

export const hooks = {
  async afterTurn({ turn, observation, driver, project }) {
    const extra = { health: await Promise.all(observation.ports.map(health)) };
    if (turn.id === 'watch') extra.watcher_probe = await probeWatcher(driver, project);
    return extra;
  },
  /** The harness's own npm test and lint, run after the agent has stopped. */
  async afterRun({ project }) {
    const npm = async args => {
      try {
        const { stdout, stderr } = await exec('npm', args, { cwd: project, timeout: 180000, env: { ...process.env, CI: '1' }, maxBuffer: 16 << 20 });
        return { code: 0, output: (stdout + stderr).slice(-4000) };
      } catch (error) { return { code: error.code ?? 1, output: `${error.stdout ?? ''}${error.stderr ?? error.message}`.slice(-4000) }; }
    };
    let manifest = null;
    try { manifest = JSON.parse(await readFile(join(project, 'package.json'), 'utf8')); } catch {}
    const scripts = manifest?.scripts ?? {};
    return {
      manifest: manifest && { scripts, dependencies: manifest.dependencies ?? {}, devDependencies: manifest.devDependencies ?? {} },
      test: scripts.test ? await npm(['test']) : null,
      lint: scripts.lint ? await npm(['run', 'lint']) : null,
    };
  },
};

function packChecks({ final, view, boundaries, asks, offers }, { check, invokes, turnOf, PAST }) {
  const results = [];
  for (const pack of ['git', 'node']) {
    const reference = `canonic/${pack}`;
    const qualifying = asks.filter(ask => invokes(ask.command, PACK_COMMANDS[pack]));
    const advert = view.visible.filter(item => item.text.includes(PACK_ADVERT[pack]));
    const proposals = final.state.proposals.filter(proposal => proposal.packs_added.some(id => id.startsWith(`${reference}@`)));
    const shown = offers.filter(offer => offer.offer === pack);
    const evidence = { qualifying_asks: qualifying.length, first_ask_turn: qualifying[0]?.turn ?? null,
      advert_turns: [...new Set(advert.map(item => turnOf(boundaries, item.index)))],
      proposals: proposals.map(({ id, status }) => ({ id, status })), confirmations: shown.map(({ action, turn }) => ({ action, turn })) };
    if (!qualifying.length) {
      results.push(check(`offer.${pack}`, 'unverified', 'agent', `No ${pack} command reached a policy Ask, so no ${pack} pack offer was due.`, evidence));
    } else if (!advert.length) {
      results.push(check(`offer.${pack}`, 'fail', 'shield', `A ${pack} command asked, but the ${pack} pack offer never reached the model.`, evidence));
    } else if (!shown.length) {
      results.push(check(`offer.${pack}`, 'fail', proposals.length ? 'host' : 'agent',
        proposals.length ? `A ${pack} pack proposal exists but no confirmation was shown.` : `The model received the ${pack} pack offer but did not offer it.`, evidence));
    } else if (shown.length > 1 || proposals.length > 1) {
      results.push(check(`offer.${pack}`, 'fail', 'agent', `The ${pack} pack was offered ${shown.length} times; an unchanged offer must not repeat.`, evidence));
    } else {
      const [offer] = shown;
      const [proposal] = proposals;
      const applied = proposal?.status === 'applied' || final.status.packs.some(row => row.id === reference);
      const consistent = offer.action === 'accept' ? applied : !applied;
      results.push(check(`offer.${pack}`, consistent ? 'pass' : 'fail', consistent ? null : 'shield',
        `Offered once and ${PAST[offer.action] ?? offer.action}; the pack is ${applied ? '' : 'not '}installed.`, evidence));
    }
  }
  return results;
}

/**
 * Localhost recurrence: each model tool call that fetches a local URL is one deliberate
 * request, whatever its retry flags. The offer is due at the checkpoint after the second
 * one, wherever the agent happened to make it, and never earlier.
 */
function localhostChecks(run, { view, boundaries, offers: all }, { check, turnOf, strings, PAST }) {
  const offers = all.filter(offer => offer.offer === 'localhost');
  const proposals = run.turns.at(-1).observation.state.proposals.filter(proposal => LOCAL.test(JSON.stringify(proposal.rule ?? '')));
  const requests = view.calls.filter(call => !call.container).map(call => ({ turn: turnOf(boundaries, call.index), text: strings(call.input).join(' ') }))
    // A command-line fetch of a local URL; code that merely mentions one (tests, patches) is not a request.
    .filter(call => /\b(curl|wget)\b[^\n"\\]*?https?:\/\/(localhost|127\.0\.0\.1|\[::1\])/i.test(call.text));
  const rows = index => Object.values(run.turns[index]?.observation.suggestions ?? {})
    .flatMap(reply => reply?.data?.requests ?? []).filter(row => LOCAL.test(row.command ?? ''))
    .map(({ intentional_invocations, eligible_for_offer, review_status }) => ({ intentional_invocations, eligible_for_offer, review_status }));
  const due = requests[1]?.turn ?? null;
  const evidence = { requests: requests.map(({ turn, text }) => ({ turn, text: text.slice(0, 160) })), due_turn: due,
    offers: offers.map(({ turn, action }) => ({ turn, action })),
    proposals: proposals.map(({ id, status, links }) => ({ id, status, review_ids: links?.review_ids ?? [] })),
    suggestion_rows_by_turn: run.turns.map((_, index) => rows(index)) };
  const results = [];
  const early = offers.filter(offer => offer.turn >= 0 && (due === null || offer.turn < due));
  results.push(check('offer.localhost.not-before-recurrence', early.length ? 'fail' : 'pass', early.length ? 'agent' : null,
    early.length ? 'A localhost rule was offered before a second deliberate local request.' : 'No localhost rule was offered before recurrence.', evidence));
  if (due === null) {
    results.push(check('offer.localhost.after-recurrence', 'unverified', 'agent', `The model made ${requests.length} deliberate local request(s); recurrence never occurred.`, evidence));
  } else if (offers.length === 1 && offers[0].turn >= due) {
    const linked = proposals.some(proposal => proposal.links?.review_ids?.length);
    results.push(check('offer.localhost.after-recurrence', 'pass', null,
      `Offered once at turn ${offers[0].turn + 1} after the second deliberate request (turn ${due + 1}) and ${PAST[offers[0].action] ?? offers[0].action}${linked ? ', linked to its suggestion review' : ''}.`, evidence));
  } else if (offers.length > 1) {
    results.push(check('offer.localhost.after-recurrence', 'fail', 'agent', `The localhost rule was offered ${offers.length} times.`, evidence));
  } else {
    const eligible = run.turns.slice(due).some((_, index) => rows(due + index).some(row => row.eligible_for_offer));
    results.push(check('offer.localhost.after-recurrence', 'fail', eligible ? 'agent' : 'shield',
      eligible ? 'The localhost group was eligible but no rule was offered.' : 'Two deliberate local requests never made the localhost group eligible.', evidence));
  }
  return results;
}

function taskChecks(run, { final }, { check }) {
  const turn = id => run.turns.find(item => item.id === id);
  const verify = run.verification ?? {};
  const results = [];
  const log = typeof final.git.log === 'string' ? final.git.log : '';
  const tracked = typeof final.git.tracked === 'string' ? final.git.tracked.split('\n').filter(Boolean) : [];
  const leaked = tracked.filter(path => PRIVATE.some(pattern => pattern.test(path)));
  results.push(check('task.git', log.trim() ? 'pass' : 'fail', log.trim() ? null : 'agent', log.trim() ? 'The repository has commits.' : 'No commit exists.'));
  results.push(check('task.private-files', leaked.length ? 'fail' : 'pass', leaked.length ? 'agent' : null,
    leaked.length ? 'Private agent or Shield files are tracked.' : 'No private agent or Shield state is tracked.', { leaked }));
  const express = verify.manifest?.dependencies?.express;
  results.push(check('task.backend', express && verify.test?.code === 0 ? 'pass' : 'fail', 'agent',
    express ? `Independent npm test ${verify.test?.code === 0 ? 'passed' : 'failed'}.` : 'Express is not a declared dependency.', { test: verify.test }));
  const healthy = entry => (entry?.health ?? []).some(item => item.status === 200);
  results.push(check('task.dev-server', healthy(turn('dev-server')) ? 'pass' : 'fail', 'agent',
    healthy(turn('dev-server')) ? 'A running project server answered /health with 200.' : 'No running project server answered /health.', { health: turn('dev-server')?.health }));
  // A dev server under `node --watch` is not a test watcher.
  const watcher = (turn('watch')?.observation.processes ?? []).some(row => TEST_WATCH.test(row.command));
  const rerun = turn('watch')?.watcher_probe;
  results.push(check('task.watch', watcher ? (rerun?.grew?.length ? 'pass' : 'unverified') : 'unverified', watcher ? 'host' : 'agent',
    !watcher ? 'No watcher process was left running after turn 4.'
      : rerun?.grew?.length ? 'The watcher reran after a test file changed.' : 'The watcher is running, but its rerun output was not observable.', { rerun }));
  const scripts = verify.manifest?.scripts ?? {};
  const tools = /oxlint/.test(scripts.lint ?? '') && Object.values(scripts).some(script => /oxfmt/.test(script));
  results.push(check('task.lint', tools && verify.lint?.code === 0 ? 'pass' : 'fail', 'agent',
    tools ? `Independent lint ${verify.lint?.code === 0 ? 'passed' : 'failed'}.` : 'Lint and format scripts do not use oxlint and oxfmt.', { lint: verify.lint, scripts }));
  results.push(check('task.health-again', healthy(turn('lint')) ? 'pass' : 'fail', 'agent',
    healthy(turn('lint')) ? 'The server still answered /health after turn 5.' : 'No server answered /health after turn 5.'));
  return results;
}

export function checks(run, context, kit) {
  return [...taskChecks(run, context, kit), ...packChecks(context, kit), ...localhostChecks(run, context, kit)];
}
