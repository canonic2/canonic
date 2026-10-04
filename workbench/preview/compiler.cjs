/* One compiler for live previews and portable exports. WASM keeps the installed
   extension independent of the host CPU and avoids downloading native tools. */
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const Module = require('node:module');
const esbuild = require('esbuild-wasm');
const picomatch = require('picomatch');
const API = path.join(__dirname, 'api.js');
const OMIT = new Set(['node_modules', '.git', '.canonic', '.claude', '.codex', 'dist', 'build', 'coverage', '.next', '.astro']);
const EXTENSIONS = ['.tsx', '.ts', '.jsx', '.js', '.mjs', '.cjs', '.json', '.vue', '.html'];
const digest = body => crypto.createHash('sha256').update(body).digest('hex').slice(0, 16);
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

function discover(root, include) {
  const matches = picomatch(include || ['**/*.workbench.ts', '**/*.workbench.tsx']);
  const files = [];
  function walk(dir) {
    for (const item of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (item.name.startsWith('.') || OMIT.has(item.name) || item.isSymbolicLink()) continue;
      const file = path.join(dir, item.name);
      if (item.isDirectory()) walk(file);
      else if (matches(slash(path.relative(root, file)))) files.push(file);
    }
  }
  walk(root);
  return files;
}

function validate(raw, file, root) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error(file + ': default export must be definePreview({...}).');
  if (typeof raw.id !== 'string' || !/^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/.test(raw.id)) throw new Error(file + ': id must use kebab-case segments separated by /.');
  if (typeof raw.adapter !== 'string' || !/^[a-z0-9-]+$/.test(raw.adapter)) throw new Error(file + ': adapter must be a registered kebab-case name.');
  const sources = [raw.source, ...Object.values(raw.states || {}).map(state => state.source).filter(Boolean)];
  for (const source of sources) {
    if (!source || typeof source.entry !== 'string' || !source.entry.trim()) throw new Error(file + ': source.entry must name a local source file.');
    const target = path.resolve(path.dirname(file), source.entry);
    if (!inside(root, target) || !fs.existsSync(target) || !inside(fs.realpathSync(root), fs.realpathSync(target))) throw new Error(file + ': source must exist inside the project: ' + source.entry);
  }
  if (raw.states !== undefined && (!raw.states || typeof raw.states !== 'object' || Array.isArray(raw.states))) throw new Error(file + ': states must be a map.');
  const states = raw.states && Object.keys(raw.states).length ? raw.states : { default: {} };
  for (const [id, state] of Object.entries(states)) {
    if (!/^[a-z0-9-]+$/.test(id) || !state || typeof state !== 'object' || Array.isArray(state)) throw new Error(file + ': states need kebab-case IDs and object definitions.');
    for (const key of ['inputs', 'fixtures', 'globals']) if (state[key] !== undefined) JSON.stringify(state[key]);
  }
  if (raw.viewports && (!Array.isArray(raw.viewports) || !raw.viewports.length || raw.viewports.some(v => !['fit', 'desktop', 'mobile', 'responsive'].includes(v)))) throw new Error(file + ': invalid viewports.');
  for (const [name, control] of Object.entries(raw.controls || {})) {
    if (!control || !['text', 'number', 'boolean', 'select', 'json'].includes(control.type)) throw new Error(file + ': invalid control ' + name);
    if (control.type === 'select' && (!Array.isArray(control.options) || !control.options.length)) throw new Error(file + ': select controls need options.');
  }
  for (const [where, requests] of [['', raw.requests], ...Object.entries(states).map(([id, state]) => ['state ' + id + ' ', state.requests])]) {
    if (requests === undefined) continue;
    if (!requests || typeof requests !== 'object' || Array.isArray(requests)) throw new Error(file + ': ' + where + 'requests must be a map.');
    for (const [key, value] of Object.entries(requests)) {
      if (!/^(?:[A-Z]+\s+)?(?:\/\S*|[a-z][a-z0-9+.-]*:\/\/\S+)(?:\s+[A-Za-z_]\w*)?$/.test(key.trim())) throw new Error(file + ': ' + where + 'request ' + key + ' must be "[METHOD] /path [Operation]" or a full URL.');
      if (typeof value !== 'function' && (!value || typeof value !== 'object' || Array.isArray(value))) throw new Error(file + ': ' + where + 'request ' + key + ' must be a response object or a function.');
    }
  }
  if (raw.links !== undefined && (!raw.links || typeof raw.links !== 'object' || Array.isArray(raw.links))) throw new Error(file + ': links must be a map.');
  for (const [href, to] of Object.entries(raw.links || {})) {
    const target = typeof to === 'string' ? { preview: to } : to;
    const valid = target && typeof target === 'object' && !Array.isArray(target) && (target.preview || target.state)
      && (target.preview === undefined || (typeof target.preview === 'string' && /^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/.test(target.preview)))
      && (target.state === undefined || (typeof target.state === 'string' && /^[a-z0-9-]+$/.test(target.state)));
    if (!valid) throw new Error(file + ': link ' + href + ' must name a preview ID, or { preview, state }.');
  }
  return Object.assign({}, raw, { states });
}

