#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const { Compiler } = require('./compiler.cjs');

async function main() {
  const command = process.argv[2];
  const root = path.resolve(process.argv[3] || '.');
  if (command === 'init') {
    fs.mkdirSync(root, { recursive: true });
    const declarations = fs.readFileSync(path.join(__dirname, 'api.d.ts'), 'utf8');
    const types = path.join(root, 'workbench-env.d.ts');
    if (!fs.existsSync(types)) fs.writeFileSync(types, '// Workbench authoring types. No runtime package installation is required.\n' +
      'declare module "@canonic2/workbench" {\n' + declarations.split('\n').map(line => '  ' + line).join('\n') + '\n}\n');
    const yaml = path.join(root, 'workbench.yaml');
    if (!fs.existsSync(yaml)) fs.writeFileSync(yaml, 'name: ' + JSON.stringify(path.basename(root)) + '\n');
    console.log('Workbench authoring types and manifest ready in ' + root);
    return;
  }
  if (command === 'check' || command === 'build') {
    const configured = require('../config').read(root);
    if (configured && configured.previews === false) { console.log('Workbench previews are disabled'); return; }
    const compiler = new Compiler(root, configured && configured.previews || {});
    if (command === 'build') {
      const destination = path.resolve(process.argv[4] || path.join(root, 'workbench-static'));
      if (fs.existsSync(destination) && fs.readdirSync(destination).length) throw new Error('Output directory must be empty: ' + destination);
      const result = await require('./portable.cjs').create(compiler, { name: configured && configured.name });
      fs.mkdirSync(destination, { recursive: true });
      for (const file of result.files) {
        const target = path.join(destination, file.path.replace(/^browser\//, ''));
        fs.mkdirSync(path.dirname(target), { recursive: true });
        fs.writeFileSync(target, Buffer.from(file.data, 'base64'));
      }
      console.log('Built ' + result.previews.length + ' previews in ' + destination);
      if (result.warnings.length) { console.error(result.warnings.join('\n')); process.exitCode = 1; }
      return;
    }
    const index = await compiler.index();
    const failures = index.errors.slice();
    for (const preview of index.previews) {
      try { await compiler.compile(preview.file, false); console.log('Built ' + preview.id); }
      catch (error) { failures.push(preview.file + ': ' + error.message); }
    }
    if (failures.length) { console.error(failures.join('\n')); process.exitCode = 1; }
    return;
  }
  throw new Error('Usage: node preview/cli.cjs <init|check|build> <project> [build-output]');
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
