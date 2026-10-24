/* Discovers and evaluates preview definitions and workbench.config.ts with
   esbuild, for the page index's evaluator and the Node evaluator. Previews,
   docs lenses and portable output build in workbench-server's native
   compiler. Each platform VSIX carries its esbuild; projects install none. */
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const esbuild = require('./engine.cjs');
const picomatch = require('picomatch');
const ignore = require('ignore');
const API = path.join(__dirname, "../../../sdk/api.js");
const OMIT = new Set(['node_modules', '.git', '.canonic', '.claude', '.codex', 'dist', 'build', 'coverage', '.next', '.astro']);
const inside = (root, file) => {
  if (!fs.existsSync(file)) {
    const relative = path.relative(root, file);
    return relative === '' || (!relative.startsWith('..' + path.sep) && relative !== '..' && !path.isAbsolute(relative));
  }
  if (fs.existsSync(root)) root = fs.realpathSync(root);
  if (fs.existsSync(file)) file = fs.realpathSync(file);
  const relative = path.relative(root, file);
  return relative === '' || (!relative.startsWith('..' + path.sep) && relative !== '..' && !path.isAbsolute(relative));
};
const slash = file => file.split(path.sep).join('/');
const stamp = file => { try { const s = fs.statSync(file); return s.mtimeMs + ':' + s.size; } catch (_) { return 'missing'; } };

/* The project's .gitignore files leave out what Git would, as the native
   index does: each applies to its own folder, the deepest one with an opinion
   decides, and nothing inside an ignored folder comes back. Only regular
   .gitignore files count; global and repository-local excludes do not. */
function gitignoreIn(dir) {
  const file = path.join(dir, '.gitignore');
  try { if (!fs.lstatSync(file).isFile()) return null; } catch (_) { return null; }
  return { dir, rules: ignore().add(fs.readFileSync(file, 'utf8')) };
}
function gitignored(stack, file, isDirectory) {
  for (let at = stack.length - 1; at >= 0; at--) {
    const result = stack[at].rules.test(slash(path.relative(stack[at].dir, file)) + (isDirectory ? '/' : ''));
    if (result.ignored) return true;
    if (result.unignored) return false;
  }
  return false;
}

function discover(root, include) {
  const matches = picomatch(include || ['**/*.workbench.ts', '**/*.workbench.tsx']);
  const files = [];
  function walk(dir, stack) {
    const own = gitignoreIn(dir);
    if (own) stack = stack.concat(own);
    for (const item of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (item.name.startsWith('.') || OMIT.has(item.name) || item.isSymbolicLink()) continue;
      const file = path.join(dir, item.name);
      if (gitignored(stack, file, item.isDirectory())) continue;
      if (item.isDirectory()) walk(file, stack);
      else if (matches(slash(path.relative(root, file)))) files.push(file);
    }
  }
  walk(root, []);
  return files;
}

// A request for one known definition does not need to walk the whole project.
// Apply the same glob, hidden-folder, output-folder, symlink and .gitignore
// rules as index.
function discoveredFile(root, file, include) {
  const relative = path.relative(root, file);
  if (!picomatch(include || ['**/*.workbench.ts', '**/*.workbench.tsx'])(slash(relative))) return false;
  const names = relative.split(path.sep);
  let current = root;
  let stack = [];
  return names.every((name, at) => {
    const own = gitignoreIn(current);
    if (own) stack = stack.concat(own);
    current = path.join(current, name);
    if (name.startsWith('.') || OMIT.has(name) || fs.lstatSync(current).isSymbolicLink()) return false;
    return !gitignored(stack, current, at + 1 < names.length);
  });
}

const isMap = value => !!value && typeof value === 'object' && !Array.isArray(value);
// Scenario, artboard and iteration IDs: a lowercase letter first, then
// lowercase letters and digits in single-hyphen parts (`compact-table`).
const NESTED_ID = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;
// The label a scenario, artboard or iteration shows without its own, in
// sentence case as configuration keys get theirs: `compact-table` → `Compact table`.
const titled = id => { const words = id.split('-').join(' '); return words.charAt(0).toUpperCase() + words.slice(1); };

// The checks below throw the problem alone; `validate` names the file once.
function checkRequests(where, requests) {
  if (requests === undefined) return;
  if (!isMap(requests)) throw new Error(where + 'requests must be a map.');
  for (const [key, value] of Object.entries(requests)) {
    if (!/^(?:[A-Z]+\s+)?(?:\/\S*|[a-z][a-z0-9+.-]*:\/\/\S+)(?:\s+[A-Za-z_]\w*)?$/.test(key.trim())) throw new Error(where + 'request ' + key + ' must be "[METHOD] /path [Operation]" or a full URL.');
    if (typeof value !== 'function' && !isMap(value)) throw new Error(where + 'request ' + key + ' must be a response object or a function.');
  }
}

