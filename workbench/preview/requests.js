// Request mocks for previews, shared by the browser runtime and the Astro
// worker. A preview's `requests` map answers the page's own fetch and
// XMLHttpRequest calls, so a page that loads its data renders unchanged.
//
//   'GET /api/customers'          one method and path
//   '/api/customers/*'            any method; * matches any characters
//   'POST /graphql Customers'     a GraphQL operation, by name
//   'https://api.example.com/me'  one origin; a bare path matches any origin
//
// A value is a response — { status, headers, body, delay, pending, failed,
// passthrough } — or a function of the request and the preview context that
// returns one, or a Response. Once a preview declares any mock, requests
// that match none answer 404 and are logged, so a preview never reaches a
// live service by accident. Workbench's own /_workbench/ requests are never
// touched.

const SAFE = ['GET', 'HEAD', 'OPTIONS'];
const escape = text => text.replace(/[.+?^${}()|[\]\\]/g, '\\$&');

export function parseKey(key) {
  const parts = String(key).trim().split(/\s+/);
  const method = parts.length > 1 && /^[A-Z]+$/.test(parts[0]) ? parts.shift() : null;
  const address = parts.shift();
  const operation = parts.shift() || null;
  const absolute = /^[a-z][a-z0-9+.-]*:\/\//i.test(address || '');
  if (!address || parts.length || (!absolute && address[0] !== '/')) {
    throw new Error('request ' + key + ' must be "[METHOD] /path [Operation]" or a full URL.');
  }
  const url = new URL(address, 'http://workbench.invalid');
  const pattern = new RegExp('^' + url.pathname.split('*').map(escape).join('.*') + '$');
  const query = Array.from(url.searchParams);
  return {
    key, method, operation,
    test: target => (!absolute || target.origin === url.origin) && pattern.test(target.pathname) &&
      query.every(([name, value]) => target.searchParams.getAll(name).includes(value)),
  };
}

// Levels run from most to least specific: a state's mocks, then its preview's.
export function requestMocks(levels) {
  const entries = [];
  for (const map of levels) {
    for (const [key, value] of Object.entries(map || {})) entries.push({ ...parseKey(key), value });
  }
  return {
    size: entries.length,
    find: request => entries.find(entry => (!entry.method || entry.method === request.method) &&
      (!entry.operation || entry.operation === request.operationName) && entry.test(request.target)),
  };
}

function parseBody(text, type) {
  if (!text) return undefined;
  if (/json/i.test(type || '') || /^\s*[[{]/.test(text)) { try { return JSON.parse(text); } catch (_) { /* text */ } }
  if (/x-www-form-urlencoded/i.test(type || '')) return Object.fromEntries(new URLSearchParams(text));
  return text;
}

// What a handler receives: the request, already parsed.
export function describe({ method, url, headers, text }) {
  const target = new URL(url);
  const body = parseBody(text, headers['content-type']);
  const graphql = body && typeof body === 'object' && !Array.isArray(body) ? body : Object.fromEntries(target.searchParams);
  let variables = graphql.variables;
  if (typeof variables === 'string') { try { variables = JSON.parse(variables); } catch (_) { /* as sent */ } }
  const named = typeof graphql.query === 'string' && /\b(?:query|mutation|subscription)\s+([A-Za-z_]\w*)/.exec(graphql.query);
  return {
    method: method.toUpperCase(), url: target.href, path: target.pathname,
    query: Object.fromEntries(target.searchParams), headers, body, target,
    operationName: typeof graphql.operationName === 'string' ? graphql.operationName : named ? named[1] : null,
    variables: variables && typeof variables === 'object' ? variables : null,
  };
}

function aborted(signal) {
  return signal?.reason instanceof Error ? signal.reason : Object.assign(new Error('The request was aborted.'), { name: 'AbortError' });
}

function wait(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) { reject(aborted(signal)); return; }
    const timer = ms === Infinity ? null : setTimeout(resolve, ms);
    signal?.addEventListener('abort', () => { clearTimeout(timer); reject(aborted(signal)); }, { once: true });
  });
}

