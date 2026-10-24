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

const has = (map: Any, key: string): boolean => !!map && typeof map === 'object' && Object.prototype.hasOwnProperty.call(map, key);

/**
 * Each scenario of an Astro preview; each declared artboard that changes what
 * the server renders (its inputs, or request mocks that can read
 * `context.artboard`) under `<scenario>+<artboard>/`; and each iteration that
 * does (its own source, inputs, fixtures, globals or requests) under its
 * source key, `<scenario>+<iteration>` or `<scenario>+<artboard>/<iteration>`;
 * or one of them with edited inputs. A requested artboard or iteration without
 * its own render uses its scenario's, rendered at its position, so request
 * handlers see `context.artboard` and `context.iteration` as the browser does.
 */
export async function renderPreview(project: Any, root: string, file: string, scenario: string | null, inputs: unknown, production = false): Promise<unknown> {
  const { validate, mockFetches, iterationsOf } = require('../../../preview/compiler.cjs');
  const config = await project.settings();
  const definition = validate((await project.definition(file)).value, file, root);
  if (inputs !== undefined && (!inputs || typeof inputs !== 'object' || Array.isArray(inputs))) throw new Error('Astro inputs must be an object.');
  const specs: Record<string, Any> = { default: { source: definition.source }, ...definition.scenarios };
  const all: Array<[string, Any, Any]> = [];
  const declared = new Map<string, Any>();
  for (const [id, spec] of Object.entries<Any>(specs)) {
    all.push([id, spec, null]);
    for (const [artboard, board] of Object.entries<Any>(spec.artboards || {})) {
      const key = id + '+' + artboard + '/';
      const found = { key, artboard, artboardInputs: board.inputs, id: null, iteration: {} };
      declared.set(key, found);
      // Request mocks see `context.artboard`, so they can render it differently.
      if (board.inputs || spec.requests || definition.requests) all.push([key, spec, found]);
    }
    for (const found of iterationsOf(id, spec)) {
      declared.set(found.key, found);
      const own = found.iteration;
      // Fixtures and globals reach the server render only through request mocks.
      const mocked = own.requests || spec.requests || definition.requests;
      if (own.source || own.inputs || own.requests || (mocked && (own.fixtures || own.globals))) all.push([found.key, spec, found]);
    }
  }
  // The render to run and, when it is its scenario's, the position it renders at.
  let requested: string | null = null;
  let at: Any = null;
  if (scenario !== null) {
    const mark = scenario.indexOf('+');
    const name = mark < 0 ? scenario : scenario.slice(0, mark);
    if (!has(specs, name)) throw new Error('Unknown preview scenario: ' + name);
    requested = name;
    if (mark >= 0) {
      // Named as the browser names them: the artboard and the iteration's own ids, not the source key.
      const rest = scenario.slice(mark + 1);
      const slash = rest.indexOf('/');
      const artboard = slash < 0 ? null : rest.slice(0, slash);
      const iteration = slash < 0 ? rest : rest.slice(slash + 1);
      if (artboard !== null && !has(specs[name].artboards, artboard)) throw new Error('Unknown preview artboard: ' + artboard);
      at = declared.get(scenario);
      if (!at) throw new Error('Unknown preview iteration: ' + iteration);
      if (all.some(([id]) => id === scenario)) requested = scenario;
    }
  }
  const scenarios: Record<string, unknown> = {};
  const found: Record<string, unknown> = {};
  const dependencies = new Set<string>();
  let publicDir: string | null = null;
  let astroPackage: unknown = null;
  for (const [id, spec, entryPosition] of all) {
    if (requested !== null && id !== requested) continue;
    const position = at || entryPosition;
    const own = position ? position.iteration : {};
    const source = own.source || spec.source || definition.source;
    const component = path.resolve(path.dirname(file), source.entry);
    const created = await renderer(root, component, config, production);
    for (const dependency of created.dependencies) dependencies.add(dependency);
    Object.assign(found, resources(created));
    publicDir = created.publicDir;
    astroPackage = created.package;
    const values = { ...definition.inputs, ...spec.inputs, ...position?.artboardInputs, ...own.inputs, ...(requested === id && inputs ? inputs as object : {}) };
    const html = await mockFetches(definition, id.split('+')[0], values, () => created.render(values, source.export), position);
    scenarios[scenario ?? id] = { html, inputs: values, component };
  }
  return { scenarios, resources: found, dependencies: local(dependencies), publicDir, package: astroPackage };
}

/** One docs example: an Astro component rendered with no props. */
export async function renderExample(project: Any, root: string, component: string, production = false): Promise<unknown> {
  const config = await project.settings();
  const created = await renderer(root, component, config, production);
  return { html: await created.render({}, 'default'), component, resources: resources(created), dependencies: local(created.dependencies), publicDir: created.publicDir };
}
