/**
 * Rspack options for one profile's compiler: Workbench's format handling,
 * then the project's plugins and rules from `workbench.config.ts`.
 */
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { BASE } from './client-sources.ts';

export type Profile = 'web' | 'react-native-web';

/** The project config fields the pipeline reads. */
export interface ProjectConfig {
  aliases?: Record<string, string>;
  dedupe?: string[];
  resolveExtensions?: string[];
  define?: Record<string, string>;
  environment?: string | Record<string, string>;
  plugins?: unknown[];
  rules?: unknown[];
  adapters?: Record<string, { runtime?: string; plugins?: unknown[]; rules?: unknown[] } | undefined>;
}

export const profileOf = (adapter: string): Profile => adapter === 'react-native-web' ? 'react-native-web' : 'web';

const EXTENSIONS = ['.tsx', '.ts', '.jsx', '.js', '.mjs', '.cjs', '.json', '.vue', '.html'];
const NATIVE_EXTENSIONS = ['.web.tsx', '.web.ts', '.web.jsx', '.web.js'];

/** A plugin must be an Rspack plugin: an object with `apply(compiler)`. */
const isPlugin = (value: unknown): boolean => !!value && typeof value === 'object' && typeof (value as { apply?: unknown }).apply === 'function';

/**
 * The project's plugins and rules for one profile. An adapter's plugins and
 * rules apply only to the compiler that builds that adapter's previews. A
 * plugin in esbuild's shape, or anything else without `apply`, is reported
 * and left out.
 */
export function projectExtensions(config: ProjectConfig, profile: Profile): { plugins: unknown[]; rules: unknown[]; problems: string[] } {
  const problems: string[] = [];
  const plugins: unknown[] = [];
  const rules: unknown[] = [];
  const take = (list: unknown[] | undefined, where: string) => {
    for (const plugin of list || []) {
      if (isPlugin(plugin)) { plugins.push(plugin); continue; }
      const name = plugin && typeof plugin === 'object' && typeof (plugin as { name?: unknown }).name === 'string' ? '“' + (plugin as { name: string }).name + '”' : 'a plugin';
      const esbuild = plugin && typeof plugin === 'object' && typeof (plugin as { setup?: unknown }).setup === 'function';
      problems.push('workbench.config.ts: ' + where + name + (esbuild ? ' is an esbuild plugin. Workbench compiles previews with Rspack: use an Rspack plugin or a loader rule.' : ' is not an Rspack plugin (an object with apply(compiler)).'));
    }
  };
  take(config.plugins, '');
  rules.push(...(Array.isArray(config.rules) ? config.rules : []));
  for (const [name, adapter] of Object.entries(config.adapters || {})) {
    if (!adapter || profileOf(name) !== profile) continue;
    take(adapter.plugins, 'adapter ' + name + ': ');
    rules.push(...(Array.isArray(adapter.rules) ? adapter.rules : []));
  }
  return { plugins, rules, problems };
}

/** Exact aliases (relative targets from the project root) and deduplicated packages. */
export function resolveAliases(root: string, config: ProjectConfig, profile: Profile, api: string): Record<string, string> {
  const aliases: Record<string, string> = { '@canonic2/workbench$': api };
  if (profile === 'react-native-web') aliases['react-native$'] = 'react-native-web';
  for (const [key, value] of Object.entries(config.aliases || {})) {
    aliases[key + '$'] = value.startsWith('.') || path.isAbsolute(value) ? path.resolve(root, value) : value;
  }
  const projectRequire = createRequire(path.join(root, 'package.json'));
  for (const name of config.dedupe || []) {
    try { aliases[name] = path.dirname(projectRequire.resolve(name + '/package.json')); } catch (_) { /* An unresolvable package stays as written. */ }
  }
  return aliases;
}

export interface OptionsInput {
  profile: Profile;
  generation: number;
  root: string;
  config: ProjectConfig;
  previewDir: string;
  cacheDir: string;
  development: boolean;
  /** A portable build: one self-contained ES module, named by its entry. */
  portable?: boolean;
  entry: () => Record<string, string[]>;
  plugins: unknown[];
  loaders: { vue: string; style: string; css: string; html: string; htmlPart: string };
}