function checkSource(file, root, source, where = '') {
  if (!source || typeof source.entry !== 'string' || !source.entry.trim()) throw new Error(where + 'source.entry must name a local source file.');
  const target = path.resolve(path.dirname(file), source.entry);
  if (!inside(root, target) || !fs.existsSync(target) || !inside(fs.realpathSync(root), fs.realpathSync(target))) throw new Error(where + 'source must exist inside the project: ' + source.entry);
}

function checkSerializable(where, owner) {
  for (const key of ['inputs', 'fixtures', 'globals']) {
    if (owner[key] === undefined) continue;
    try { JSON.stringify(owner[key]); } catch (error) { throw new Error(where + key + ' must be JSON-serializable: ' + error.message); }
  }
}

// Iterations of one artboard: `where` names the scenario and, for a declared
// artboard, the artboard, so a problem points at its location.
function checkIterations(file, root, where, iterations) {
  if (iterations === undefined) return;
  if (!isMap(iterations)) throw new Error(where + ': iterations must be a map.');
  for (const [id, iteration] of Object.entries(iterations)) {
    if (!NESTED_ID.test(id)) throw new Error(where + ': iteration id “' + id + '” must be kebab-case.');
    const at = where + ' › iteration ' + id + ': ';
    if (!isMap(iteration)) throw new Error(at + 'must be an object.');
    for (const key of ['scenarios', 'artboards', 'iterations']) if (iteration[key] !== undefined) throw new Error(at + 'cannot declare ' + key + '.');
    if (iteration.label !== undefined && (typeof iteration.label !== 'string' || !iteration.label.trim())) throw new Error(at + 'label must be nonempty text.');
    // Merged key by key and called at render: a wrong shape fails here, by name.
    for (const key of ['inputs', 'fixtures', 'globals']) if (iteration[key] !== undefined && !isMap(iteration[key])) throw new Error(at + key + ' must be a map.');
    for (const key of ['setup', 'play', 'ready']) if (iteration[key] !== undefined && typeof iteration[key] !== 'function') throw new Error(at + key + ' must be a function.');
    if (iteration.source !== undefined) checkSource(file, root, iteration.source, at);
    checkSerializable(at, iteration);
    checkRequests(at, iteration.requests);
  }
}

const ARTBOARD_KEYS = ['label', 'width', 'height', 'inputs', 'iterations'];

function checkArtboards(file, root, where, artboards) {
  if (!isMap(artboards) || !Object.keys(artboards).length || Object.keys(artboards).length > 32) throw new Error(where + ': artboards must be a map of 1–32 artboards.');
  for (const [id, board] of Object.entries(artboards)) {
    if (!NESTED_ID.test(id)) throw new Error(where + ': artboard id “' + id + '” must be kebab-case.');
    const at = where + ' › artboard ' + id;
    if (!isMap(board)) throw new Error(at + ': must be an object.');
    // Anything else on an artboard would do nothing; an iteration carries the rest.
    const extra = Object.keys(board).find(key => !ARTBOARD_KEYS.includes(key));
    if (extra) throw new Error(at + ': “' + extra + '” is not an artboard field; an artboard takes label, width, height, inputs and iterations.');
    if (![board.width, board.height].every(value => Number.isInteger(value) && value >= 1 && value <= 8192)) throw new Error(at + ': width and height must be integers from 1 to 8192.');
    if (board.label !== undefined && (typeof board.label !== 'string' || !board.label.trim())) throw new Error(at + ': label must be nonempty text.');
    if (board.inputs !== undefined && !isMap(board.inputs)) throw new Error(at + ': inputs must be a map.');
    checkSerializable(at + ': ', board);
    checkIterations(file, root, at, board.iterations);
  }
}

// The native index reports the same text (catalog/answer.rs).
const RESERVED_PATH = 'a preview’s path can’t contain “:”, “+”, “!”, or “~” — they mark the scenario, the iteration, the example, and the lens in the address.';

