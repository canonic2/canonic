/**
 * The Node evaluator: the Workbench server's child process for project
 * JavaScript. It evaluates `workbench.config.ts` and computed definitions,
 * runs a project's JavaScript loaders one module at a time, compiles Vue
 * single-file components and renders Astro pages. The server sends one JSON
 * request per line on stdin and reads one answer per line on stdout:
 *
 *   in:  {"id": 1, "op": "config", "root": "/acme", "options": {...}}
 *   out: {"id": 1, "ok": true, "value": {...}} or {"id": 1, "ok": false, "error": "..."}
 *
 * Project output goes to stderr; stdout carries only answers. The server
 * starts it only in a trusted workspace.
 */
import path from 'node:path';
import { createRequire } from 'node:module';
import { createInterface } from 'node:readline';
import { serializeConfig, serializeDefinition } from './serialize.ts';
import { runLoader } from './loaders.ts';
import { compileVue } from './vue.ts';
import { renderExample, renderPreview } from './astro.ts';

const require = createRequire(import.meta.url);
// The esbuild-based evaluation and validation the page index's evaluator uses.
const { Compiler, validate } = require('../../../preview/compiler.cjs');

type Request = { id: number; op: string; root: string; [key: string]: unknown };
type Any = any;

const compilers = new Map<string, Any>();
function compiler(root: string, options: unknown): Any {
  const key = root + '\0' + JSON.stringify(options || {});
  let found = compilers.get(key);
  if (!found) { found = new Compiler(root, options || {}); compilers.set(key, found); }
  return found;
}

const relative = (root: string, files: Iterable<string>): string[] =>
  Array.from(new Set(files), file => path.relative(root, file).split(path.sep).join('/'));

export const operations: Record<string, (request: Request) => Promise<unknown>> = {
  /** `workbench.config.ts` as data, with the files its evaluation read. */
  async config(request) {
    const project = compiler(request.root, request.options);
    const config = await project.settings();
    return { ...serializeConfig(config, request.root), files: relative(request.root, project.configFiles.keys()) };
  },
  /** One definition, validated, without its functions, and the files it read. */
  async definition(request) {
    const project = compiler(request.root, request.options);
    await project.settings();
    const file = project.declared(String(request.file));
    const evaluated = await project.definition(file);
    const value = evaluated.value && evaluated.value.kind === 'docs' ? evaluated.value : validate(evaluated.value, file, request.root);
    return { value: serializeDefinition(value), files: relative(request.root, [file, ...evaluated.files, ...project.configFiles.keys()]) };
  },
  /** A project JavaScript loader over one module's source. */
  loader: request => runLoader(request as Any),
  /** A Vue single-file component's script and styles. */
  vue: async request => compileVue(String(request.file), String(request.source), request.hot === true),
  /** An Astro preview's states, rendered; or one state with edited inputs. */
  async astro(request) {
    const project = compiler(request.root, request.options);
    await project.settings();
    const file = project.declared(String(request.file));
    return renderPreview(project, request.root, file, typeof request.state === 'string' ? request.state : null, request.inputs, request.production === true);
  },
  /** One Astro component of a docs lens's folder, rendered. */
  async astroExample(request) {
    const project = compiler(request.root, request.options);
    const component = path.resolve(request.root, String(request.file));
    if (path.relative(request.root, component).startsWith('..')) throw new Error('Docs examples must be inside the project.');
    return renderExample(project, request.root, component, request.production === true);
  },
};

export async function answer(request: Request): Promise<unknown> {
  const operation = Object.prototype.hasOwnProperty.call(operations, request.op) ? operations[request.op] : null;
  if (!operation) throw new Error('Unknown evaluator operation: ' + request.op);
  return operation(request);
}

/** Answers requests on stdin until it closes. Requests run concurrently. */
export function serve(): void {
  const write = process.stdout.write.bind(process.stdout);
  // Project code that logs must not corrupt the answers.
  process.stdout.write = process.stderr.write.bind(process.stderr) as typeof process.stdout.write;
  const lines = createInterface({ input: process.stdin });
  lines.on('line', line => {
    let request: Request;
    try { request = JSON.parse(line); } catch (error) {
      write(JSON.stringify({ id: null, ok: false, error: 'Invalid evaluator request: ' + String((error as Error).message) }) + '\n');
      return;
    }
    answer(request).then(
      value => write(JSON.stringify({ id: request.id, ok: true, value }) + '\n'),
      error => write(JSON.stringify({ id: request.id, ok: false, error: String((error && error.message) || error) }) + '\n'),
    );
  });
  lines.on('close', () => process.exit(0));
}
