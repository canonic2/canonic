// Lab-only asset acquisition from exact package pins and the installed VS Code.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const root = path.resolve(process.argv[2]);
const app = process.argv[3] || '/Applications/Visual Studio Code.app/Contents/Resources/app';
const assets = path.join(root, 'assets');
fs.mkdirSync(assets, { recursive: true });
const hash = (value) => crypto.createHash('sha256').update(value).digest('hex');
for (const [name, from] of Object.entries({
  hljs: 'node_modules/@highlightjs/cdn-assets/es',
  textmate: 'node_modules/vscode-textmate/release',
  onig: 'node_modules/vscode-oniguruma/release',
}))
  fs.cpSync(path.join(root, from), path.join(assets, name), { recursive: true });
const scopes = {};
const grammars = [];
for (const extension of fs.readdirSync(path.join(app, 'extensions'))) {
  const packageFile = path.join(app, 'extensions', extension, 'package.json');
  if (!fs.existsSync(packageFile)) continue;
  const manifest = JSON.parse(fs.readFileSync(packageFile));
  for (const grammar of manifest.contributes?.grammars || []) {
    if (!grammar.path || !grammar.scopeName) continue;
    const file = path.join(app, 'extensions', extension, grammar.path);
    const bytes = fs.readFileSync(file);
    const name = `${extension}-${path.basename(file)}`;
    fs.mkdirSync(path.join(assets, 'grammars'), { recursive: true });
    fs.writeFileSync(path.join(assets, 'grammars', name), bytes);
    scopes[grammar.scopeName] = name;
    grammars.push({
      scope: grammar.scopeName,
      file: name,
      sha256: hash(bytes),
      injectTo: grammar.injectTo || [],
    });
  }
}
fs.writeFileSync(path.join(assets, 'grammars/index.json'), JSON.stringify(scopes));
const highlightSource = fs.readFileSync(
  path.resolve(__dirname, '../../server/src/docs/highlight.rs'),
  'utf8',
);
const rules = [
  ...highlightSource
    .split('const CLASSES:')[1]
    .split('];')[0]
    .matchAll(/\("([^"]*)", "([^"]*)"\)/g),
].map((match) => [match[1], match[2]]);
fs.writeFileSync(path.join(assets, 'roles.json'), JSON.stringify(rules));
const font = path.resolve(
  __dirname,
  '../../server/target/doc/static.files/SourceCodePro-Regular-8badfe75.ttf.woff2',
);
fs.copyFileSync(font, path.join(assets, 'acme-mono.woff2'));
const record = {
  vscode: JSON.parse(fs.readFileSync(path.join(app, 'package.json'))).version,
  grammars,
  font: { name: path.basename(font), sha256: hash(fs.readFileSync(font)) },
};
fs.writeFileSync(path.join(root, 'assets.json'), JSON.stringify(record, null, 2));
const pin = path.join(__dirname, 'dependencies/assets.json');
if (fs.existsSync(pin)) {
  if (fs.readFileSync(pin, 'utf8') !== JSON.stringify(record, null, 2) + '\n')
    throw Error('Installed grammar/font hashes do not match the lab pins');
} else {
  fs.writeFileSync(pin, JSON.stringify(record, null, 2) + '\n');
}
console.log(JSON.stringify({ vscode: record.vscode, grammars: grammars.length, root }));