// The definition's fields checked in order, each problem without the file:
// the page index names it, by its path from the project root.
function checkDefinition(raw, file, root) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('default export must be definePreview({...}).');
  if (typeof raw.id !== 'string' || !/^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/.test(raw.id)) throw new Error('id must use kebab-case segments separated by /.');
  if (typeof raw.adapter !== 'string' || !/^[a-z0-9-]+$/.test(raw.adapter)) throw new Error('adapter must be a registered kebab-case name.');
  if (raw.icon !== undefined && (typeof raw.icon !== 'string' || !/^[a-z0-9-]+$/.test(raw.icon))) throw new Error('icon must be a kebab-case Lucide icon name.');
  const sources = [raw.source, ...Object.values(raw.scenarios || {}).map(scenario => scenario.source).filter(Boolean)];
  for (const source of sources) checkSource(file, root, source);
  for (const key of ['artboards', 'iterations']) if (raw[key] !== undefined) throw new Error(key + ' must be declared inside a scenario, as scenarios.<id>.' + key + '.');
  if (raw.scenarios !== undefined && (!raw.scenarios || typeof raw.scenarios !== 'object' || Array.isArray(raw.scenarios))) throw new Error('scenarios must be a map.');
  const scenarios = raw.scenarios && Object.keys(raw.scenarios).length ? raw.scenarios : { default: {} };
  for (const [id, scenario] of Object.entries(scenarios)) {
    if (!NESTED_ID.test(id) || !scenario || typeof scenario !== 'object' || Array.isArray(scenario)) throw new Error('scenarios need kebab-case IDs and object definitions.');
    checkSerializable('scenario ' + id + ' ', scenario);
    if (scenario.artboards !== undefined && scenario.iterations !== undefined) throw new Error('scenario ' + id + ': iterations of declared artboards belong to each artboard; a scenario with artboards cannot declare iterations.');
    if (scenario.artboards !== undefined) checkArtboards(file, root, 'scenario ' + id, scenario.artboards);
    checkIterations(file, root, 'scenario ' + id, scenario.iterations);
  }
  /* Keys only; the server resolves them against the space it shows the preview in. */
  if (raw.sizes !== undefined && (!Array.isArray(raw.sizes) || !raw.sizes.length || raw.sizes.some(v => typeof v !== 'string' || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(v) || /^[0-9-]+$/.test(v)))) throw new Error('sizes must be a list of size keys, such as laptop or mobile.');
  for (const [name, control] of Object.entries(raw.controls || {})) {
    if (!control || !['text', 'number', 'boolean', 'select', 'json'].includes(control.type)) throw new Error('invalid control ' + name);
    if (control.type === 'select' && (!Array.isArray(control.options) || !control.options.length)) throw new Error('select controls need options.');
  }
  checkRequests('', raw.requests);
  for (const [id, scenario] of Object.entries(scenarios)) checkRequests('scenario ' + id + ' ', scenario.requests);
  if (raw.links !== undefined && (!raw.links || typeof raw.links !== 'object' || Array.isArray(raw.links))) throw new Error('links must be a map.');
  for (const [href, to] of Object.entries(raw.links || {})) {
    const target = typeof to === 'string' ? { preview: to } : to;
    const valid = target && typeof target === 'object' && !Array.isArray(target) && (target.preview || target.scenario)
      && (target.preview === undefined || (typeof target.preview === 'string' && /^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/.test(target.preview)))
      && (target.scenario === undefined || (typeof target.scenario === 'string' && NESTED_ID.test(target.scenario)));
    if (!valid) throw new Error('link ' + href + ' must name a preview ID, or { preview, scenario }.');
  }
  return Object.assign({}, raw, { scenarios });
}

// A definition validated for a render or a request, its problem named by the
// definition's path from the project root.
function validate(raw, file, root) {
  try { return checkDefinition(raw, file, root); } catch (error) {
    error.message = slash(path.relative(root, file)) + ': ' + error.message;
    throw error;
  }
}

// Astro frontmatter runs here, in Node, so its fetch calls get the same
// request mocks the browser does. Background builds and requested renders
// share globalThis.fetch, so every render must wait for the previous one.
let astroRenderQueue = Promise.resolve();
// `position`, from iterationsOf, renders one iteration of the scenario; one
// with `id: null` and an empty `iteration` renders a declared artboard of it.
function mockFetches(definition, scenario, inputs, render, position = null) {
  const pending = astroRenderQueue.then(() => renderWithFetches(definition, scenario, inputs, render, position));
  astroRenderQueue = pending.then(() => undefined, () => undefined);
  return pending;
}

