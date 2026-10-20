/**
 * Project configuration and definitions as data for the Workbench server.
 * Regular expressions become `{ $regexp: { source, flags } }`; a function the
 * server cannot use becomes a problem naming where it was. Loader paths in
 * module rules resolve from the project root, as Rspack resolves them.
 */
import path from 'node:path';
import { createRequire } from 'node:module';

type Any = any;

const isObject = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);

/** Plain data: regular expressions tagged, functions and other values reported. */
export function toData(value: unknown, where: string, problems: string[]): unknown {
  if (value instanceof RegExp) return { $regexp: { source: value.source, flags: value.flags } };
  if (typeof value === 'function') { problems.push(where + ' is a function, which Workbench cannot use here.'); return undefined; }
  if (Array.isArray(value)) return value.map((item, index) => toData(item, where + '[' + index + ']', problems)).filter(item => item !== undefined);
  if (isObject(value)) {
    const out: Record<string, unknown> = {};
    for (const [key, item] of Object.entries(value)) {
      const data = toData(item, where + '.' + key, problems);
      if (data !== undefined) out[key] = data;
    }
    return out;
  }
  if (typeof value === 'bigint' || typeof value === 'symbol') { problems.push(where + ' cannot be read as data.'); return undefined; }
  return value;
}

const pluginName = (plugin: unknown): string =>
  isObject(plugin) && typeof plugin.name === 'string' ? '“' + plugin.name + '”' : isObject(plugin) && plugin.constructor && plugin.constructor.name !== 'Object' ? '“' + plugin.constructor.name + '”' : 'a plugin';

/**
 * Native plugin capabilities are plain objects naming their `plugin`, made by
 * the helpers `@canonic2/workbench` exports. JavaScript plugin objects, with
 * Rspack's `apply` or esbuild's `setup`, do not run in the native compiler.
 */
function plugins(list: unknown, where: string, problems: string[]): unknown[] {
  const out: unknown[] = [];
  for (const plugin of Array.isArray(list) ? list : []) {
    if (isObject(plugin) && typeof plugin.plugin === 'string' && typeof plugin.apply !== 'function' && typeof plugin.setup !== 'function') {
      out.push(toData(plugin, where + pluginName(plugin), problems));
      continue;
    }
    const kind = isObject(plugin) && typeof plugin.setup === 'function' ? 'an esbuild plugin'
      : isObject(plugin) && typeof plugin.apply === 'function' ? 'a JavaScript Rspack plugin' : 'not a Workbench plugin';
    problems.push('workbench.config.ts: ' + where + pluginName(plugin) + ' is ' + kind + '. Workbench compiles previews natively: use a plugin from @canonic2/workbench, or a loader rule.');
  }
  return out;
}

/** A loader request resolved from the project root; built-in loaders stay as named. */
function loaderPath(request: string, root: string): string {
  if (request.startsWith('builtin:')) return request;
  const [name, query = ''] = request.split(/(?=\?)/);
  const projectRequire = createRequire(path.join(root, 'package.json'));
  const target = name.startsWith('.') ? path.resolve(root, name) : name;
  return projectRequire.resolve(target) + query;
}

function rules(list: unknown, where: string, root: string, problems: string[]): unknown[] {
  const resolveUse = (use: unknown, at: string): unknown => {
    if (typeof use === 'string') return { loader: loaderPath(use, root) };
    if (Array.isArray(use)) return use.map((item, index) => resolveUse(item, at + '[' + index + ']'));
    if (isObject(use) && typeof use.loader === 'string') return { ...use, loader: loaderPath(use.loader, root) };
    return use;
  };
  const rule = (value: unknown, at: string): unknown => {
    if (!isObject(value)) { problems.push('workbench.config.ts: ' + at + ' is not a module rule.'); return undefined; }
    const out: Record<string, unknown> = { ...value };
    try {
      if (typeof out.loader === 'string') out.loader = loaderPath(out.loader, root);
      if (out.use !== undefined) out.use = resolveUse(out.use, at + '.use');
    } catch (error) {
      problems.push('workbench.config.ts: ' + at + ': ' + String((error as Error).message || error).split('\n')[0]);
      return undefined;
    }
    for (const key of ['oneOf', 'rules']) if (Array.isArray(out[key])) out[key] = (out[key] as unknown[]).map((item, index) => rule(item, at + '.' + key + '[' + index + ']')).filter(Boolean);
    return toData(out, 'workbench.config.ts: ' + at, problems);
  };
  return (Array.isArray(list) ? list : []).map((value, index) => rule(value, where + 'rules[' + index + ']')).filter(Boolean);
}

/** `workbench.config.ts`'s value as the server reads it. */
export function serializeConfig(config: Any, root: string): { config: unknown; problems: string[] } {
  const problems: string[] = [];
  const source = isObject(config) ? config : {};
  const out: Record<string, unknown> = {};
  for (const key of ['aliases', 'dedupe', 'resolveExtensions', 'define', 'environment']) {
    if (source[key] !== undefined) out[key] = toData(source[key], 'workbench.config.ts: ' + key, problems);
  }
  out.plugins = plugins(source.plugins, '', problems);
  out.rules = rules(source.rules, '', root, problems);
  const adapters: Record<string, unknown> = {};
  for (const [name, adapter] of Object.entries(isObject(source.adapters) ? source.adapters : {})) {
    if (!isObject(adapter)) continue;
    adapters[name] = {
      ...(typeof adapter.runtime === 'string' ? { runtime: adapter.runtime } : {}),
      plugins: plugins(adapter.plugins, 'adapter ' + name + ': ', problems),
      rules: rules(adapter.rules, 'adapter ' + name + ' ', root, problems),
    };
  }
  out.adapters = adapters;
  out.problems = problems;
  return { config: out, problems };
}

/** A validated definition's data; functions stay in the browser bundle. */
export function serializeDefinition(definition: unknown): unknown {
  return toData(definition, 'definition', []);
}
