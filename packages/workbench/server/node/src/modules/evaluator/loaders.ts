/**
 * A project's JavaScript loader over one module, with the parts of the
 * webpack loader context loaders commonly use: options, the resource, an
 * async callback, dependencies, and warnings. Child compilations
 * (`loadModule`, `importModule`) belong to the compiler and are refused.
 */
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

type Any = any;

export interface LoaderRequest {
  root: string;
  /** The loader's absolute path, with its query if the rule gave one. */
  loader: string;
  options?: unknown;
  resourcePath: string;
  resourceQuery?: string;
  /** The module's source, as text, or base64 when the loader is raw. */
  source: string;
  encoding?: 'utf8' | 'base64';
  hot?: boolean;
}

export interface LoaderResult {
  source: string;
  encoding: 'utf8' | 'base64';
  map: unknown;
  dependencies: string[];
  contextDependencies: string[];
  missingDependencies: string[];
  cacheable: boolean;
  /** `this.emitError` messages: the build fails on them. */
  errors: string[];
  warnings: string[];
}

const loaded = new Map<string, { stamp: string; loader: Any }>();
const stampOf = (file: string): string => { try { const stat = fs.statSync(file); return stat.mtimeMs + ':' + stat.size; } catch { return 'missing'; } };

/** The loader function, loaded again when its file changes. */
async function loaderModule(file: string): Promise<Any> {
  const stamp = stampOf(file);
  const cached = loaded.get(file);
  if (cached && cached.stamp === stamp) return cached.loader;
  const require = createRequire(file);
  let module: Any;
  try {
    delete require.cache[file];
    module = require(file);
  } catch (error) {
    if ((error as Any).code !== 'ERR_REQUIRE_ESM') throw error;
    module = await import(pathToFileURL(file).href + '?t=' + encodeURIComponent(stamp));
  }
  const loader = typeof module === 'function' ? module : module && typeof module.default === 'function' ? Object.assign(module.default, { raw: module.raw ?? module.default.raw }) : null;
  if (!loader) throw new Error('The loader ' + file + ' does not export a function.');
  loaded.set(file, { stamp, loader });
  return loader;
}

export async function runLoader(request: LoaderRequest): Promise<LoaderResult> {
  const [file, query = ''] = request.loader.split(/(?=\?)/);
  const loader = await loaderModule(file);
  const raw = !!loader.raw;
  const input = request.encoding === 'base64' ? Buffer.from(request.source, 'base64') : request.source;
  const content = raw ? Buffer.from(input) : String(input);
  const dependencies = new Set<string>([file]);
  const contextDependencies = new Set<string>();
  const missingDependencies = new Set<string>();
  const errors: string[] = [];
  const warnings: string[] = [];
  let cacheable = true;
  const resourceQuery = request.resourceQuery || '';
  return new Promise<LoaderResult>((resolve, reject) => {
    let settled = false;
    const finish = (error: unknown, result?: string | Buffer, map?: unknown) => {
      if (settled) return;
      settled = true;
      if (error) { reject(error instanceof Error ? error : new Error(String(error))); return; }
      const output = result === undefined ? content : result;
      const binary = Buffer.isBuffer(output);
      resolve({
        source: binary ? (output as Buffer).toString('base64') : String(output), encoding: binary ? 'base64' : 'utf8',
        map: typeof map === 'string' ? JSON.parse(map) : map ?? null,
        dependencies: [...dependencies], contextDependencies: [...contextDependencies], missingDependencies: [...missingDependencies],
        cacheable, errors, warnings,
      });
    };
    let asynchronous = false;
    const options = request.options === undefined ? (query ? Object.fromEntries(new URLSearchParams(query.slice(1))) : {}) : request.options;
    const context: Any = {
      version: 2, mode: 'development', target: 'web', webpack: false, hot: !!request.hot, sourceMap: false,
      rootContext: request.root, context: path.dirname(request.resourcePath),
      resourcePath: request.resourcePath, resourceQuery, resourceFragment: '', resource: request.resourcePath + resourceQuery,
      query: request.options === undefined ? query : options, loaders: [], loaderIndex: 0, data: {},
      getOptions: () => options,
      async: () => { asynchronous = true; return finish; },
      callback: (error: unknown, result?: string | Buffer, map?: unknown) => { asynchronous = true; finish(error, result, map); },
      cacheable: (flag = true) => { cacheable = flag !== false; },
      addDependency: (dependency: string) => { dependencies.add(path.resolve(dependency)); },
      dependency: (dependency: string) => { dependencies.add(path.resolve(dependency)); },
      addContextDependency: (dependency: string) => { contextDependencies.add(path.resolve(dependency)); },
      addMissingDependency: (dependency: string) => { missingDependencies.add(path.resolve(dependency)); },
      getDependencies: () => [...dependencies], getContextDependencies: () => [...contextDependencies],
      clearDependencies: () => { dependencies.clear(); contextDependencies.clear(); missingDependencies.clear(); cacheable = true; },
      emitWarning: (warning: unknown) => { warnings.push(String((warning as Error)?.message || warning)); },
      emitError: (error: unknown) => { errors.push(String((error as Error)?.message || error)); },
      getLogger: () => ({ error: console.error, warn: console.warn, info: console.error, log: console.error, debug: () => {} }),
      utils: {
        absolutify: (context: string, request: string) => path.resolve(context, request),
        contextify: (context: string, request: string) => './' + path.relative(context, request).split(path.sep).join('/'),
      },
      loadModule: () => { throw new Error('this.loadModule is not available to project loaders in Workbench.'); },
      importModule: () => { throw new Error('this.importModule is not available to project loaders in Workbench.'); },
      resolve: (context: string, request: string, callback: (error: unknown, result?: string) => void) => {
        try { callback(null, createRequire(path.join(context, 'index.js')).resolve(request)); } catch (error) { callback(error); }
      },
    };
    context.getResolve = () => (context_: string, request_: string, callback?: (error: unknown, result?: string) => void) => {
      const run = () => createRequire(path.join(context_, 'index.js')).resolve(request_);
      if (callback) { try { callback(null, run()); } catch (error) { callback(error); } return undefined; }
      return Promise.resolve().then(run);
    };
    try {
      const result = loader.call(context, content, null, undefined);
      if (asynchronous) return;
      if (result && typeof result.then === 'function') result.then((value: string | Buffer) => finish(null, value), (error: unknown) => finish(error));
      else finish(null, result);
    } catch (error) { finish(error); }
  });
}