async function renderWithFetches(definition, scenario, inputs, render, position) {
  // The pipeline also renders a base source named "default", even when
  // the preview's authored scenarios use other IDs. That base has no overrides.
  const spec = definition.scenarios[scenario] || {};
  const own = position ? position.iteration : {};
  if (!own.requests && !spec.requests && !definition.requests) return render();
  const { mockedFetch, requestMocks } = await import(require('node:url').pathToFileURL(path.join(__dirname, "../../../ui/preview/requests.js")).href);
  const original = globalThis.fetch;
  const context = { id: definition.id, scenario, ...(position && position.artboard ? { artboard: position.artboard } : {}), ...(position && position.id ? { iteration: position.id } : {}),
    inputs, fixtures: { ...definition.fixtures, ...spec.fixtures, ...own.fixtures }, globals: { ...definition.globals, ...spec.globals, ...own.globals } };
  // A server render awaits every fetch, so `pending` would hang it; give up
  // after ten seconds with a rendering error instead.
  const active = { mocks: requestMocks([own.requests, spec.requests, definition.requests]), context, base: 'http://localhost/', signal: AbortSignal.timeout(10000) };
  globalThis.fetch = mockedFetch(original, () => active);
  try { return await render(); } finally { globalThis.fetch = original; }
}

class Compiler {
  constructor(root, options = {}) {
    this.root = path.resolve(root);
    this.options = options;
    this.config = {};
    this.configFiles = new Map();
    this.definitions = new Map();
    // The definitions build in progress, which other callers wait for.
    this.loading = null;
  }
  tracked(meta) {
    return Object.keys(meta.inputs).filter(file => !file.startsWith('workbench:')).map(file => path.resolve(this.root, file)).filter(file => fs.existsSync(file));
  }
  fresh(files) { return Array.from(files).every(([file, value]) => stamp(file) === value); }
  apiPlugin() {
    return { name: 'workbench-api', setup(build) {
      build.onResolve({ filter: /^@canonic2\/workbench$/ }, () => ({ path: API }));
    } };
  }
  evaluationOptions() {
    return { absWorkingDir: this.root, bundle: true, platform: 'node', format: 'cjs', packages: 'external', write: false, metafile: true, logLevel: 'silent', plugins: [this.apiPlugin()] };
  }
  run(text, file) {
    const loaded = new Module(file);
    loaded.filename = file;
    loaded.paths = Module._nodeModulePaths(path.dirname(file));
    loaded._compile(text, file);
    return loaded.exports.default || loaded.exports;
  }
  async evaluate(file) {
    const result = await esbuild.build({ ...this.evaluationOptions(), entryPoints: [file] });
    return { value: this.run(result.outputFiles[0].text, file), files: this.tracked(result.metafile) };
  }
  // Evaluates every definition whose cached result is stale in one esbuild
  // build. A build costs tens of milliseconds however little it holds, so one
  // build per definition made listing a large project take seconds. A build
  // that fails is split in halves, so a broken definition costs a few more
  // builds rather than one per file. The files left, and a definition that
  // throws, go to definition(), which evaluates them alone and reports their
  // own errors. Callers at the same time, such as the page list and
  // pre-warming at start, wait for the build in progress and then evaluate
  // only what it left stale, so an edit made meanwhile is still picked up.
  async load(files) {
    // The caller that started the build handles its failure; others only wait for it.
    while (this.loading) await this.loading.catch(() => {});
    const stale = files.filter(file => { const cached = this.definitions.get(file); return !cached || !this.fresh(cached.watched); });
    if (stale.length < 2) return;
    this.loading = this.loadBatch(stale).finally(() => { this.loading = null; });
    await this.loading;
  }
  async loadBatch(files) {
    let result;
    try {
      result = await esbuild.build({ ...this.evaluationOptions(), entryPoints: files, outdir: path.join(this.root, '.workbench-definitions'), outbase: this.root });
    } catch (_) {
      if (files.length < 2) return;
      const half = Math.ceil(files.length / 2);
      await this.loadBatch(files.slice(0, half));
      await this.loadBatch(files.slice(half));
      return;
    }
    const pending = new Set(files);
    const texts = new Map(result.outputFiles.map(output => [output.path, output.text]));
    for (const [output, meta] of Object.entries(result.metafile.outputs)) {
      if (!meta.entryPoint) continue;
      const file = path.resolve(this.root, meta.entryPoint);
      const text = texts.get(path.resolve(this.root, output));
      if (!pending.has(file) || text === undefined) continue;
      let value;
      try { value = this.run(text, file); } catch (_) { continue; }
      const evaluated = { value, files: this.tracked(meta) };
      this.definitions.set(file, { evaluated, watched: new Map(evaluated.files.map(file => [file, stamp(file)])) });
    }
  }
  async definition(file) {
    const cached = this.definitions.get(file);
    if (cached && this.fresh(cached.watched)) return cached.evaluated;
    const evaluated = await this.evaluate(file);
    this.definitions.set(file, { evaluated, watched: new Map(evaluated.files.map(file => [file, stamp(file)])) });
    return evaluated;
  }
  async settings() {
    const file = path.resolve(this.root, this.options.config || 'workbench.config.ts');
    if (!inside(this.root, file)) throw new Error('Preview config must be inside the project.');
    if (!this.configFiles.has(file) || !this.fresh(this.configFiles)) {
      const evaluated = fs.existsSync(file) ? await this.evaluate(file) : { value: {}, files: [] };
      this.config = evaluated.value;
      const files = evaluated.files;
      this.configFiles = new Map([file, ...files].map(file => [file, stamp(file)]));
    }
    return this.config;
  }
  async index() {
    await this.settings();
    const previews = [];
    const docs = [];
    const errors = [];
    const ids = new Set();
    const files = discover(this.root, this.options.include);
    // An address could not name it: these marks split the address. Such a
    // definition is a problem whatever it holds, so it is not evaluated.
    const reserved = file => /[:+!~]/.test(slash(path.relative(this.root, file)));
    await this.load(files.filter(file => !reserved(file)));
    for (const file of files) {
      try {
        if (reserved(file)) throw new Error(RESERVED_PATH);
        const evaluated = await this.definition(file);
        const { entry, docs: isDocs } = projectDefinition(evaluated.value, file, this.root);
        if (ids.has(entry.id)) throw new Error('Duplicate preview id: ' + entry.id);
        ids.add(entry.id);
        (isDocs ? docs : previews).push(entry);
      } catch (error) { errors.push(slash(path.relative(this.root, file)) + ': ' + error.message); }
    }
    return { previews, docs, errors };
  }
  // The absolute path of a definition discovery would list. Nothing else is
  // evaluated: not files outside the project, excluded folders or symlinks.
  declared(file) {
    file = path.resolve(this.root, file);
    if (!inside(this.root, file) || !fs.existsSync(file) || !/\.workbench\.tsx?$/.test(file) || !inside(fs.realpathSync(this.root), fs.realpathSync(file))) throw new Error('Not a declared Workbench preview definition.');
    if (!discoveredFile(this.root, file, this.options.include)) throw new Error('Not a declared Workbench preview definition.');
    return file;
  }
}

