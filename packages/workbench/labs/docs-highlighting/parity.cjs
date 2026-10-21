const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { createRequire } = require('node:module');

// Normal coloured captures and actual computed palettes are checked separately.
// The neutral browser render preserves span structure and typography, allowing
// every glyph pixel to be compared without confusing lexical colours with layout.
async function compareGroup(runRoot, samples) {
  const control = samples.find(sample => sample.variant === '0' && sample.status === 'passed');
  if (!control) return { unavailable: 'No successful control sample' };
  const dependencies = path.resolve(runRoot, '../node_modules');
  const requireTool=createRequire(fs.realpathSync(path.join(dependencies,'pixelmatch/package.json')));
  const { PNG } = requireTool('pngjs');
  const { default: pixelmatch } = await import(pathToFileURL(path.join(dependencies, 'pixelmatch/index.js')).href);
  const image = sample => PNG.sync.read(fs.readFileSync(path.join(runRoot,
    `${sample.index}-${sample.variant}-${sample.shape}-${sample.cache === 'cold-startup' ? 'startup' : 'settled'}`,
    'settled-neutral.png')));
  const a = image(control);
  for (const sample of samples.filter(sample => sample.status === 'passed')) {
    const b = image(sample);
    const before = control.visual.codeBoxes, after = sample.visual.codeBoxes;
    if (a.width !== b.width || a.height !== b.height || before.length !== after.length) {
      sample.checks.renderParity = { pass: false, reason: 'Raster or code-box dimensions differ' };
      continue;
    }
    const geometry = before.every((box,index) => ['x','y','width','height'].every(key => Math.abs(box[key]-after[index][key])<.01) && box.background === after[index].background);
    const code = new Uint8Array(a.width*a.height);
    for (const box of before) for (let y=Math.max(0,Math.floor(box.y));y<Math.min(a.height,Math.ceil(box.y+box.height));y++)
      for (let x=Math.max(0,Math.floor(box.x));x<Math.min(a.width,Math.ceil(box.x+box.width));x++) code[y*a.width+x]=1;
    const substantive = Buffer.alloc(a.data.length);
    pixelmatch(a.data,b.data,substantive,a.width,a.height,{threshold:0,includeAA:false,diffMask:true});
    let outsideDifferent=0,codeDifferent=0,codePixels=0,outsidePixels=0,antialiasedCode=0;
    const differences=[];
    for(let y=0;y<a.height;y++) for(let x=0;x<a.width-16;x++) {
      const pixel=y*a.width+x,at=pixel*4;
      if(code[pixel])codePixels++;else outsidePixels++;
      if(![0,1,2].some(channel=>Math.abs(a.data[at+channel]-b.data[at+channel])>2))continue;
      if(code[pixel]&&!substantive[at+3]){antialiasedCode++;continue;}
      if(code[pixel])codeDifferent++;else outsideDifferent++;
      if(differences.length<10)differences.push({x,y,kind:code[pixel]?'code':'outside-code',a:[...a.data.slice(at,at+3)],b:[...b.data.slice(at,at+3)]});
    }
    sample.checks.renderParity={pass:geometry&&!outsideDifferent&&!codeDifferent,geometry,outsideDifferent,codeDifferent,codePixels,outsidePixels,antialiasedCode,differences,rgbChannelTolerance:2,antialiasDetector:'pixelmatch 7.1.0; threshold 0; includeAA false; applied only inside code',normalized:'Syntax span foreground colour only; normal colours checked separately',excluded:'Rightmost 16 viewport pixels: native scrollbar raster'};
  }
  for (const sample of samples) if (sample.correctness)
    sample.correctness.eligible = sample.correctness.execution && sample.checks.tokenRoles === true && sample.checks.renderParity?.pass === true;
  return {checked:samples.filter(sample=>sample.checks.renderParity).length,failures:samples.filter(sample=>sample.checks.renderParity?.pass===false).map(sample=>({variant:sample.variant,...sample.checks.renderParity}))};
}
module.exports = { compareGroup };
