/* Lab variant 1: one code-split build per project and adapter. Every
   discovered preview that the shared build can carry becomes an entry; code
   they share lands in content-hashed chunks the browser can cache across
   previews. A preview whose entry fails is built alone by the shipping path,
   so its error still shows and the others still share. */
const fs = require('node:fs');
const crypto = require('node:crypto');
const esbuild = require('./engine.cjs');
const { discover } = require('./compiler.cjs');

const digest = body => crypto.createHash('sha256').update(body).digest('hex').slice(0, 16);
const stamp = file => { try { const s = fs.statSync(file); return s.mtimeMs + ':' + s.size; } catch (_) { return 'missing'; } };
const EXTERNAL = ['/_workbench/preview-runtime.js', '/_workbench/preview-requests.js'];

class ProjectBuild {
  constructor(compiler) {
    this.compiler = compiler;
    this.groups = new Map();
    this.entries = new Map();
  }

  async prepared(file) {
    const cached = this.entries.get(file);
    if (cached && cached.fresh()) return cached.value;
    let value = null;
    try { value = await this.compiler.prepareEntry(file); } catch (_) { /* docs definitions and invalid ones stay on the shipping path */ }
    const watched = new Map(value ? Array.from(value.dependencies, dependency => [dependency, stamp(dependency)]) : [[file, stamp(file)]]);
    this.entries.set(file, { value, fresh: () => Array.from(watched).every(([dependency, mark]) => stamp(dependency) === mark) });
    return value;
  }

  // The group a preview belongs to, built and fresh, or null when the
  // preview stays on the shipping path.
  async forFile(file) {
    const entry = await this.prepared(require('node:path').resolve(this.compiler.root, file));
    if (!entry || !entry.projectBuildable) return null;
    const group = await this.build(entry.adapter);
    return group.excluded.has(entry.slug) ? null : { group, entry };
  }

