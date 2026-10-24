/**
 * Astro renders for the built-in Astro adapter, with the project's own Astro
 * through its container API (`preview/astro.cjs`). Frontmatter runs here, so
 * its fetch calls get the scenario's request mocks. A renderer is reused until
 * a file it read changes.
 */
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
type Any = any;

const renderers = new Map<string, { renderer: Any; stamps: Map<string, string> }>();
const stampOf = (file: string): string => { try { const stat = fs.statSync(file); return stat.mtimeMs + ':' + stat.size; } catch { return 'missing'; } };

async function renderer(root: string, component: string, config: Any, production = false): Promise<Any> {
  const settings = { ...config, define: { 'import.meta.env.DEV': String(!production), 'import.meta.env.PROD': String(production), ...config.define } };
  const key = component + '\0' + JSON.stringify(settings.define) + '\0' + JSON.stringify(settings.aliases || {});
  const cached = renderers.get(key);
  if (cached && Array.from(cached.stamps).every(([file, stamp]) => stampOf(file) === stamp)) return cached.renderer;
  const created = await require('../../../preview/astro.cjs').create(root, component, settings);
  renderers.set(key, { renderer: created, stamps: new Map(Array.from(created.dependencies as Set<string>, file => [file, stampOf(file)])) });
  return created;
}

const local = (files: Iterable<string>): string[] => Array.from(files).filter(file => !/[\\/]node_modules[\\/]/.test(file) && fs.existsSync(file));

function resources(created: Any): Record<string, { contents: string; resolveDir: string | null }> {
  const out: Record<string, { contents: string; resolveDir: string | null }> = {};
  for (const [id, resource] of created.resources as Map<string, Any>) out[id] = { contents: resource.contents, resolveDir: resource.resolveDir || null };
  return out;
}

/** Each scenario of an Astro preview, or one scenario with edited inputs. */
export async function renderPreview(project: Any, root: string, file: string, scenario: string | null, inputs: unknown, production = false): Promise<unknown> {
  const { validate, mockFetches } = require('../../../preview/compiler.cjs');
  const config = await project.settings();
  const definition = validate((await project.definition(file)).value, file, root);
  if (inputs !== undefined && (!inputs || typeof inputs !== 'object' || Array.isArray(inputs))) throw new Error('Astro inputs must be an object.');
  const all: Array<[string, Any]> = Object.entries({ default: { source: definition.source }, ...definition.scenarios });
  if (scenario !== null && !all.some(([id]) => id === scenario)) throw new Error('Unknown preview scenario: ' + scenario);
  const scenarios: Record<string, unknown> = {};
  const found: Record<string, unknown> = {};
  const dependencies = new Set<string>();
  let publicDir: string | null = null;
  let astroPackage: unknown = null;
  for (const [id, spec] of all) {
    if (scenario !== null && id !== scenario) continue;
    const source = spec.source || definition.source;
    const component = path.resolve(path.dirname(file), source.entry);
    const created = await renderer(root, component, config, production);
    for (const dependency of created.dependencies) dependencies.add(dependency);
    Object.assign(found, resources(created));
    publicDir = created.publicDir;
    astroPackage = created.package;
    const values = { ...definition.inputs, ...spec.inputs, ...(scenario === id && inputs ? inputs as object : {}) };
    const html = await mockFetches(definition, id, values, () => created.render(values, source.export));
    scenarios[id] = { html, inputs: values, component };
  }
  return { scenarios, resources: found, dependencies: local(dependencies), publicDir, package: astroPackage };
}

/** One docs example: an Astro component rendered with no props. */
export async function renderExample(project: Any, root: string, component: string, production = false): Promise<unknown> {
  const config = await project.settings();
  const created = await renderer(root, component, config, production);
  return { html: await created.render({}, 'default'), component, resources: resources(created), dependencies: local(created.dependencies), publicDir: created.publicDir };
}
