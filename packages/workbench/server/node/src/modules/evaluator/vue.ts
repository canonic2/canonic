/**
 * Vue single-file components, compiled with Workbench's bundled
 * `@vue/compiler-sfc`: the script with its template, and each style block as
 * CSS, scoped by the component's ID. The server's Vue loaders turn the result
 * into modules of the build; the project need not install the compiler.
 *
 * Portable builds inline the template into `<script setup>`. Live builds keep
 * it a separate render function, as Vue Loader does in development, so a
 * template edit re-renders through Vue's hot reload and keeps the
 * component's state; `scriptKey` names everything but the template.
 */
import crypto from 'node:crypto';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

const require = createRequire(import.meta.url);
type Any = any;

export interface CompiledVue {
  /** The component's ID: scoped styles use `data-v-<id>`, hot reload uses it as is. */
  id: string;
  script: string;
  /** The script's language, for the compiler that runs after the Vue loader. */
  lang: string;
  scoped: boolean;
  styles: Array<{ code: string; module: boolean }>;
  /** Changes with anything but the template: a hot update with the same key only re-renders. */
  scriptKey: string;
  /** The `<style src>` files it read, absolute: an edit to one rebuilds the component. */
  dependencies: string[];
}

const cache = new Map<string, { result: CompiledVue; read: Array<[string, string | null]> }>();
const digest = (value: string): string => crypto.createHash('sha256').update(value).digest('hex').slice(0, 8);

export function compileVue(file: string, source: string, hot = false): CompiledVue {
  const key = file + '\0' + (hot ? 'hot\0' : '') + digest(source);
  const cached = cache.get(key);
  // A `<style src>` file edited since is compiled again.
  if (cached && cached.read.every(([dependency, content]) => contentOf(dependency) === content)) return cached.result;
  const sfc: Any = require('@vue/compiler-sfc');
  const { descriptor, errors } = sfc.parse(source, { filename: file });
  if (errors.length) throw new Error(errors.map((error: Any) => String(error.message || error)).join('\n'));
  const id = digest(file);
  const scoped = descriptor.styles.some((style: Any) => style.scoped);
  const lang = (descriptor.scriptSetup && descriptor.scriptSetup.lang) || (descriptor.script && descriptor.script.lang) || 'js';
  let script = '';
  if (descriptor.script || descriptor.scriptSetup) {
    const inlineTemplate = !hot;
    const compiled = sfc.compileScript(descriptor, { id, genDefaultAs: '__component', inlineTemplate, templateOptions: { scoped } });
    script = compiled.content;
    if ((!descriptor.scriptSetup || !inlineTemplate) && descriptor.template) {
      const expressionPlugins = lang === 'ts' || lang === 'tsx' ? ['typescript'] : undefined;
      const template = sfc.compileTemplate({ source: descriptor.template.content, filename: file, id, scoped, compilerOptions: { bindingMetadata: compiled.bindings, expressionPlugins } });
      if (template.errors.length) throw new Error(template.errors.join('\n'));
      script += '\n' + template.code + '\n__component.render = render;';
    }
  } else {
    const template = sfc.compileTemplate({ source: descriptor.template ? descriptor.template.content : '', filename: file, id, scoped });
    if (template.errors.length) throw new Error(template.errors.join('\n'));
    script = template.code + '\nconst __component = { render };';
  }
  const dependencies: string[] = [];
  const styles = descriptor.styles.map((style: Any) => {
    if (style.lang && style.lang !== 'css') throw new Error('Vue style preprocessors are not supported; use plain CSS: ' + file);
    let source = style.content;
    let filename = file;
    if (style.src) {
      filename = styleSource(file, style.src);
      dependencies.push(filename);
      source = fs.readFileSync(filename, 'utf8');
    }
    const compiled = sfc.compileStyle({ source, filename, id: 'data-v-' + id, scoped: !!style.scoped });
    if (compiled.errors.length) throw new Error(compiled.errors.join('\n'));
    return { code: compiled.code, module: !!style.module };
  });
  const scriptKey = digest(JSON.stringify([
    descriptor.script && [descriptor.script.content, descriptor.script.lang],
    descriptor.scriptSetup && [descriptor.scriptSetup.content, descriptor.scriptSetup.lang],
    descriptor.styles.map((style: Any) => [!!style.scoped, style.module || false]),
    descriptor.cssVars,
  ]));
  const result = { id, script, lang, scoped, styles, scriptKey, dependencies };
  cache.set(key, { result, read: dependencies.map(dependency => [dependency, contentOf(dependency)]) });
  while (cache.size > 500) cache.delete(cache.keys().next().value as string);
  return result;
}

/** A file's content digest, or null when it can't be read. */
function contentOf(file: string): string | null {
  try { return digest(fs.readFileSync(file, 'utf8')); } catch { return null; }
}

/**
 * The CSS file a `<style src>` names, resolved as Vue Loader resolves it:
 * relative to the component, or from a package (`~` optional).
 */
function styleSource(component: string, src: string): string {
  if (!/\.css$/i.test(src.split('?')[0])) throw new Error(`Vue style src must name a .css file: ${src} in ${component}`);
  const from = path.dirname(component);
  const request = src.replace(/^~/, '');
  if (request.startsWith('.') || path.isAbsolute(request)) {
    const file = path.resolve(from, request);
    if (!fs.existsSync(file)) throw new Error(`Vue style src not found: ${src} in ${component}`);
    return file;
  }
  try {
    return createRequire(path.join(from, 'index.js')).resolve(request);
  } catch {
    throw new Error(`Vue style src not found: ${src} in ${component}`);
  }
}