/** The compiler's options. Output goes to `/out/<profile>/<generation>/` in memory. */
export function compilerOptions(input: OptionsInput): Record<string, unknown> {
  const { profile, config, development } = input;
  const extensions = config.resolveExtensions || (profile === 'react-native-web' ? NATIVE_EXTENSIONS : []).concat(EXTENSIONS);
  const project = projectExtensions(config, profile);
  const swc = { loader: 'builtin:swc-loader', options: { jsc: { parser: { syntax: 'typescript', tsx: true }, transform: { react: { runtime: 'automatic', development, refresh: development } }, target: 'es2020' } } };
  // Scripts taken from an HTML page are modules whatever their file is called.
  const swcModule = { loader: 'builtin:swc-loader', options: { ...swc.options, isModule: true } };
  const rules = [
    { test: /\.[cm]?[jt]sx?$/, exclude: /node_modules/, type: 'javascript/auto', oneOf: [
      // A local script an HTML preview loads, run again on each render.
      { resourceQuery: /workbench-run/, use: [swcModule, input.loaders.htmlPart] },
      { use: [swc] },
    ] },
    { test: /\.vue$/, loader: input.loaders.vue, options: { hotReload: development } },
    { test: /\.html?$/, oneOf: [
      { resourceQuery: /workbench-style/, loader: input.loaders.htmlPart, type: 'css' },
      { resourceQuery: /workbench-script/, use: [swcModule, input.loaders.htmlPart], type: 'javascript/auto' },
      { loader: input.loaders.html },
    ] },
    { test: /\.css$/, oneOf: [
      // Vue Loader routes each style block through a JavaScript module, which
      // Rspack's native CSS cannot take; these use the classic loaders.
      // A `<style module>` block's class names are its default export, as Vue Loader imports them.
      { resourceQuery: /vue/, type: 'javascript/auto', use: [input.loaders.style, { loader: input.loaders.css,
        options: { modules: { auto: (_file: string, query: string) => /[?&]module\b/.test(query), namedExport: false, exportLocalsConvention: 'as-is' } } }] },
      // Projects import CSS modules as a default object.
      { test: /\.module\.css$/, type: 'css/module', parser: { namedExports: false } },
      { type: 'css' },
    ] },
    { test: /\.(svg|png|jpe?g|gif|webp|avif|ico|bmp|woff2?|ttf|otf|eot|mp4|webm|mov|mp3|ogg|wav|m4a)$/, type: 'asset/resource' },
    // Workbench's package declares no side effects, but its request mocks and
    // React Native Web host styles are imported for nothing else.
    { include: [path.join(input.previewDir, 'requests.js'), path.join(input.previewDir, 'react-native-web.css')], sideEffects: true },
    ...project.rules,
  ];
  const publicPath = BASE + profile + '/' + input.generation + '/';
  const portable = input.portable;
  const output = portable
    ? { path: '/out', publicPath: 'auto', clean: true, module: true, chunkFormat: 'module', chunkLoading: 'import', library: { type: 'module' },
      filename: '[name].js', chunkFilename: 'chunks/[contenthash:8].js', cssFilename: '[name].css',
      cssChunkFilename: 'chunks/[contenthash:8].css', assetModuleFilename: 'assets/[name].[hash:8][ext]' }
    : { path: '/out/' + profile + '/' + input.generation, publicPath, filename: '[name].[contenthash:8].js', chunkFilename: 'chunks/[name].[contenthash:8].js',
      // Stylesheets keep stable names: Rspack's CSS hot update fetches a
      // chunk's stylesheet by the name its runtime already knows.
      cssFilename: '[name].css', cssChunkFilename: 'chunks/[name].css', assetModuleFilename: 'assets/[name].[hash:8][ext]',
      uniqueName: 'wb_' + profile.replace(/-/g, '_'), hotUpdateChunkFilename: 'hot/[id].[fullhash].hot-update.js', hotUpdateMainFilename: 'hot/[runtime].[fullhash].hot-update.json' };
  return {
    name: profile, mode: development ? 'development' : 'production', devtool: false, context: input.root, target: 'web',
    entry: input.entry,
    output,
    ...(portable ? { experiments: { outputModule: true } } : {}),
    externalsType: 'window',
    externals: { '/_workbench/preview-runtime.js': '__workbenchRuntime' },
    resolve: { alias: resolveAliases(input.root, config, profile, path.join(input.previewDir, 'api.js')), extensions, modules: [path.join(input.root, 'node_modules'), 'node_modules'],
      // The project's `paths`, such as `@/*`, resolve as they do in its own build.
      ...(fs.existsSync(path.join(input.root, 'tsconfig.json')) ? { tsConfig: { configFile: path.join(input.root, 'tsconfig.json'), references: 'auto' } } : {}) },
    module: { rules },
    plugins: [...input.plugins, ...project.plugins],
    optimization: portable ? { splitChunks: false, runtimeChunk: false, minimize: true }
      : { splitChunks: { chunks: 'all' }, runtimeChunk: 'single', minimize: false, moduleIds: 'named' },
    // Rspack's default location is node_modules/.cache inside the project.
    cache: portable ? true : { type: 'persistent', name: profile, storage: { type: 'filesystem', directory: input.cacheDir } },
    infrastructureLogging: { level: 'error' }, stats: 'errors-only',
    watchOptions: { ignored: ['**/node_modules/**', '**/.git/**'], aggregateTimeout: 5 },
  };
}
