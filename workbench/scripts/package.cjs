var cp = require('node:child_process');
var path = require('node:path');
var args = process.argv.slice(2);
var at = args.indexOf('--target');
var target = at < 0 ? process.platform + '-' + process.arch : args[at + 1];
if (at < 0) args.push('--target', target);
require('./bundle-compiler.cjs').bundle(target).then(function () {
  return require('./bundle-runtime.cjs').bundle(target);
}).then(function () {
  var vsce = require.resolve('@vscode/vsce/vsce');
  var child = cp.spawn(process.execPath, [vsce, 'package', '--allow-missing-repository', '--no-rewrite-relative-links'].concat(args), {
    cwd: path.resolve(__dirname, '..'), stdio: 'inherit',
  });
  child.on('error', function (error) { console.error(error); process.exitCode = 1; });
  child.on('exit', function (code) { process.exitCode = code === null ? 1 : code; });
}).catch(function (error) { console.error(error); process.exitCode = 1; });