// One signal that aborts with either: the caller's, or the render's.
function either(a, b) {
  if (!a || !b) return a || b;
  const controller = new AbortController();
  for (const signal of [a, b]) {
    if (signal.aborted) controller.abort(signal.reason);
    else signal.addEventListener('abort', () => controller.abort(signal.reason), { once: true });
  }
  return controller.signal;
}

async function respond(spec, signal) {
  if (spec instanceof Response) return spec;
  spec = spec || {};
  if (spec.delay) await wait(Number(spec.delay), signal);
  if (spec.pending) await wait(Infinity, signal);
  if (spec.failed) throw new TypeError('Failed to fetch');
  const headers = new Headers(spec.headers || {});
  let body = spec.body;
  if (body !== undefined && body !== null && typeof body !== 'string' && !(body instanceof Blob) && !(body instanceof ArrayBuffer)) {
    body = JSON.stringify(body);
    if (!headers.has('content-type')) headers.set('content-type', 'application/json');
  }
  const status = spec.status || 200;
  return new Response([101, 204, 205, 304].includes(status) ? null : body ?? null, { status, statusText: spec.statusText || '', headers });
}

// Answer one request from the active mocks. Resolves to a Response, or to
// null when the request should reach the network.
async function answer(active, request, signal) {
  const entry = active.mocks.find(request);
  const label = request.method + ' ' + request.path + (request.operationName ? ' ' + request.operationName : '');
  if (!entry) {
    active.log?.('request', label + ' — no mock');
    return respond({ status: 404, body: { error: 'No Workbench request mock for ' + label } }, signal);
  }
  // Writes are worth seeing in the log; reads are the page loading itself.
  // A GraphQL request is a write only when it is a mutation.
  const write = request.operationName ? /^\s*mutation\b/.test(String(request.body?.query || '')) : !SAFE.includes(request.method);
  if (write) active.log?.('request', label, ...(request.body === undefined ? [] : [request.body]));
  const spec = typeof entry.value === 'function' ? await entry.value(request, active.context) : entry.value;
  if (spec && !(spec instanceof Response) && spec.passthrough) return null;
  return respond(spec, either(signal, active.signal));
}

function headerObject(headers) {
  const out = {};
  headers.forEach((value, name) => { out[name.toLowerCase()] = value; });
  return out;
}

// A fetch that consults `current()` — the active mocks, or null — on every
// call. Used as window.fetch in the browser and globalThis.fetch while Astro
// renders in the worker.
export function mockedFetch(original, current, bypass = () => false) {
  return async function fetch(input, init) {
    const active = current();
    if (!active || !active.mocks.size) return original(input, init);
    let request;
    try { request = new Request(input, init); } catch (error) {
      if (!active.base) throw error;
      request = new Request(new URL(String(input?.url || input), active.base), init);
    }
    // Building the Request consumed any body `input` carried, so the network
    // gets this one; only its clone is read here.
    if (bypass(new URL(request.url))) return original(request);
    const text = SAFE.includes(request.method) ? '' : await request.clone().text();
    const described = describe({ method: request.method, url: request.url, headers: headerObject(request.headers), text });
    const response = await answer(active, described, request.signal);
    return response || original(request);
  };
}