  async build(adapter) {
    const files = discover(this.compiler.root, this.compiler.options.include);
    const current = this.groups.get(adapter);
    const fresh = data => data && data.files === files.join('\n') && Array.from(data.watched).every(([file, mark]) => stamp(file) === mark);
    if (fresh(current)) return current;
    // Like the shipping compiler, keep built groups on disk across worker
    // restarts, scoped to the evaluated config.
    let diskKey = null;
    try { diskKey = 'project:' + adapter + ':' + digest(files.join('\n') + JSON.stringify(this.compiler.config, (_name, value) => typeof value === 'function' ? value.toString() : value)); }
    catch (_) { /* configs with cyclic plugin state use the memory cache only */ }
    if (!current && diskKey) {
      const stored = this.compiler.diskCache.read(diskKey);
      if (stored && stored.outputs instanceof Map && fresh(stored)) return this.remember(adapter, stored);
    }
    const entries = (await Promise.all(files.map(file => this.prepared(file))))
      .filter(entry => entry && entry.projectBuildable && entry.adapter === adapter);
    // Entries that failed on their own stay out until a file they read
    // changes, so an edit elsewhere costs one shared build, not a search.
    const excluded = new Set();
    for (const [slug, failure] of (current?.failures || new Map())) {
      if (Array.from(failure.watched).every(([file, mark]) => stamp(file) === mark)) excluded.add(slug);
    }
    const failures = new Map();
    let result;
    for (let attempt = 0; attempt < 3; attempt++) {
      const included = entries.filter(entry => !excluded.has(entry.slug));
      try { result = await this.bundle(included); break; }
      catch (error) {
        if (attempt === 2) throw error;
        // First leave out the entries that import a failing file directly;
        // if that is not enough, find the entries that fail on their own.
        const failing = (error.errors || []).map(item => item.location && require('node:path').resolve(this.compiler.root, item.location.file)).filter(Boolean);
        const direct = attempt === 0 ? included.filter(entry => failing.some(file => entry.entry.includes(JSON.stringify(file)) || require('node:path').dirname(file) === entry.resolveDir)) : [];
        if (direct.length && direct.length < included.length) { for (const entry of direct) excluded.add(entry.slug); continue; }
        const checks = await Promise.all(included.map(entry => this.bundle([entry], false).then(() => null, () => entry.slug)));
        for (const slug of checks) if (slug) excluded.add(slug);
        if (!checks.some(Boolean)) throw error;
      }
    }
    for (const slug of excluded) {
      const entry = entries.find(item => item.slug === slug);
      // A failed entry has no metafile; watch its definition's inputs and
      // the files under its directory, where its sources live.
      const watched = new Map();
      if (entry) {
        for (const dependency of entry.dependencies) watched.set(dependency, stamp(dependency));
        const directory = require('node:path').dirname(require('node:path').resolve(this.compiler.root, entry.file));
        for (const name of fs.readdirSync(directory)) watched.set(require('node:path').join(directory, name), stamp(require('node:path').join(directory, name)));
      }
      failures.set(slug, { watched });
    }
    const outputs = new Map();
    for (const output of result.outputFiles) outputs.set(require('node:path').relative(entries[0].outdir, output.path).split(require('node:path').sep).join('/'), Buffer.from(output.contents));
    const watched = new Map();
    for (const input of Object.keys(result.metafile.inputs)) {
      if (input.includes(':')) continue;
      const file = require('node:path').resolve(this.compiler.root, input);
      if (fs.existsSync(file)) watched.set(file, stamp(file));
    }
    for (const entry of entries) for (const dependency of entry.dependencies) watched.set(dependency, stamp(dependency));
    // Chunks every included entry imports statically: what a spare frame
    // can load ahead of a selection.
    const shared = new Map();
    for (const [name, output] of Object.entries(result.metafile.outputs)) {
      if (!output.entryPoint) continue;
      for (const item of output.imports || []) if (item.kind === 'import-statement' && !item.external) {
        const chunk = require('node:path').relative(entries[0].outdir, require('node:path').resolve(this.compiler.root, item.path)).split(require('node:path').sep).join('/');
        shared.set(chunk, (shared.get(chunk) || 0) + 1);
      }
    }
    const data = {
      files: files.join('\n'), outputs, watched, excluded: Array.from(excluded), failures,
      sharedChunks: Array.from(shared).filter(([, count]) => count > 1).map(([chunk]) => chunk),
    };
    const missing = entries.filter(entry => !excluded.has(entry.slug) && !outputs.has(entry.slug + '.js'));
    if (missing.length) throw new Error('Project build produced no entry for ' + missing.map(entry => entry.slug).join(', '));
    if (diskKey) this.compiler.diskCache.write(diskKey, data);
    return this.remember(adapter, data);
  }

  remember(adapter, data) {
    const group = { adapter, ...data, excluded: new Set(data.excluded),
      revision: slug => digest((data.outputs.get(slug + '.js') || '') + '' + (data.outputs.get(slug + '.css') || '')) };
    this.groups.set(adapter, group);
    return group;
  }

  bundle(entries, split = true) {
    const options = entries[0].options;
    const byName = new Map(entries.map(entry => [entry.slug, entry]));
    const virtual = { name: 'workbench-project-entries', setup(build) {
      build.onResolve({ filter: /^workbench-entry:/ }, args => ({ path: args.path.slice('workbench-entry:'.length), namespace: 'workbench-entry' }));
      build.onLoad({ filter: /.*/, namespace: 'workbench-entry' }, args => {
        const entry = byName.get(args.path);
        return { contents: entry.entry, loader: 'js', resolveDir: entry.resolveDir };
      });
    } };
    return esbuild.build(Object.assign({}, options, {
      // Named entries: output names are the preview slugs, not derived paths.
      entryPoints: Object.fromEntries(entries.map(entry => [entry.slug, 'workbench-entry:' + entry.slug])), plugins: [virtual, ...options.plugins],
      outdir: entries[0].outdir, entryNames: '[name]', chunkNames: 'chunks/[name]-[hash]',
      splitting: split, external: EXTERNAL, sourcemap: false, write: false, metafile: true,
    }));
  }
}

module.exports = { ProjectBuild };
