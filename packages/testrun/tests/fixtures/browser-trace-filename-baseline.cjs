// Execute the unchanged pinned file-verification and save helpers in an owned root.
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const {fileURLToPath, pathToFileURL} = require('node:url');
const {createHash} = require('node:crypto');
const {stripTypeScriptTypes} = require('node:module');
const zlib = require('node:zlib');
const pins = {
  'McpContext.ts': '1cc543eeadae4c4a3994ae13b9bb0d100bd986f265ce7b47ae7dcdff39d2a76b',
  'files.ts': 'bb9abca8ea461a1a01b63b5c791decd0feadcc74428cebda86eb5a20da9ef725',
  'ToolHandler.ts': 'c0f7e80829b08c4ac51beee6d526f2bbe25f9a513222e80ef7b20969615bb157',
};
function between(source, start, end) {
  const from = source.indexOf(start);
  const to = source.indexOf(end, from + start.length);
  if (from < 0 || to < 0) throw new Error('Pinned Acme helper boundary changed');
  return source.slice(from, to);
}
(async () => {
  const [reference, root, input] = process.argv.slice(2);
  const sources = {};
  for (const [file, expected] of Object.entries(pins)) {
    const bytes = await fs.readFile(path.join(reference, file));
    if (createHash('sha256').update(bytes).digest('hex') !== expected) throw new Error('Pinned Acme source changed');
    sources[file] = bytes.toString('utf8');
  }
  const fileHelpers = sources['files.ts'].slice(sources['files.ts'].indexOf('export async function getTempFilePath')).replaceAll('export async function', 'async function');
  const {resolveCanonicalPath, getTempFilePath} = new Function('fs', 'os', 'path', stripTypeScriptTypes(fileHelpers) + '\nreturn {resolveCanonicalPath,getTempFilePath};')(fs, os, path);
  const methods = between(sources['McpContext.ts'], '  roots(): Root[] {', '  async newPage(') + between(sources['McpContext.ts'], '  async #writeFile(', '  storeTraceRecording(');
  const skeleton = 'class ReferenceContext { #roots; #config={resolved:{allowUnrestrictedPaths:false}}; constructor(root){this.#roots=[{uri:pathToFileURL(root).href,name:"Acme fixture"}];}\n' + methods + '\n};';
  const ReferenceContext = new Function('fs', 'os', 'path', 'fileURLToPath', 'pathToFileURL', 'resolveCanonicalPath', 'getTempFilePath', 'escapeForLog', stripTypeScriptTypes(skeleton) + '\nreturn ReferenceContext;')(fs, os, path, fileURLToPath, pathToFileURL, resolveCanonicalPath, getTempFilePath, value => value);
  const validation = between(sources['ToolHandler.ts'], 'async function validateAndResolvePathOrUrl(', 'function isLocalBrowser(');
  const validate = new Function('fileURLToPath', 'pathToFileURL', stripTypeScriptTypes(validation) + '\nreturn validateAndResolvePathOrUrl;')(fileURLToPath, pathToFileURL);
  const original = Buffer.from('{"traceEvents":[{"name":"Acme filename"}]}');
  const report = {};
  for (const item of JSON.parse(input)) {
    if (!/^[a-z0-9-]+$/.test(item.name)) throw new Error('Invalid Acme fixture case');
    const selected = path.join(root, item.name);
    await fs.mkdir(selected, {recursive:true, mode:0o700});
    const directory = await fs.realpath(selected);
    if (item.parentMode !== undefined) await fs.chmod(directory, item.parentMode);
    process.chdir(directory);
    const context = new ReferenceContext(directory);
    const resolved = await validate(item.path, context, false);
    const gzip = resolved.endsWith('.gz');
    const data = gzip ? zlib.gzipSync(original) : original;
    const {filename} = await context.saveFile(data, resolved, gzip ? '.gz' : '.json');
    const stored = await fs.readFile(filename);
    if (!(gzip ? zlib.gunzipSync(stored) : stored).equals(original)) throw new Error('Acme evidence changed');
    report[item.name] = {relative:path.relative(directory,filename), gzip, mode:(await fs.stat(filename)).mode & 0o777, parentMode:(await fs.stat(directory)).mode & 0o7777};
  }
  process.stdout.write(JSON.stringify(report));
})().catch(error => {console.error(error);process.exitCode=1;});
