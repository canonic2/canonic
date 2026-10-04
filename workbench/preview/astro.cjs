/* Astro frontmatter stays in the preview worker. Only rendered HTML and client
   assets enter the browser bundle; frontmatter is never bundled for browsers. */
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const { pathToFileURL } = require('node:url');
const esbuild = require('./engine.cjs');
const crypto = require('node:crypto');
const hash = value => crypto.createHash('sha256').update(value).digest('hex').slice(0, 16);

async function create(root, source, config = {}) {
  const resolve = Module.createRequire(source);
  let astroPath;
  try { astroPath = fs.realpathSync(resolve.resolve('astro/package.json')); }
  catch (_) { throw new Error('The Astro adapter needs astro installed in the project.'); }
  const astro = Module.createRequire(astroPath);
  const version = JSON.parse(fs.readFileSync(astroPath, 'utf8')).version;
  const [major, minor] = version.split('.').map(Number);
  if (major < 4 || major === 4 && minor < 9) throw new Error('Astro previews require Astro 4.9 or newer (the Container API).');
  let projectRoot = path.dirname(source);
  while (projectRoot !== root && !fs.existsSync(path.join(projectRoot, 'package.json')) && projectRoot !== path.dirname(projectRoot)) projectRoot = path.dirname(projectRoot);
  let transform, parse;
  try {
    const name = Number(version.split('.')[0]) >= 7 ? '@astrojs/compiler-rs' : '@astrojs/compiler';
    ({ transform, parse } = await import(pathToFileURL(astro.resolve(name)).href));
  } catch (error) { throw new Error('Cannot load the project Astro compiler: ' + error.message); }
  const resources = new Map();
  const dependencies = new Set([astroPath]);
  const scripts = new Map();
  const styles = new Set();
  const resource = (file, index, extension, contents) => {
    const id = 'workbench:astro-resource:' + hash(file + ':' + index + extension) + extension;
    resources.set(id, { contents, resolveDir: path.dirname(file), loader: extension === '.css' ? 'css' : 'ts' });
    return id;
  };
  const plugin = { name: 'workbench-astro-server', setup(build) {
    // Keep Astro's server runtime in Node, using this project's own version.
    build.onResolve({ filter: /^astro\// }, args => ({ path: pathToFileURL(astro.resolve(args.path)).href, external: true }));
    build.onResolve({ filter: /^astro:/ }, args => { throw new Error(args.path + ' needs the Astro application pipeline; use a URL lens or a compiler plugin.'); });
    build.onResolve({ filter: /.*/ }, args => {
      const alias = !args.pluginData?.workbenchAlias && config.aliases?.[args.path];
      if (alias) {
        const target = alias.startsWith('.') || path.isAbsolute(alias) ? path.resolve(root, alias) : alias;
        return build.resolve(target, { resolveDir: args.resolveDir, kind: args.kind, pluginData: { workbenchAlias: true } });
      }
      if (/\.astro\?/.test(args.path)) return { path: args.path, namespace: 'astro-generated' };
      if (/\?(raw|url)$/.test(args.path)) return { path: path.resolve(args.resolveDir, args.path), namespace: 'astro-import' };
    });
    build.onLoad({ filter: /.*/, namespace: 'astro-import' }, args => {
      const [file, query] = args.path.split('?');
      dependencies.add(file);
      const value = query === 'raw' ? fs.readFileSync(file, 'utf8') : '/' + path.relative(root, file).split(path.sep).join('/');
      return { contents: JSON.stringify(value), loader: 'json' };
    });
    build.onLoad({ filter: /.*/, namespace: 'astro-generated' }, () => ({ contents: '', loader: 'js' }));
    build.onLoad({ filter: /\.astro$/ }, async args => {
      dependencies.add(args.path);
      const input = fs.readFileSync(args.path, 'utf8');
      const parsed = await parse(input);
      const visit = node => {
        if (!node || typeof node !== 'object') return;
        const tag = node.openingElement?.name?.name || node.name;
        if (tag === 'style') {
          const attributes = node.openingElement?.attributes || node.attributes || [];
          const language = attributes.find(attribute => (attribute.name?.name || attribute.name) === 'lang');
          const value = language?.value?.value || language?.value;
          if (value && value !== 'css') throw new Error('Astro style preprocessors need a compiler plugin: ' + args.path);
        }
        for (const value of Object.values(node)) {
          if (Array.isArray(value)) value.forEach(visit);
          else if (value && typeof value === 'object') visit(value);
        }
      };
      visit(parsed.ast);
      const result = await transform(input, {
        filename: args.path, normalizedFilename: path.relative(root, args.path),
        internalURL: Number(version.split('.')[0]) >= 7 ? 'astro/compiler-runtime' : 'astro/runtime/server/index.js',
        resultScopedSlot: true, inlineComponentAssets: false, renderScript: true,
        // Astro's application compiler also provides this callback. It disables
        // legacy compiler metadata and lets the container own rendering.
        resolvePath: major >= 7 ? value => value : async value => value,
      });
      const errors = (result.diagnostics || []).filter(item => item.severity === 1 || item.severity === 'error');
      if (errors.length) throw new Error(args.path + ': ' + errors.map(item => item.text).join('\n'));
      if (result.hydratedComponents?.length || result.clientOnlyComponents?.length || result.serverComponents?.length)
        throw new Error('Astro client/server islands need application integrations; use a URL lens for ' + args.path);
      for (const [index, css] of (result.css || []).entries()) styles.add(resource(args.path, index, '.css', css));
      for (const [index, script] of (result.scripts || []).entries()) {
        const id = args.path + '?astro&type=script&index=' + index + '&lang.ts';
        if (script.type === 'external') {
          const target = path.resolve(path.dirname(args.path), script.src);
          dependencies.add(target);
          scripts.set(id, resource(target, 0, '.js', fs.readFileSync(target, 'utf8')));
        } else scripts.set(id, resource(args.path, index, '.js', script.code));
      }
      const code = await esbuild.transform(result.code, { loader: 'ts', define: { 'import.meta.url': JSON.stringify(pathToFileURL(args.path).href) } });
      return { contents: code.code, loader: 'js', resolveDir: path.dirname(args.path) };
    });
    build.onLoad({ filter: /\.[cm]?[jt]sx?$/ }, async args => {
      const extension = path.extname(args.path).slice(1);
      const loader = ['ts', 'tsx', 'jsx'].includes(extension) ? extension : 'js';
      const code = await esbuild.transform(fs.readFileSync(args.path, 'utf8'), {
        loader, define: { 'import.meta.url': JSON.stringify(pathToFileURL(args.path).href) },
      });
      return { contents: code.code, loader: 'js', resolveDir: path.dirname(args.path) };
    });
    build.onLoad({ filter: /\.css$/ }, args => {
      dependencies.add(args.path);
      styles.add(resource(args.path, 0, '.css', fs.readFileSync(args.path, 'utf8')));
      return { contents: '', loader: 'js' };
    });
    build.onLoad({ filter: /\.(svg|png|jpe?g|gif|webp|avif|woff2?|ttf)$/ }, args => {
      dependencies.add(args.path);
      return { contents: JSON.stringify('/' + path.relative(root, args.path).split(path.sep).join('/')), loader: 'json' };
    });
  } };
  const built = await esbuild.build({ absWorkingDir: root, entryPoints: [source], bundle: true,
    platform: 'node', format: 'esm', write: false, metafile: true, logLevel: 'silent',
    plugins: [...(config.plugins || []), plugin],
    define: { 'import.meta.env.BASE_URL': JSON.stringify('/'), 'import.meta.env.SITE': JSON.stringify('https://example.com'),
      'import.meta.env.DEV': 'false', 'import.meta.env.PROD': 'true', ...config.define },
  });
  for (const file of Object.keys(built.metafile.inputs)) dependencies.add(path.resolve(root, file));
  const module = await import('data:text/javascript;base64,' + Buffer.from(built.outputFiles[0].text).toString('base64'));
  const { experimental_AstroContainer } = await import(pathToFileURL(astro.resolve('astro/container')).href);
  return { resources, dependencies, publicDir: path.join(projectRoot, 'public'), package: { name: 'astro', version },
    async render(props, exported = 'default') {
      if (typeof module[exported] !== 'function') throw new Error('The selected Astro source export does not exist: ' + exported);
      const container = await experimental_AstroContainer.create({
        manifest: { inlinedScripts: new Map(Array.from(scripts, ([id, url]) => [id, `import ${JSON.stringify(url)};`])) },
      });
      const html = await container.renderToString(module[exported], { props, partial: true });
      return Array.from(styles, url => `<link rel="stylesheet" href="${url}">`).join('') + html;
    },
  };
}

module.exports = { create };
