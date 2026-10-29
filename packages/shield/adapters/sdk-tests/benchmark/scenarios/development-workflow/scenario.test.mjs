// Oracle behavior on synthetic run records; no agent, network or Shield process.
import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluate } from '../../lib/checks.mjs';
import { offerClassifier } from '../../lib/decisions.mjs';
import { loadScenario } from '../../lib/scenario.mjs';

const scenario = await loadScenario('development-workflow');
const ids = ['git', 'backend', 'dev-server', 'watch', 'lint'];
const minute = index => new Date(Date.UTC(2026, 9, 27, 12, index * 10)).toISOString();
const rule = { id: 'acme-local-health', decision: 'allow', conditions: { domain: 'web', url: { prefix: 'http://localhost:3000/' } } };

function observation(index, overrides = {}) {
  return {
    history: { entries: [] }, status: { packs: [{ id: 'canonic/baseline' }] },
    state: { proposals: [] }, custom_rules: [], suggestions: {},
    git: { log: 'abc\tInitial commit\nREADME.md\n', tracked: 'README.md\n.shield/config.yml\n', status: '' },
    config: { '.mcp.json': 'mcp', '.claude/settings.local.json': 'local' },
    processes: index >= 2 ? [{ pid: 10, command: 'node server.js' }, ...(index >= 3 ? [{ pid: 11, command: 'node --test --watch' }] : [])] : [],
    ports: index >= 2 ? [3000] : [], tool_calls: 0,
    ...overrides,
  };
}

/** A complete Claude run where every expected offer happens exactly once. */
function passingRun() {
  const ask = (index, executable, command) => ({ entry_id: `e${index}${executable}`, requested_at: minute(index + 0.1),
    evaluation: { decision: 'ask' }, request: { executable }, command });
  const history = { entries: [ask(0, 'git', 'git init'), ask(1, 'npm', 'npm install express'), ask(2, 'curl', 'curl -s http://localhost:3000/health'), ask(4, 'curl', 'curl -s http://localhost:3000/health')] };
  const proposals = [
    { id: 'p1', status: 'applied', packs_added: ['canonic/git@0.7.0:recommended'], rule: null },
    { id: 'p2', status: 'pending', packs_added: ['canonic/node@0.3.1:recommended'], rule: null },
    { id: 'p3', status: 'applied', packs_added: [], rule_id: rule.id, rule },
  ];
  const turns = ids.map((id, index) => ({ id, started_at: minute(index), finished_at: minute(index + 0.9), transcript_length: (index + 1) * 10,
    observation: observation(index, { history }), health: index >= 2 ? [{ port: 3000, status: 200, json: { status: 'ok' } }] : [] }));
  turns[2].observation.suggestions = { s: { data: { requests: [{ command: 'curl -s http://localhost:3000/health', intentional_invocations: 1, eligible_for_offer: false }] } } };
  turns[4].observation.suggestions = { s: { data: { requests: [{ command: 'curl -s http://localhost:3000/health', intentional_invocations: 2, eligible_for_offer: false, review_status: 'proposed' }] } } };
  Object.assign(turns[4].observation, { state: { proposals }, custom_rules: [rule], status: { packs: [{ id: 'canonic/baseline' }, { id: 'canonic/git' }] } });
  turns[3].watcher_probe = { grew: ['/tmp/acme/task.output'] };
  const lines = [];
  for (let index = 0; index < 50; index++) lines.push({ type: 'system', content: 'filler' });
  lines[1] = { type: 'attachment', attachment: { content: 'The `git` rule pack advertises this command; profiles may still ask.' } };
  lines[11] = { type: 'attachment', attachment: { content: 'The `node` rule pack advertises this command; profiles may still ask.' } };
  lines[21] = { type: 'assistant', message: { content: [{ type: 'tool_use', name: 'Bash', input: { command: 'curl -s http://localhost:3000/health' } }] } };
  lines[41] = { type: 'assistant', message: { content: [{ type: 'tool_use', name: 'Bash', input: { command: 'curl -s http://localhost:3000/health' } }] } };
  return {
    label: 'acme', host: 'claude', scenario: 'development-workflow', decision_set: 'claude-candidate',
    decision_table: scenario.decisions['claude-candidate'], identities: { shield_version: 'shield 0.9.0', shield_sha256: 'a'.repeat(64) },
    initial_config: { '.mcp.json': 'mcp', '.claude/settings.local.json': 'local' }, turns, transcript_lines: lines,
    decision_log: [
      { kind: 'offer', offer: 'git', action: 'accept', at: minute(0.5) },
      { kind: 'offer', offer: 'node', action: 'decline', at: minute(1.5) },
      { kind: 'offer', offer: 'localhost', action: 'accept', at: minute(4.5) },
    ],
    verification: { manifest: { dependencies: { express: '^5.0.0' }, scripts: { test: 'node --test', lint: 'oxlint .', format: 'oxfmt .' } }, test: { code: 0 }, lint: { code: 0 } },
    cleanup: { stopped: [], remaining: [] },
  };
}

