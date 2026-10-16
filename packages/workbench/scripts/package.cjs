var cp = require('node:child_process');
var fs = require('node:fs');
var path = require('node:path');
var args = process.argv.slice(2);
var at = args.indexOf('--target');
var target = at < 0 ? process.platform + '-' + process.arch : args[at + 1];
if (at < 0) args.push('--target', target);
require('./build-server.cjs').build(target);
require('./bundle-compiler.cjs').bundle(target).then(function () {
  return require('./bundle-runtime.cjs').bundle(target);
}).then(function () {
  var vsce = path.join(__dirname, 'vsce.cjs');
  var stage = require('./stage-extension.cjs').stageExtension(path.resolve(__dirname, '..'));
  var output = args.indexOf('--out');
  if (output >= 0) args[output + 1] = path.resolve(args[output + 1]);
  else args.push('--out', path.resolve(__dirname, '..', 'canonic-workbench-' + target + '-' + require('../extension/package.json').version + '.vsix'));
  var child = cp.spawn(process.execPath, [vsce, 'package', '--allow-missing-repository', '--no-rewrite-relative-links'].concat(args), {
    cwd: stage, stdio: 'inherit',
  });
  child.on('error', function (error) { fs.rmSync(stage, { recursive: true, force: true }); console.error(error); process.exitCode = 1; });
  child.on('exit', function (code) { fs.rmSync(stage, { recursive: true, force: true }); process.exitCode = code === null ? 1 : code; });
}).catch(function (error) { console.error(error); process.exitCode = 1; });