// Every iteration of one scenario, with its compiled source key:
// `<scenario>+<iteration>` for the implicit artboard, `<scenario>+<artboard>/<iteration>`
// for a declared one. The key is the same the native compiler gives a source.
function iterationsOf(scenario, spec) {
  const found = [];
  for (const [id, iteration] of Object.entries((spec && spec.iterations) || {})) found.push({ key: scenario + '+' + id, artboard: null, artboardInputs: undefined, id, iteration });
  for (const [artboard, board] of Object.entries((spec && spec.artboards) || {})) {
    for (const [id, iteration] of Object.entries(board.iterations || {})) found.push({ key: scenario + '+' + artboard + '/' + id, artboard, artboardInputs: board.inputs, id, iteration });
  }
  return found;
}

// `{ [key]: [...] }` for a nonempty map of a scenario or artboard, else nothing.
const listed = (key, map, item) => (map && Object.keys(map).length ? { [key]: Object.entries(map).map(item) } : {});
// An iteration's own source is listed from the project root, as the entry's is.
const listedIteration = (file, root) => ([id, spec]) => ({ id, label: spec.label || titled(id),
  ...(spec.source && spec.source.entry ? { source: slash(path.relative(root, path.resolve(path.dirname(file), spec.source.entry))) } : {}) });