// XMLHttpRequest, for clients that use it (axios among them). Unmocked and
// synchronous requests go to the real implementation.
function mockedXHR(Original, current, bypass) {
  return class XMLHttpRequest extends Original {
    open(method, url, async = true, ...rest) {
      this.wbRequest = { method: String(method).toUpperCase(), url: new URL(String(url), document.baseURI).href, headers: {}, async: async !== false };
      return super.open(method, url, async, ...rest);
    }
    setRequestHeader(name, value) {
      if (this.wbRequest) this.wbRequest.headers[String(name).toLowerCase()] = String(value);
      return super.setRequestHeader(name, value);
    }
    send(body) {
      const active = current();
      const sent = this.wbRequest;
      if (!active || !active.mocks.size || !sent || !sent.async || bypass(new URL(sent.url))) return super.send(body);
      const text = body == null ? '' : typeof body === 'string' ? body
        : body instanceof URLSearchParams ? body.toString() : '';
      if (body instanceof URLSearchParams && !sent.headers['content-type']) sent.headers['content-type'] = 'application/x-www-form-urlencoded';
      const controller = new AbortController();
      this.wbAbort = () => controller.abort();
      answer(active, describe({ ...sent, text }), controller.signal).then(async response => {
        if (!response) { this.wbAbort = null; super.send(body); return; }
        const content = await response.text();
        this.wbFinish({ status: response.status, statusText: response.statusText, headers: response.headers, text: content });
      }, error => {
        if (controller.signal.aborted) return;
        this.wbFinish({ status: 0, statusText: '', headers: new Headers(), text: '', failed: error });
      });
    }
    abort() {
      if (!this.wbAbort) return super.abort();
      this.wbAbort();
      this.wbAbort = null;
      this.wbFinish({ status: 0, statusText: '', headers: new Headers(), text: '', aborted: true });
    }
    wbFinish(result) {
      this.wbAbort = null;
      const responseType = this.responseType;
      let response = result.text;
      if (responseType === 'json') { try { response = result.text ? JSON.parse(result.text) : null; } catch (_) { response = null; } }
      else if (responseType === 'blob') response = new Blob([result.text], { type: result.headers.get('content-type') || '' });
      else if (responseType === 'arraybuffer') response = new TextEncoder().encode(result.text).buffer;
      else if (responseType === 'document') response = null;
      let readyState = 1;
      const define = (name, get) => Object.defineProperty(this, name, { configurable: true, get });
      define('readyState', () => readyState);
      define('status', () => result.status);
      define('statusText', () => result.statusText);
      define('responseURL', () => this.wbRequest.url);
      define('response', () => readyState === 4 ? response : null);
      define('responseText', () => readyState === 4 && typeof response === 'string' ? response : '');
      this.getResponseHeader = name => result.headers.get(name);
      this.getAllResponseHeaders = () => { let all = ''; result.headers.forEach((value, name) => { all += name + ': ' + value + '\r\n'; }); return all; };
      const fire = type => this.dispatchEvent(type === 'readystatechange' ? new Event(type) : new ProgressEvent(type));
      if (result.failed || result.aborted) {
        readyState = 4; fire('readystatechange');
        fire(result.aborted ? 'abort' : 'error'); fire('loadend');
        return;
      }
      readyState = 2; fire('readystatechange');
      readyState = 3; fire('readystatechange');
      readyState = 4; fire('readystatechange');
      fire('load'); fire('loadend');
    }
  };
}

// In a browser, replace fetch and XMLHttpRequest as soon as this module is
// evaluated: compiled previews import it before any project module, so a
// client that keeps a reference to fetch at load time gets this one.
// browser.js activates a render's mocks through window.__workbenchRequests.
if (typeof window !== 'undefined' && typeof document !== 'undefined' && !window.__workbenchRequests) {
  let active = null;
  const bypass = url => url.origin === location.origin && url.pathname.startsWith('/_workbench/');
  const fetchOriginal = window.fetch.bind(window);
  window.fetch = mockedFetch(fetchOriginal, () => active, bypass);
  if (window.XMLHttpRequest) window.XMLHttpRequest = mockedXHR(window.XMLHttpRequest, () => active, bypass);
  window.__workbenchRequests = {
    activate(levels, context, log, signal) { active = { mocks: requestMocks(levels), context, log, signal }; },
    deactivate() { active = null; },
  };
}