// Astro frontmatter runs here, in Node, so its fetch calls get the same
// request mocks the browser does. The worker renders one preview at a time,
// which is what makes swapping globalThis.fetch for the render safe.
async function mockFetches(definition, state, inputs, render) {
  const spec = definition.states[state];
  if (!spec.requests && !definition.requests) return render();
  const { mockedFetch, requestMocks } = await import(require('node:url').pathToFileURL(path.join(__dirname, 'requests.js')).href);
  const original = globalThis.fetch;
  const context = { id: definition.id, state, inputs, fixtures: { ...definition.fixtures, ...spec.fixtures }, globals: { ...definition.globals, ...spec.globals } };
  // A server render awaits every fetch, so `pending` would hang it; give up
  // after ten seconds with a rendering error instead.
  const active = { mocks: requestMocks([spec.requests, definition.requests]), context, base: 'http://localhost/', signal: AbortSignal.timeout(10000) };
  globalThis.fetch = mockedFetch(original, () => active);
  try { return await render(); } finally { globalThis.fetch = original; }
}

class Compiler {
  constructor(root, options = {}) {
    this.root = path.resolve(root);
    this.options = options;
    this.cache = new Map();
    this.config = {};
    this.configFiles = new Map();
    this.definitions = new Map();
  }
  tracked(meta) {
    return Object.keys(meta.inputs).filter(file => !file.startsWith('workbench:')).map(file => path.resolve(this.root, file)).filter(file => fs.existsSync(file));
  }
  fresh(files) { return Array.from(files).every(([file, value]) => stamp(file) === value); }
  apiPlugin() {
    return { name: 'workbench-api', setup(build) {
      build.onResolve({ filter: /^@canonic\/workbench$/ }, () => ({ path: API }));
    } };
  }
  async evaluate(file) {
    const result = await esbuild.build({ absWorkingDir: this.root, entryPoints: [file], bundle: true, platform: 'node', format: 'cjs', packages: 'external', write: false, metafile: true, logLevel: 'silent', plugins: [this.apiPlugin()] });
    const loaded = new Module(file);
    loaded.filename = file;
    loaded.paths = Module._nodeModulePaths(path.dirname(file));
    loaded._compile(result.outputFiles[0].text, file);
    return { value: loaded.exports.default || loaded.exports, files: this.tracked(result.metafile) };
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
      this.cache.clear();
    }
    return this.config;
  }
  async index() {
    await this.settings();
    const previews = [];
    const errors = [];
    const ids = new Set();
    const files = discover(this.root, this.options.include);
    for (const file of files) {
      try {
        const evaluated = await this.definition(file);
        const definition = validate(evaluated.value, file, this.root);
        if (ids.has(definition.id)) throw new Error('Duplicate preview id: ' + definition.id);
        ids.add(definition.id);
        const title = definition.title || definition.id;
        previews.push({ id: definition.id, title, adapter: definition.adapter, file: slash(path.relative(this.root, file)),
          source: slash(path.relative(this.root, path.resolve(path.dirname(file), definition.source.entry))),
          states: Object.entries(definition.states).map(([id, state]) => ({ id, label: state.label || id.replace(/(^|-)(\w)/g, (_, gap, char) => (gap ? ' ' : '') + char.toUpperCase()) })),
          controls: definition.controls || {}, viewports: definition.viewports, docs: definition.docs || null,
          links: Object.fromEntries(Object.entries(definition.links || {}).map(([href, to]) => [href,
            typeof to === 'string' ? { preview: to } : { preview: to.preview || definition.id, ...(to.state ? { state: to.state } : {}) }])),
        });
      } catch (error) { errors.push(slash(path.relative(this.root, file)) + ': ' + error.message); }
    }
    return { previews, errors };
  }
  async compile(file, development = true, renderRequest) {
    await this.settings();
    file = path.resolve(this.root, file);
    if (!inside(this.root, file) || !fs.existsSync(file) || !/\.workbench\.tsx?$/.test(file) || !inside(fs.realpathSync(this.root), fs.realpathSync(file))) throw new Error('Not a declared Workbench preview definition.');
    const key = file + ':' + development + (renderRequest ? ':' + JSON.stringify(renderRequest) : '');
    const cached = this.cache.get(key);
    if (cached && this.fresh(cached.watched)) return cached;
    if (!discover(this.root, this.options.include).includes(file)) throw new Error('Not a declared Workbench preview definition.');
    const evaluated = await this.definition(file);
    const definition = validate(evaluated.value, file, this.root);
    const config = this.config;
    const builtins = { html: 'html.js', react: 'react.js', vue: 'vue.js', 'react-native-web': 'react.js', astro: 'astro.js' };
    const addon = config.adapters?.[definition.adapter];
    const runtime = addon?.runtime ? path.resolve(this.root, addon.runtime) : builtins[definition.adapter] && path.join(__dirname, builtins[definition.adapter]);
    if (!runtime) throw new Error('Unknown adapter “' + definition.adapter + '”. Register it in workbench.config.ts.');
    const outputs = new Map();
    const dependencies = new Set([...evaluated.files, ...this.configFiles.keys()]);
    const extraSources = new Set();
    const outdir = path.join(this.root, '__workbench_output__');
    const aliases = Object.assign({}, definition.adapter === 'react-native-web' ? { 'react-native': 'react-native-web' } : {}, config.aliases);
    const plugins = [this.apiPlugin(), ...(config.plugins || []), ...(addon?.plugins || [])];
    const compiler = this;
    const htmlAssets = new Map();
    const htmlStyles = new Map();
    const astroSources = new Map();
    const astroResources = new Map();
    const astroPackages = [];
    const astroPublicDirs = new Set();
    const rendered = {};
    if (definition.adapter === 'astro' && !addon) {
      if (renderRequest && !Object.prototype.hasOwnProperty.call(definition.states, renderRequest.state)) throw new Error('Unknown preview state: ' + renderRequest.state);
      for (const [state, spec] of Object.entries(definition.states)) {
        if (renderRequest && state !== renderRequest.state) continue;
        const source = spec.source || definition.source;
        const target = path.resolve(path.dirname(file), source.entry);
        let renderer = astroSources.get(target);
        if (!renderer) {
          renderer = await require('./astro.cjs').create(this.root, target, { ...config,
            define: { 'import.meta.env.DEV': JSON.stringify(development), 'import.meta.env.PROD': JSON.stringify(!development), ...config.define } });
          astroSources.set(target, renderer);
          for (const dependency of renderer.dependencies) dependencies.add(dependency);
          for (const [id, resource] of renderer.resources) astroResources.set(id, resource);
          astroPackages.push(renderer.package);
          astroPublicDirs.add(renderer.publicDir);
        }
        const inputs = { ...definition.inputs, ...spec.inputs,
          ...(renderRequest?.state === state ? renderRequest.inputs : {}) };
        const html = await mockFetches(definition, state, inputs, () => renderer.render(inputs, source.export));
        rendered[state] = { html, inputs, target };
      }
      plugins.push({ name: 'workbench-astro-assets', setup(build) {
        build.onResolve({ filter: /^workbench:astro-resource:/ }, args => ({ path: args.path, namespace: 'astro-resource' }));
        build.onLoad({ filter: /.*/, namespace: 'astro-resource' }, args => astroResources.get(args.path));
      } });
    }
    const copyAsset = async (from, reference) => {
      if (!astroResources.has(reference) && /^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(reference)) return reference;
      const [clean, suffix = ''] = reference.split(/(?=[?#])/);
      const resource = astroResources.get(reference);
      let target = resource ? reference : clean.startsWith('/') ? path.join(this.root, clean.slice(1)) : path.resolve(path.dirname(from), clean);
      if (!resource && definition.adapter === 'astro' && clean.startsWith('/') && !fs.existsSync(target)) {
        for (const directory of astroPublicDirs) {
          const candidate = path.join(directory, clean.slice(1));
          if (fs.existsSync(candidate)) { target = candidate; break; }
        }
      }
      if (!resource && (!inside(this.root, target) || !fs.existsSync(target))) throw new Error('Missing HTML asset: ' + reference + ' in ' + from);
      if (!resource) dependencies.add(target);
      if (htmlAssets.has(target)) return htmlAssets.get(target) + suffix;
      const extension = path.extname(target);
      const name = 'assets/' + digest(target + (resource ? resource.contents : fs.readFileSync(target))) + extension;
      htmlAssets.set(target, './' + name);
      if (/\.(?:[cm]?[jt]sx?|css)$/.test(target)) {
        const entry = resource ? { stdin: resource } : { entryPoints: [target] };
        const result = await esbuild.build(Object.assign({}, buildOptions(), entry, { outdir, entryNames: name.replace(/\.[^.]+$/, ''), format: extension === '.css' ? 'esm' : 'iife' }));
        for (const output of result.outputFiles) outputs.set(slash(path.relative(outdir, output.path)), Buffer.from(output.contents));
        for (const dependency of compiler.tracked(result.metafile)) dependencies.add(dependency);
        const output = result.outputFiles.find(output => output.path.endsWith(extension === '.css' ? '.css' : '.js'));
        const url = './' + slash(path.relative(outdir, output.path));
        const styles = result.outputFiles.filter(output => output.path.endsWith('.css'))
          .map(output => './' + slash(path.relative(outdir, output.path)));
        if (extension !== '.css' && styles.length) htmlStyles.set(target, styles);
        htmlAssets.set(target, url);
        return url + suffix;
      }
      outputs.set(name, fs.readFileSync(target));
      return './' + name + suffix;
    };
    plugins.push({ name: 'workbench-project', setup(build) {
      build.onResolve({ filter: /^[^./]/ }, args => {
        if (args.pluginData?.workbenchResolved) return;
        if (args.path.startsWith('workbench:')) return;
        const exact = Object.prototype.hasOwnProperty.call(aliases, args.path) && aliases[args.path];
        if (exact) {
          const target = exact.startsWith('.') || path.isAbsolute(exact) ? path.resolve(compiler.root, exact) : exact;
          return build.resolve(target, { resolveDir: path.dirname(file), kind: args.kind, pluginData: { workbenchResolved: true } });
        }
        const packageName = args.path.startsWith('@') ? args.path.split('/').slice(0, 2).join('/') : args.path.split('/')[0];
        if ((config.dedupe || []).includes(packageName) || args.importer.startsWith(__dirname + path.sep)) {
          return build.resolve(args.path, { resolveDir: path.dirname(file), kind: args.kind, pluginData: { workbenchResolved: true } });
        }
      });
      build.onLoad({ filter: /\.vue$/ }, async args => {
        const compilerSfc = require('@vue/compiler-sfc');
        const { descriptor, errors } = compilerSfc.parse(fs.readFileSync(args.path, 'utf8'), { filename: args.path });
        if (errors.length) throw new Error(errors.join('\n'));
        const id = digest(args.path);
        let content = '';
        if (descriptor.script || descriptor.scriptSetup) {
          const script = compilerSfc.compileScript(descriptor, { id, genDefaultAs: '__component', inlineTemplate: true, templateOptions: { scoped: descriptor.styles.some(style => style.scoped) } });
          content = script.content;
          if (!descriptor.scriptSetup && descriptor.template) {
            const template = compilerSfc.compileTemplate({ source: descriptor.template.content, filename: args.path, id, scoped: descriptor.styles.some(style => style.scoped), compilerOptions: { bindingMetadata: script.bindings } });
            if (template.errors.length) throw new Error(template.errors.join('\n'));
            content += '\n' + template.code + '\n__component.render = render;';
          }
        } else {
          const template = compilerSfc.compileTemplate({ source: descriptor.template?.content || '', filename: args.path, id, scoped: descriptor.styles.some(style => style.scoped) });
          if (template.errors.length) throw new Error(template.errors.join('\n'));
          content = template.code + '\nconst __component = { render };';
        }
        for (let index = 0; index < descriptor.styles.length; index++) {
          const style = descriptor.styles[index];
          if (style.src || style.lang && style.lang !== 'css') throw new Error('Vue style preprocessors need a compiler plugin: ' + args.path);
          const compiled = compilerSfc.compileStyle({ source: style.content, filename: args.path, id: 'data-v-' + id, scoped: style.scoped });
          if (compiled.errors.length) throw new Error(compiled.errors.join('\n'));
          content += '\nimport ' + JSON.stringify('workbench:style:' + args.path + ':' + index) + ';';
          htmlAssets.set('workbench:style:' + args.path + ':' + index, { contents: compiled.code, resolveDir: path.dirname(args.path) });
        }
        if (descriptor.styles.some(style => style.scoped)) content += '\n__component.__scopeId = ' + JSON.stringify('data-v-' + id) + ';';
        return { contents: content + '\nexport default __component;', loader: 'ts', resolveDir: path.dirname(args.path) };
      });
      build.onResolve({ filter: /^workbench:style:/ }, args => ({ path: args.path, namespace: 'workbench-style' }));
      build.onLoad({ filter: /.*/, namespace: 'workbench-style' }, args => ({ ...htmlAssets.get(args.path), loader: 'css' }));
      build.onResolve({ filter: /^workbench:astro-page:/ }, args => ({ path: args.path, namespace: 'astro-page' }));
      const loadHtml = async (args, initialHtml) => {
        let html = initialHtml;
        // Copy referenced browser assets; scripts/styles are compiled so their
        // own imports and URLs travel too. Inline modules/styles enter the graph.
        const linkedStyles = new Set();
        const tags = Array.from(html.matchAll(/<([a-z][a-z0-9-]*)\b[^>]*>/gi));
        for (const tag of tags) {
          let rewritten = tag[0];
          for (const match of tag[0].matchAll(/\b(src|href|poster)\s*=\s*(["'])([^"']+)\2/gi)) {
            // Anchor links and base URLs describe navigation, not build assets.
            if (match[1].toLowerCase() === 'href' && tag[1].toLowerCase() !== 'link') continue;
            const url = await copyAsset(args.path, match[3]);
            rewritten = rewritten.replace(match[0], match[1] + '=' + match[2] + url + match[2]);
            const target = astroResources.has(match[3]) ? match[3] : match[3].startsWith('/') ? path.join(compiler.root, match[3].slice(1).split(/[?#]/)[0]) : path.resolve(path.dirname(args.path), match[3].split(/[?#]/)[0]);
            for (const style of htmlStyles.get(target) || []) linkedStyles.add(style);
          }
          html = html.replace(tag[0], rewritten);
        }
        const srcsets = Array.from(html.matchAll(/\bsrcset\s*=\s*(["'])([^"']+)\1/gi));
        for (const match of srcsets) {
          const values = [];
          for (const item of match[2].split(',')) { const [url, ...size] = item.trim().split(/\s+/); values.push(await copyAsset(args.path, url) + (size.length ? ' ' + size.join(' ') : '')); }
          html = html.replace(match[0], 'srcset=' + match[1] + values.join(', ') + match[1]);
        }
        for (const match of Array.from(html.matchAll(/<style(?:\s[^>]*)?>([\s\S]*?)<\/style>/gi))) {
          const result = await esbuild.build(Object.assign({}, buildOptions(), { stdin: { contents: match[1], loader: 'css', resolveDir: path.dirname(args.path) }, outdir, entryNames: 'assets/' + digest(args.path + match[1]) }));
          for (const output of result.outputFiles) outputs.set(slash(path.relative(outdir, output.path)), Buffer.from(output.contents));
          for (const dependency of compiler.tracked(result.metafile)) dependencies.add(dependency);
          const css = result.outputFiles.find(output => output.path.endsWith('.css'));
          html = html.replace(match[0], '<link rel="stylesheet" href="./' + slash(path.relative(outdir, css.path)) + '">');
        }
        for (const match of Array.from(html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi))) {
          if (/\bsrc\s*=/.test(match[1]) || /\btype\s*=\s*["'](?:application\/ld\+json|application\/json)/i.test(match[1])) continue;
          const isModule = /\btype\s*=\s*["']module["']/i.test(match[1]);
          const result = await esbuild.build(Object.assign({}, buildOptions(), { stdin: { contents: match[2], loader: 'js', resolveDir: path.dirname(args.path) }, format: isModule ? 'esm' : 'iife', outdir, entryNames: 'assets/' + digest(args.path + match[2]) }));
          for (const output of result.outputFiles) outputs.set(slash(path.relative(outdir, output.path)), Buffer.from(output.contents));
          for (const dependency of compiler.tracked(result.metafile)) dependencies.add(dependency);
          const js = result.outputFiles.find(output => output.path.endsWith('.js'));
          for (const output of result.outputFiles.filter(output => output.path.endsWith('.css'))) linkedStyles.add('./' + slash(path.relative(outdir, output.path)));
          html = html.replace(match[0], '<script' + match[1] + ' src="./' + slash(path.relative(outdir, js.path)) + '"></script>');
        }
        html = Array.from(linkedStyles, style => '<link rel="stylesheet" href="' + style + '">').join('') + html;
        // Keep document attributes for the HTML adapter (body classes and
        // styles are part of the source's rendering contract).
        html = html.replace(/<title>[\s\S]*?<\/title>|<meta\b[^>]*>|<base\b[^>]*>/gi, '');
        if (args.state) rendered[args.state].html = html;
        return { contents: 'export default ' + JSON.stringify(html), loader: 'js' };
      };
      build.onLoad({ filter: /\.html?$/ }, args => loadHtml(args, fs.readFileSync(args.path, 'utf8')));
      build.onLoad({ filter: /.*/, namespace: 'astro-page' }, async args => {
        const state = args.path.slice('workbench:astro-page:'.length);
        const item = rendered[state];
        const html = await loadHtml({ path: item.target, state }, item.html);
        return { contents: html.contents + '\nexport const inputs = ' + JSON.stringify(item.inputs) + ';', loader: 'js' };
      });
    } });
    function buildOptions() {
      return { absWorkingDir: compiler.root, bundle: true, write: false, metafile: true, logLevel: 'silent', platform: 'browser', format: 'esm', target: 'es2020', jsx: 'automatic',
        define: { 'process.env.NODE_ENV': JSON.stringify(development ? 'development' : 'production'), __VUE_OPTIONS_API__: 'true', __VUE_PROD_DEVTOOLS__: 'false', __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: 'false', ...config.define },
        resolveExtensions: config.resolveExtensions || (definition.adapter === 'react-native-web' ? ['.web.tsx', '.web.ts', '.web.jsx', '.web.js', ...EXTENSIONS] : EXTENSIONS),
        loader: { '.svg': 'file', '.png': 'file', '.jpg': 'file', '.jpeg': 'file', '.gif': 'file', '.webp': 'file', '.avif': 'file', '.woff': 'file', '.woff2': 'file', '.ttf': 'file', '.mp4': 'file', '.mp3': 'file' },
        assetNames: 'assets/[name]-[hash]', plugins };
    }
    // Request mocks first: they replace fetch before any project module runs.
    const lines = ['import ' + JSON.stringify(development ? '/_workbench/preview-requests.js' : path.join(__dirname, 'requests.js')) + ';',
      'import definition from ' + JSON.stringify(file) + ';', 'import * as adapter from ' + JSON.stringify(runtime) + ';', 'import { boot, combine } from ' + JSON.stringify(development ? '/_workbench/preview-runtime.js' : path.join(__dirname, 'browser.js')) + ';'];
    const sourceMap = [];
    const defaultSource = definition.source;
    const sources = { default: defaultSource };
    Object.entries(definition.states).forEach(([state, spec]) => { sources[state] = spec.source || defaultSource; });
    for (const [state, source] of Object.entries(sources)) {
      const name = 'source' + sourceMap.length;
      const target = path.resolve(path.dirname(file), source.entry);
      if (rendered[state]) {
        lines.push('import * as ' + name + ' from ' + JSON.stringify('workbench:astro-page:' + state) + ';');
        sourceMap.push(JSON.stringify(state) + ': { html: ' + name + '.default, inputs: ' + name + '.inputs, renderUrl: ' +
          JSON.stringify(development ? '/_workbench/previews/render?file=' + encodeURIComponent(slash(path.relative(this.root, file))) : null) + ' }');
      } else if (definition.adapter !== 'astro' || addon) {
        lines.push('import * as ' + name + ' from ' + JSON.stringify(target) + ';');
        sourceMap.push(JSON.stringify(state) + ': ' + name + '[' + JSON.stringify(source.export || 'default') + ']');
      }
    }
    const environments = [];
    // One environment for every preview, or one per adapter name, so a
    // project with React and Vue screens can wrap each in its own providers.
    const projectEnvironment = typeof config.environment === 'string' ? config.environment
      : config.environment && typeof config.environment === 'object' ? config.environment[definition.adapter] : undefined;
    if (projectEnvironment) {
      const target = path.resolve(this.root, projectEnvironment);
      if (!inside(this.root, target) || !fs.existsSync(target)) throw new Error('The preview config environment must be a file inside the project: ' + projectEnvironment);
      lines.push('import * as projectEnvironment from ' + JSON.stringify(target) + ';');
      environments.push('projectEnvironment');
    }
    if (definition.environment) {
      lines.push('import * as environment from ' + JSON.stringify(path.resolve(path.dirname(file), definition.environment)) + ';');
      environments.push('environment');
    }
    for (const style of definition.styles || []) lines.push('import ' + JSON.stringify(path.resolve(path.dirname(file), style)) + ';');
    const slug = definition.id.replace(/\//g, '--') + '-' + digest(definition.id + (renderRequest ? JSON.stringify(renderRequest) : ''));
    const portableNote = Object.keys(rendered).length ? 'Astro input edits require the live Workbench server. This export contains the authored states.' : '';
    const browserDefinition = portableNote && !development ? '{ ...definition, controls: {}, docs: (definition.docs || "") + "\\n" + ' + JSON.stringify(portableNote) + ' }' : 'definition';
    const entry = lines.join('\n') + '\nexport function mount(options) { return boot(' + browserDefinition + ', {' + sourceMap.join(',') + '}, adapter, ' + (environments.length === 2 ? 'combine(projectEnvironment, environment)' : environments[0] || '{}') + ', options); }\nif (!window.__workbenchHost) mount(window.__workbenchOptions);';
    const result = await esbuild.build(Object.assign({}, buildOptions(), { stdin: { contents: entry, loader: 'js', resolveDir: path.dirname(file) }, outdir, entryNames: 'preview', external: development ? ['/_workbench/preview-runtime.js', '/_workbench/preview-requests.js'] : [], sourcemap: development ? 'inline' : false }));
    for (const output of result.outputFiles) outputs.set(slash(path.relative(outdir, output.path)), Buffer.from(output.contents));
    for (const dependency of this.tracked(result.metafile)) dependencies.add(dependency);
    for (const asset of definition.assets || []) {
      const target = path.resolve(path.dirname(file), asset);
      if (!inside(this.root, target)) throw new Error('Declared assets must be inside the project.');
      const add = file => { dependencies.add(file); extraSources.add(file); outputs.set('assets/source/' + slash(path.relative(this.root, file)), fs.readFileSync(file)); };
      if (fs.statSync(target).isDirectory()) {
        const walk = dir => { for (const item of fs.readdirSync(dir, { withFileTypes: true })) { if (item.isSymbolicLink() || item.name.startsWith('.') || OMIT.has(item.name)) continue; const child = path.join(dir, item.name); if (item.isDirectory()) walk(child); else add(child); } };
        walk(target);
        dependencies.add(target);
      } else add(target);
    }
    const revision = digest(Array.from(outputs, ([name, body]) => name + Buffer.from(body).toString('base64')).join('\n'));
    const html = '<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">' +
      (development ? '<base href="/_workbench/previews/preview/' + slug + '/">' : '') +
      '<title>' + (definition.title || definition.id).replace(/[&<>"']/g, char => '&#' + char.charCodeAt(0) + ';') + '</title>' +
      (outputs.has('preview.css') ? '<link rel="stylesheet" href="./preview.css">' : '') +
      '<style>html,body{margin:0;min-height:100%}#workbench-preview{min-height:100vh}</style></head><body>' +
      '<script>window.__workbenchOptions=' + JSON.stringify({ development, revision, revisionUrl: '/_workbench/previews/revision?file=' + encodeURIComponent(slash(path.relative(this.root, file))) }).replace(/</g, '\\u003c') + '</script>' +
      '<script type="module" src="./preview.js"></script></body></html>';
    outputs.set('index.html', Buffer.from(html));
    const localFiles = Array.from(new Set(Array.from(dependencies)
      .filter(file => inside(this.root, file) && !slash(path.relative(this.root, file)).split('/').includes('node_modules') && fs.existsSync(file) && fs.statSync(file).isFile())
      .map(file => path.join(this.root, path.relative(fs.realpathSync(this.root), fs.realpathSync(file))))));
    const packages = new Map();
    for (const pkg of astroPackages) packages.set(pkg.name, pkg.version);
    for (const dependency of dependencies) {
      if (!slash(dependency).includes('/node_modules/')) continue;
      let directory = path.dirname(dependency);
      while (directory !== path.dirname(directory)) {
        const manifest = path.join(directory, 'package.json');
        if (fs.existsSync(manifest)) {
          try { const pkg = JSON.parse(fs.readFileSync(manifest, 'utf8')); if (pkg.name && pkg.version) packages.set(pkg.name, pkg.version); } catch (_) {}
          break;
        }
        directory = path.dirname(directory);
      }
    }
    const compiled = { slug, id: definition.id, title: definition.title || definition.id, file: slash(path.relative(this.root, file)), revision, outputs, localFiles, rendered, portableNote,
      packages: Array.from(packages, ([name, version]) => ({ name, version })),
      watched: new Map(Array.from(dependencies).map(file => [file, stamp(file)])) };
    // Input edits can create arbitrarily many variants. Keep only authored
    // reference builds in the compiler cache.
    if (!renderRequest) this.cache.set(key, compiled);
    return compiled;
  }
}

module.exports = { Compiler, discover, validate, inside };
