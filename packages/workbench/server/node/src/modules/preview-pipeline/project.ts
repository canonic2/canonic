/**
 * What live and portable builds share about a project: which module mounts
 * an adapter, which environment wraps it, and how an Astro render becomes
 * modules the compiler builds.
 */
import fs from 'node:fs';
import path from 'node:path';
import { digest, type AdapterImport } from './entries.ts';
import type { ProjectConfig } from './options.ts';

const BUILTINS: Record<string, string> = { html: 'html.js', react: 'react.js', vue: 'vue.js', 'react-native-web': 'react.js', astro: 'astro.js' };

export function adapterImport(config: ProjectConfig, root: string, previewDir: string, adapter: string): AdapterImport {
  const addon = config.adapters && config.adapters[adapter];
  if (addon && addon.runtime) return { file: path.resolve(root, addon.runtime), native: false };
  if (!BUILTINS[adapter]) throw new Error('Unknown adapter “' + adapter + '”. Register it in workbench.config.ts.');
  const file = path.join(previewDir, BUILTINS[adapter]);
  return adapter === 'react-native-web' ? { file, native: true, nativeStyles: path.join(previewDir, 'react-native-web.css') } : { file, native: false };
}

/** One environment for every preview, or one per adapter name. */
export function projectEnvironment(config: ProjectConfig, root: string, adapter: string): string | null {
  const value = typeof config.environment === 'string' ? config.environment
    : config.environment && typeof config.environment === 'object' ? config.environment[adapter] : undefined;
  if (!value) return null;
  const target = path.resolve(root, value);
  if (!fs.existsSync(target)) throw new Error('The preview config environment must be a file inside the project: ' + value);
  return target;
}

/** The built-in Astro adapter renders in Node; a registered `astro` adapter replaces it. */
export const builtInAstro = (config: ProjectConfig, adapter: string): boolean => adapter === 'astro' && !(config.adapters && config.adapters.astro);

/**
 * Writes an Astro render as modules: each resource Astro emitted (styles,
 * client scripts) as its own file, and the HTML, which names them with
 * `wbfile:`. Root-relative URLs name files in the project's public folder, and
 * relative ones resolve from the component (`component`).
 * Returns the module that exports `html` and `inputs`.
 */
export function astroModules(virtualDir: string, key: string, component: string, html: string, resources: Map<string, { contents: string; resolveDir?: string }>, inputs: unknown,
  publicDir: string | null, write: (file: string, contents: string) => void): string {
  for (const [id, resource] of resources) {
    // Beside the component it came from, so its relative imports resolve.
    const file = path.join(resource.resolveDir || path.join(virtualDir, 'astro'), '.workbench-astro-' + digest(id) + (id.endsWith('.css') ? '.css' : '.ts'));
    write(file, resource.contents);
    html = html.split(id).join('wbfile:' + file);
  }
  if (publicDir) html = html.replace(/\b(src|href|poster)=(["'])(\/[^"'?#]+)/g, (match, attribute: string, quote: string, url: string) =>
    fs.existsSync(path.join(publicDir, url.slice(1))) ? attribute + '=' + quote + 'wbfile:' + path.join(publicDir, url.slice(1)) : match);
  // Beside the component, so relative URLs in it resolve as the author wrote them.
  const htmlFile = path.join(path.dirname(component), '.workbench-astro-' + digest(key) + '.html');
  write(htmlFile, html);
  const module = path.join(virtualDir, 'astro', key + '.js');
  write(module, 'export { default as html } from ' + JSON.stringify(htmlFile) + ';\nexport const inputs = ' + JSON.stringify(inputs) + ';\n');
  return module;
}
