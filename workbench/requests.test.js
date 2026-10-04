const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const load = () => import(pathToFileURL(path.join(__dirname, 'preview/requests.js')).href);

async function mocked(levels, options = {}) {
  const { mockedFetch, requestMocks } = await load();
  const log = [];
  const network = [];
  const active = { mocks: requestMocks(levels), context: { state: 'default', fixtures: { customers: ['Ada'] } },
    log: (name, ...values) => log.push([name, ...values]), base: 'http://127.0.0.1/', signal: options.signal };
  const fetch = mockedFetch(request => { network.push(request.url); return Promise.resolve(new Response('live')); }, () => active,
    url => url.pathname.startsWith('/_workbench/'));
  return { fetch, log, network };
}

test('request keys match method, path wildcards, origin, query, and GraphQL operation', async () => {
  const { parseKey } = await load();
  const url = text => new URL(text);
  assert.ok(parseKey('GET /api/customers').test(url('http://127.0.0.1/api/customers?page=2')));
  assert.ok(parseKey('/api/customers/*').test(url('https://api.example.com/api/customers/7/notes')));
  assert.ok(!parseKey('https://api.example.com/me').test(url('https://other.example.com/me')));
  assert.ok(parseKey('/search?q=acme').test(url('http://127.0.0.1/search?q=acme&page=1')));
  assert.ok(!parseKey('/search?q=acme').test(url('http://127.0.0.1/search?q=other')));
  assert.equal(parseKey('POST /graphql Customers').operation, 'Customers');
  assert.throws(() => parseKey('api/customers'), /must be/);
});

test('mocks answer JSON, prefer the state, read the context, and keep reads out of the log', async () => {
  const { fetch, log, network } = await mocked([
    { 'GET /api/customers': { body: [] } },
    { 'GET /api/customers': { body: ['Grace'] }, 'GET /api/me': (request, context) => ({ body: { state: context.state, query: request.query } }) },
  ]);
  const response = await fetch('/api/customers');
  assert.equal(response.headers.get('content-type'), 'application/json');
  assert.deepEqual(await response.json(), []);
  assert.deepEqual(await (await fetch('https://api.example.com/api/me?team=acme')).json(), { state: 'default', query: { team: 'acme' } });
  assert.deepEqual(log, []);
  assert.deepEqual(network, []);
});

test('GraphQL operations are matched by name and only mutations are logged', async () => {
  const { fetch, log } = await mocked([{
    'POST /graphql Customers': (request) => ({ body: { data: { customers: [], first: request.variables.first } } }),
    'POST /graphql RenameCustomer': { body: { data: { renameCustomer: true } } },
  }]);
  const post = body => fetch('/graphql', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  assert.deepEqual(await (await post({ operationName: 'Customers', query: 'query Customers { customers }', variables: { first: 5 } })).json(),
    { data: { customers: [], first: 5 } });
  await post({ query: 'mutation RenameCustomer($id: ID!) { renameCustomer(id: $id) }', variables: { id: '7' } });
  assert.deepEqual(log.map(entry => entry.slice(0, 2)), [['request', 'POST /graphql RenameCustomer']]);
});

test('writes and unmatched requests are logged; unmatched ones answer 404 and Workbench requests pass', async () => {
  const { fetch, log, network } = await mocked([{ 'POST /api/customers': { status: 201, body: { id: 7 } } }]);
  const created = await fetch('/api/customers', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"name":"Ada"}' });
  assert.equal(created.status, 201);
  const missing = await fetch('/api/invoices');
  assert.equal(missing.status, 404);
  assert.match((await missing.json()).error, /GET \/api\/invoices/);
  assert.equal(await (await fetch('http://127.0.0.1/_workbench/log')).text(), 'live');
  assert.deepEqual(log, [['request', 'POST /api/customers', { name: 'Ada' }], ['request', 'GET /api/invoices — no mock']]);
  assert.deepEqual(network, ['http://127.0.0.1/_workbench/log']);
});

test('pending, failed, delayed, and passthrough responses', async () => {
  const render = new AbortController();
  const { fetch, network } = await mocked([{
    '/api/slow': { pending: true }, '/api/offline': { failed: true }, '/api/later': { delay: 5, body: 'ok' }, '/api/live': { passthrough: true },
  }], { signal: render.signal });
  await assert.rejects(fetch('/api/offline'), TypeError);
  assert.equal(await (await fetch('/api/later')).text(), 'ok');
  assert.equal(await (await fetch('/api/live')).text(), 'live');
  assert.deepEqual(network, ['http://127.0.0.1/api/live']);
  const slow = fetch('/api/slow');
  const caller = new AbortController();
  const cancelled = fetch('/api/slow', { signal: caller.signal });
  caller.abort();
  await assert.rejects(cancelled, /abort/i);
  render.abort();
  await assert.rejects(slow, /abort/i);
});