// The list contract shared by the worker and the native index's evaluator.
// Duplicate IDs belong to the caller, after discovery ordering. Each scenario
// lists its declared artboards and iterations; iterations of a declared
// artboard are listed on it, those of the implicit artboard on the scenario.
function projectDefinition(raw, file, root) {
  if (raw && raw.kind === 'docs') return { docs: true, entry: validateDocs(raw, file, root) };
  const definition = checkDefinition(raw, file, root);
  const iteration = listedIteration(file, root);
  return { docs: false, entry: {
    id: definition.id, title: definition.title || definition.id, adapter: definition.adapter,
    file: slash(path.relative(root, file)),
    source: slash(path.relative(root, path.resolve(path.dirname(file), definition.source.entry))),
    scenarios: Object.entries(definition.scenarios).map(([id, scenario]) => ({ id, label: scenario.label || titled(id),
      ...listed('artboards', scenario.artboards, ([board, spec]) => ({ id: board, label: spec.label || titled(board), width: spec.width, height: spec.height,
        ...listed('iterations', spec.iterations, iteration) })),
      ...listed('iterations', scenario.iterations, iteration) })),
    controls: definition.controls || {}, sizes: definition.sizes, docs: definition.docs || null,
    ...(definition.icon ? { icon: definition.icon } : {}),
    links: Object.fromEntries(Object.entries(definition.links || {}).map(([href, to]) => [href,
      typeof to === 'string' ? { preview: to } : { preview: to.preview || definition.id, ...(to.scenario ? { scenario: to.scenario } : {}) }])),
  } };
}

// A defineDocs({...}) definition, with its paths made relative to the project
// root so it reads like a docs page declared in workbench.yaml.
function validateDocs(raw, file, root) {
  const id = /^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/;
  const key = /^[a-z0-9-]+$/;
  if (typeof raw.id !== 'string' || !id.test(raw.id)) throw new Error('id must use kebab-case segments separated by /.');
  const local = (value, what, check) => {
    if (typeof value !== 'string' || !value.trim()) throw new Error(what + ' must be a path relative to the definition.');
    const target = path.resolve(path.dirname(file), value);
    if (!inside(root, target) || !fs.existsSync(target) || !inside(fs.realpathSync(root), fs.realpathSync(target))) throw new Error(what + ' must exist inside the project: ' + value);
    if (check && !check(target)) throw new Error(what + ' must be a Markdown file: ' + value);
    return slash(path.relative(root, target)) + (fs.statSync(target).isDirectory() ? '/' : '');
  };
  const docs = local(raw.docs, 'docs', target => /\.md$/i.test(target));
  if (raw.lenses !== undefined && (!raw.lenses || typeof raw.lenses !== 'object' || Array.isArray(raw.lenses))) throw new Error('lenses must be a map of names to lenses.');
  const lenses = Object.entries(raw.lenses || {}).map(([name, lens]) => {
    if (!key.test(name)) throw new Error('lens “' + name + '” must be kebab-case.');
    if (!lens || typeof lens !== 'object' || typeof lens.adapter !== 'string' || !key.test(lens.adapter)) throw new Error('lens “' + name + '” needs a kebab-case adapter.');
    if (lens.styles !== undefined && !Array.isArray(lens.styles)) throw new Error('lens “' + name + '”: styles must be a list.');
    return { key: name, label: typeof lens.label === 'string' && lens.label.trim() ? lens.label.trim() : name.charAt(0).toUpperCase() + name.slice(1).replace(/-/g, ' '),
      adapter: lens.adapter, examples: local(lens.examples, 'lens “' + name + '” examples'),
      styles: (lens.styles || []).map(style => local(style, 'lens “' + name + '” style')),
      ...(lens.environment ? { environment: local(lens.environment, 'lens “' + name + '” environment') } : {}) };
  });
  if (raw.lens !== undefined && !lenses.some(lens => lens.key === raw.lens)) throw new Error('lens “' + raw.lens + '” isn’t one of its lenses.');
  if (raw.scenarios !== undefined && (!raw.scenarios || typeof raw.scenarios !== 'object' || Array.isArray(raw.scenarios))) throw new Error('scenarios must be a map.');
  const scenarios = Object.entries(raw.scenarios || {}).map(([scenario, spec]) => {
    if (!NESTED_ID.test(scenario)) throw new Error('scenarios need kebab-case IDs.');
    return { id: scenario, label: spec && typeof spec.label === 'string' && spec.label ? spec.label : titled(scenario) };
  });
  if (raw.icon !== undefined && (typeof raw.icon !== 'string' || !key.test(raw.icon))) throw new Error('icon must be a kebab-case Lucide icon name.');
  return { id: raw.id, title: typeof raw.title === 'string' && raw.title ? raw.title : raw.id, ...(raw.icon ? { icon: raw.icon } : {}), file: slash(path.relative(root, file)),
    src: docs, lens: raw.lens || (lenses[0] ? lenses[0].key : null), lenses, scenarios };
}

module.exports = { Compiler, discover, validate, inside, mockFetches, iterationsOf, projectDefinition };
