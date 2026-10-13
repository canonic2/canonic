// Capability probe run by the Bun binary: which parts of the esbuild plugin
// and build API Workbench relies on does Bun.build provide? Prints one JSON line.
// bun bun-capabilities.mjs <empty-directory>
import fs from 'node:fs';
import path from 'node:path';

const dir = process.argv[2];
fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(path.join(dir, 'dep.js'), 'export const value = 1;\n');
fs.writeFileSync(path.join(dir, 'entry.js'), "import { value } from 'virtual:dep';\nimport 'lodash-free';\nexport default value;\n");
const result = {};

async function probe(name, run) {
  try { result[name] = await run(); }
  catch (error) { result[name] = { ok: false, error: String(error?.message || error).slice(0, 300) }; }
}

// Workbench's project plugin resolves aliased and deduplicated packages with
// build.resolve(), from a chosen directory.
await probe('buildResolve', async () => {
  let seen;
  await Bun.build({ entrypoints: [path.join(dir, 'entry.js')], outdir: path.join(dir, 'out-resolve'), throw: false, plugins: [{
    name: 'probe', setup(build) {
      seen = typeof build.resolve;
      build.onResolve({ filter: /^virtual:dep$/ }, () => ({ path: path.join(dir, 'dep.js') }));
      build.onResolve({ filter: /^lodash-free$/ }, () => ({ path: 'lodash-free', namespace: 'probe' }));
      build.onLoad({ filter: /.*/, namespace: 'probe' }, () => ({ contents: '', loader: 'js' }));
    },
  }] });
  return { ok: seen === 'function', type: seen };
});

// onResolve/onLoad with namespaces, as Workbench's API, Astro and Vue style
// plugins use.
await probe('namespacesAndOnLoad', async () => {
  const out = await Bun.build({ entrypoints: [path.join(dir, 'entry.js')], outdir: path.join(dir, 'out-ns'), throw: false, plugins: [{
    name: 'probe', setup(build) {
      build.onResolve({ filter: /^virtual:dep$/ }, () => ({ path: 'dep', namespace: 'virtual' }));
      build.onLoad({ filter: /.*/, namespace: 'virtual' }, () => ({ contents: 'export const value = 2;', loader: 'js' }));
      build.onResolve({ filter: /^lodash-free$/ }, () => ({ path: 'x', namespace: 'virtual' }));
    },
  }] });
  return { ok: out.success, logs: out.logs.map(log => String(log.message)).slice(0, 3) };
});

// Definitions are bundled for Node as CommonJS with packages external.
await probe('nodeCommonJsPackagesExternal', async () => {
  fs.writeFileSync(path.join(dir, 'definition.ts'), "import react from 'react';\nexport default { id: 'probe', react: typeof react };\n");
  const out = await Bun.build({ entrypoints: [path.join(dir, 'definition.ts')], outdir: path.join(dir, 'out-cjs'), target: 'node', format: 'cjs', packages: 'external', throw: false });
  const text = out.success ? await out.outputs[0].text() : '';
  return { ok: out.success && /require\(["']react["']\)/.test(text), logs: out.logs.map(log => String(log.message)).slice(0, 3) };
});

// build.resolve from a chosen directory, as Workbench's dedupe and alias
// logic calls it: resolve 'dep-pkg' from a directory that has it.
await probe('buildResolveFromDirectory', async () => {
  const other = path.join(dir, 'other');
  fs.mkdirSync(path.join(other, 'node_modules', 'dep-pkg'), { recursive: true });
  fs.writeFileSync(path.join(other, 'node_modules', 'dep-pkg', 'package.json'), JSON.stringify({ name: 'dep-pkg', main: 'index.js' }));
  fs.writeFileSync(path.join(other, 'node_modules', 'dep-pkg', 'index.js'), 'export const fromOther = true;\n');
  fs.writeFileSync(path.join(dir, 'uses-dep.js'), "import { fromOther } from 'dep-pkg';\nexport default fromOther;\n");
  let resolved;
  const out = await Bun.build({ entrypoints: [path.join(dir, 'uses-dep.js')], outdir: path.join(dir, 'out-resolve-dir'), throw: false, plugins: [{
    name: 'probe', setup(build) {
      build.onResolve({ filter: /^dep-pkg$/ }, async args => {
        if (args.pluginData?.again) return undefined;
        const result = await build.resolve(args.path, { resolveDir: other, kind: args.kind, pluginData: { again: true } });
        resolved = result?.path;
        return result;
      });
    },
  }] });
  return { ok: out.success && !!resolved && resolved.includes(path.join('other', 'node_modules', 'dep-pkg')), resolved: resolved ?? null, logs: out.logs.map(log => String(log.message)).slice(0, 3) };
});

// Workbench tracks freshness from the build's metafile inputs.
await probe('metafileInputs', async () => {
  const out = await Bun.build({ entrypoints: [path.join(dir, 'dep.js')], outdir: path.join(dir, 'out-meta'), metafile: true, throw: false });
  const inputs = out.metafile ? Object.keys(out.metafile.inputs || {}) : [];
  return { ok: out.success && inputs.some(input => input.endsWith('dep.js')), inputs: inputs.slice(0, 3) };
});

// Workbench generates each preview's entry in memory (esbuild's stdin).
await probe('stdinEntry', async () => {
  try {
    const out = await Bun.build({ stdin: { contents: 'export default 1;', loader: 'js', resolveDir: dir }, throw: false });
    return { ok: !!out?.success, note: 'stdin option accepted' };
  } catch (error) {
    return { ok: false, note: 'no stdin option; a virtual entry through onResolve/onLoad would be needed', error: String(error?.message || error).slice(0, 200) };
  }
});

// Builds in memory, without writing to disk.
await probe('inMemoryOutput', async () => {
  const out = await Bun.build({ entrypoints: [path.join(dir, 'dep.js')], throw: false });
  return { ok: out.success && out.outputs.length > 0 && !fs.existsSync(path.join(dir, 'dep.js.out')) };
});

process.stdout.write(JSON.stringify({ bun: Bun.version, ...result }) + '\n');
process.exit(0);
