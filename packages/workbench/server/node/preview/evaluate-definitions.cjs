/* Process adapter for the native index. Stdout is exclusively the response;
   project output belongs on stderr. Evaluation requires workspace trust. */
const fs = require('node:fs');
const path = require('node:path');
const { Compiler, projectDefinition } = require('./compiler.cjs');

async function evaluate(request) {
  const { root, files, options = {} } = request;
  if (request.action === 'select') {
    const matches = require('picomatch')(options.include);
    return files.filter(file => matches(file));
  }
  const compiler = new Compiler(root, options);
  await compiler.settings();
  const absolute = files.map(file => path.resolve(root, file));
  await compiler.load(absolute);
  const entries = [];
  for (const file of absolute) {
    let evaluated;
    try {
      evaluated = await compiler.definition(file);
      const projected = projectDefinition(evaluated.value, file, root);
      const dependencies = new Set([file, ...evaluated.files, ...compiler.configFiles.keys()]);
      // Validation reads these paths in addition to esbuild's input graph.
      // Missing candidates are watched too, so adding one recovers a definition.
      function local(value) { if (typeof value === 'string') dependencies.add(path.resolve(path.dirname(file), value)); }
      if (projected.docs) {
        local(evaluated.value.docs);
        for (const lens of Object.values(evaluated.value.lenses || {})) {
          local(lens.examples); local(lens.environment);
          for (const style of lens.styles || []) local(style);
        }
      } else {
        local(evaluated.value.source?.entry);
        for (const scenario of Object.values(evaluated.value.scenarios || {})) local(scenario?.source?.entry);
      }
      entries.push({ file: path.relative(root, file).split(path.sep).join('/'), ...projected,
        files: [...dependencies].map(file => path.relative(root, file)), cacheable: true });
    } catch (error) {
      // A failed build can hide an unresolved dependency. Retry failures on
      // the next request rather than keeping an error that cannot invalidate.
      entries.push({ file: path.relative(root, file).split(path.sep).join('/'), docs: false,
        error: error.message, files: [], cacheable: false });
    }
  }
  return { entries, settings: [...compiler.configFiles.keys()].map(file => path.relative(root, file)) };
}

if (require.main === module) {
  const input = fs.readFileSync(0, 'utf8');
  const original = process.stdout.write.bind(process.stdout);
  process.stdout.write = process.stderr.write.bind(process.stderr);
  evaluate(JSON.parse(input)).then(answer => original(JSON.stringify(answer)), error => {
    console.error(error.message); process.exitCode = 1;
  });
}
module.exports = { evaluate };
