const fs = require('node:fs');
const path = require('node:path');
const product = path.resolve(__dirname, '../..');
function overlay(root, variant, assets, base = product, lab = __dirname) {
  fs.mkdirSync(root, { recursive: true });
  for (const name of ['LICENSE', 'THIRD_PARTY_LICENSES.md'])
    if (fs.existsSync(path.join(base, name)))
      fs.copyFileSync(path.join(base, name), path.join(root, name));
  for (const name of ['extension', 'ui', 'contracts', 'sdk', 'capture', 'server/node']) {
    fs.cpSync(path.join(base, name), path.join(root, name), {
      recursive: true,
      mode: fs.constants.COPYFILE_FICLONE,
      filter: (file) =>
        !/(\/node_modules|\/target|\/runtime|\/\.native-helper|\/\.cache)(\/|$)/.test(file),
    });
  }
  // Dependencies and compressed runtimes are read-only inputs, not measured caches.
  fs.symlinkSync(path.join(base, 'node_modules'), path.join(root, 'node_modules'));
  if (fs.existsSync(path.join(base, 'capture/runtime')))
    fs.symlinkSync(path.join(base, 'capture/runtime'), path.join(root, 'capture/runtime'));
  const target = process.platform + '-' + process.arch;
  fs.mkdirSync(path.join(root, 'server/bin', target), { recursive: true });
  fs.copyFileSync(
    path.join(base, 'server/bin', target, 'workbench-server'),
    path.join(root, 'server/bin', target, 'workbench-server'),
    fs.constants.COPYFILE_FICLONE,
  );
  if (variant === 'D') {
    fs.writeFileSync(
      path.join(root, 'ui/src/modules/docs/page/highlighting.js'),
      'export function createHighlighting(){return {fences(){},source(){},async settled(){},reset(){},dispose(){}};}\n',
    );
  } else if (variant !== '0') {
    const owned = path.join(root, 'ui/src/lab');
    fs.mkdirSync(owned, { recursive: true });
    for (const name of ['delivery.js', 'worker.js'])
      fs.copyFileSync(path.join(lab, name), path.join(owned, name));
    if (variant === '1' || variant === '2')
      fs.cpSync(assets, path.join(root, 'ui/legacy/lab-assets'), { recursive: true });
    const file = path.join(root, 'ui/src/modules/docs/page/highlighting.js');
    let source = fs.readFileSync(file, 'utf8');
    if (!source.includes('request = fetch') || !source.includes('fences(root, url) {'))
      throw Error('Shipping highlighter contract changed');
    source =
      `let __labDelivery;let __labLoad;let __labRoot;\nconst __lab={fences(root){__labRoot=root;__labDelivery?.fences(root);},async request(...args){__labLoad||=import('../../../lab/delivery.js').then(({localRequest})=>{__labDelivery=localRequest(${JSON.stringify(variant)});if(__labRoot)__labDelivery.fences(__labRoot);return __labDelivery;});return (await __labLoad).request(...args);}};\n` +
      source
        .replace('request = fetch', 'request = __lab.request')
        .replace('fences(root, url) {', 'fences(root, url) {\n      __lab.fences(root);');
    fs.writeFileSync(file, source);
  }
  return root;
}
module.exports = { overlay };
