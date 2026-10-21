import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

function fixture(on) {
  let now = 10;
  const records = [];
  const source = readFileSync(new URL('./browser.js', import.meta.url), 'utf8');
  const api = vm.runInNewContext(
    source.replace(/^export /gm, '') + '\n({begin,phase,address,documentTrace})',
    {
      document: { querySelector: () => (on ? { content: 'true' } : null) },
      location: {
        href: 'http://example.com/docs/card.md',
        origin: 'http://example.com',
        search: '',
      },
      URL,
      URLSearchParams,
      crypto: { randomUUID: () => 'acme-selection' },
      performance: {
        timeOrigin: 1000,
        now() {
          assert.ok(on, 'disabled tracing never reads the clock');
          return now;
        },
      },
      fetch(url, options) {
        assert.ok(on, 'disabled tracing never sends diagnostics');
        records.push({ url, ...JSON.parse(options.body) });
        return Promise.resolve();
      },
    },
  );
  return {
    api,
    records,
    tick: (ms) => {
      now += ms;
    },
  };
}

void test('disabled tracing does no timing or transport work and preserves addresses', () => {
  const { api, records } = fixture(false);
  const trace = api.begin({ src: 'card.md' });
  assert.equal(trace, null);
  api.phase(trace, 'selection');
  assert.equal(api.address('card.md', trace), 'card.md');
  assert.equal(records.length, 0);
});

void test('phases preserve selection identity across documents without including page payloads', () => {
  const { api, records, tick } = fixture(true);
  const identity = { src: 'card.md', state: 'error', lens: 'html', inputs: { secret: 'PRIVATE' } };
  const trace = api.begin(identity);
  tick(20);
  api.phase(trace, 'selection');
  const renderer = api.begin(identity, trace);
  tick(30);
  api.phase(renderer, 'renderer-ready');
  assert.equal(records[0].details.durationMs, 20);
  assert.equal(records[1].details.durationMs, 30);
  assert.equal(records[1].details.elapsedMs, 50);
  assert.equal(records[1].details.selection, 'acme-selection');
  assert.equal(records[1].details.page, 'card.md');
  assert.equal(records[1].details.state, 'error');
  assert.equal(records[1].details.lens, 'html');
  assert.doesNotMatch(JSON.stringify(records), /PRIVATE/);
  assert.equal(
    new URL(api.address('card.md?lens=html', trace)).searchParams.get('wbTrace'),
    'acme-selection',
  );
  const url = new URL(api.address('card.md?lens=html', trace));
  assert.equal(url.searchParams.get('wbPage'), 'card.md');
  assert.equal(url.searchParams.get('wbLens'), 'html');
  assert.equal(url.searchParams.get('wbState'), 'error');
  assert.equal(
    api.address('https://other.example.com/page', trace),
    'https://other.example.com/page',
  );
});