const status = (result, id) => result.checks.find(item => item.id === id)?.status;

test('a run with each expected offer once passes every check', () => {
  const result = evaluate(passingRun(), scenario);
  assert.equal(result.verdict, 'pass', JSON.stringify(result.checks.filter(item => item.status !== 'pass'), null, 2));
  assert.deepEqual(result.friction.map(row => row.ask), [1, 1, 1, 0, 1]);
});

test('a repeated pack offer fails and is attributed to the agent', () => {
  const run = passingRun();
  run.decision_log.push({ kind: 'offer', offer: 'git', action: 'accept', at: minute(1.2) });
  const result = evaluate(run, scenario);
  assert.equal(status(result, 'offer.git'), 'fail');
  assert.equal(result.checks.find(item => item.id === 'offer.git').attribution, 'agent');
});

test('a pack offer that never reaches the model is a Shield failure', () => {
  const run = passingRun();
  run.transcript_lines[1] = { type: 'system', content: 'filler' };
  const result = evaluate(run, scenario);
  assert.equal(status(result, 'offer.git'), 'fail');
  assert.equal(result.checks.find(item => item.id === 'offer.git').attribution, 'shield');
});

test('a localhost offer after the first request fails', () => {
  const run = passingRun();
  run.decision_log.push({ kind: 'offer', offer: 'localhost', action: 'decline', at: minute(2.5) });
  assert.equal(status(evaluate(run, scenario), 'offer.localhost.not-before-recurrence'), 'fail');
});

test('the localhost offer is due after the second deliberate request, whichever turn makes it', () => {
  const run = passingRun();
  // The agent verifies /health in turn 2 as well, so turn 3 is the second request.
  run.transcript_lines[13] = { type: 'assistant', message: { content: [{ type: 'tool_use', name: 'Bash', input: { command: 'curl -si --retry 5 http://127.0.0.1:3917/health' } }] } };
  run.decision_log.find(item => item.offer === 'localhost').at = minute(2.5);
  const result = evaluate(run, scenario);
  assert.equal(status(result, 'offer.localhost.not-before-recurrence'), 'pass');
  assert.equal(status(result, 'offer.localhost.after-recurrence'), 'pass');
});

test('an eligible localhost group without an offer fails', () => {
  const run = passingRun();
  run.decision_log = run.decision_log.filter(item => item.offer !== 'localhost');
  run.turns[4].observation.state.proposals.pop();
  run.turns[4].observation.custom_rules = [];
  run.turns[4].observation.suggestions.s.data.requests[0] = { command: 'curl -s http://localhost:3000/health', intentional_invocations: 2, eligible_for_offer: true };
  assert.equal(status(evaluate(run, scenario), 'offer.localhost.after-recurrence'), 'fail');
});

test('an active rule that differs from the approved proposal fails', () => {
  const run = passingRun();
  run.turns[4].observation.custom_rules = [{ ...rule, conditions: { domain: 'web', url: { prefix: 'http://' } } }];
  assert.equal(status(evaluate(run, scenario), `policy.rule.${rule.id}`), 'fail');
});

test('tracked private agent settings fail the private-files check', () => {
  const run = passingRun();
  run.turns[4].observation.git.tracked += '.claude/settings.local.json\n';
  assert.equal(status(evaluate(run, scenario), 'task.private-files'), 'fail');
});

test('a watcher without observable reruns is unverified, not passed', () => {
  const run = passingRun();
  run.turns[3].watcher_probe = { grew: [] };
  assert.equal(status(evaluate(run, scenario), 'task.watch'), 'unverified');
});

test('an answer that differs from the scenario is a harness failure', () => {
  const run = passingRun();
  run.decision_log[0] = { kind: 'offer', offer: 'other', action: 'decline', at: minute(0.5), text: 'Add the git rule pack — version 0.7.0' };
  const result = evaluate(run, scenario);
  assert.equal(status(result, 'harness.decisions-followed'), 'fail');
  assert.equal(result.checks.find(item => item.id === 'offer.git').evidence.confirmations[0].action, 'decline');
});

test('confirmations name their offer from the scenario patterns', () => {
  const classify = offerClassifier(scenario.offers);
  assert.equal(classify('Install canonic/git 0.7.0 with the Recommended profile'), 'git');
  assert.equal(classify('Approve this Shield policy change?\n\nAdd the git rule pack — version 0.7.0'), 'git');
  assert.equal(classify('Add the node rule pack — version 0.3.1, recommended profile.'), 'node');
  assert.equal(classify('Add rule allowing GET http://localhost:3000/health'), 'localhost');
  assert.equal(classify('Add rule acme-test for npm test'), 'other');
});
