/* Builds one platform VSIX per capture target into dist/. Targets run one at
   a time because each bundle replaces capture-runtime/. The build host's own
   target goes last, so the local runtime is left usable afterwards.

   node scripts/package-all.cjs [target ...] [--out <dir>] */
var cp = require('node:child_process');
var fs = require('node:fs');
var path = require('node:path');
var targets = require('./bundle-capture.cjs').targets;
var ROOT = path.resolve(__dirname, '..');
var VERSION = require('../package.json').version;

function plan(argv, host) {
  var args = argv.slice();
  var out = path.join(ROOT, 'dist');
  var at = args.indexOf('--out');
  if (at >= 0) { out = path.resolve(args[at + 1] || ''); args.splice(at, 2); }
  var unknown = args.filter(function (t) { return !targets.includes(t); });
  if (unknown.length) throw new Error('Unsupported target: ' + unknown.join(', ') + ' (supported: ' + targets.join(', ') + ')');
  var chosen = args.length ? targets.filter(function (t) { return args.includes(t); }) : targets.slice();
  chosen.sort(function (a, b) { return (a === host) - (b === host); });
  return { out: out, targets: chosen };
}

function build(target, out) {
  var file = path.join(out, 'canonic-workbench-' + target + '-' + VERSION + '.vsix');
  var result = cp.spawnSync(process.execPath, [path.join(__dirname, 'package.cjs'), '--target', target, '--out', file], {
    cwd: ROOT, stdio: 'inherit',
  });
  return result.status === 0 ? file : null;
}

function main() {
  var p = plan(process.argv.slice(2), process.platform + '-' + process.arch);
  fs.mkdirSync(p.out, { recursive: true });
  var built = [], failed = [];
  p.targets.forEach(function (target) {
    console.log('\n== ' + target);
    var file = build(target, p.out);
    if (file) built.push(file); else failed.push(target);
  });
  console.log('');
  built.forEach(function (file) {
    console.log('built  ' + path.relative(process.cwd(), file) + ' (' + (fs.statSync(file).size / 1048576).toFixed(1) + ' MB)');
  });
  failed.forEach(function (target) { console.log('FAILED ' + target); });
  if (failed.length) process.exitCode = 1;
}

if (require.main === module) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
module.exports = { plan: plan };
