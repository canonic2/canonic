// Pinned SDK URL matcher, including repeated lastIndex resets; no browsers.
const assert=require('node:assert/strict');
const fs=require('node:fs'),crypto=require('node:crypto');
const bundle=fs.readFileSync(process.argv[2]+'/lib/coreBundle.js','utf8');
const source=name=>{
  const functions=[...bundle.matchAll(new RegExp('^function '+name+'\\([^]*?^\\}', 'gm'))];
  assert.equal(functions.length,1,`Pinned SDK function ${name} must remain unique`);
  return functions[0][0];
};
// Execute the exact pinned RegExp branch, retaining its real type helpers.
// Other matcher branches intentionally have no bindings in this reference.
const urlMatches=new Function('isString','isRegExp3',`return (${source('urlMatches')})`)(
  new Function(`return (${source('isString')})`)(),
  new Function(`return (${source('isRegExp3')})`)(),
);
const cases=[];
const add=(pattern,flags,path,expected)=>cases.push({pattern,flags,path,expected});
for(const flags of ['', 'u', 'gy', 'di']) {
  add('',flags,'/Acme/one',true);
  add('[]',flags,'/Acme/one',false);
  add('[]+|missing',flags,'/Acme/one',false);
  add('[]*Acme',flags.replace(/[gy]/g,''),'/Acme/one',true);
}
for(const [pattern,path,expected] of [
  [String.raw`\057Acme`,'/Acme/one',true],
  [String.raw`[\101]cme`,'/Acme/one',true],
  [String.raw`[\41-\132]cme`,'/Acme/one',true],
  [String.raw`[\8\9A]cme`,'/Acme/one',true],
  [String.raw`\012`,'/Acme/one',false],
  [String.raw`\08`,'/Acme/one',false],
  [String.raw`[\400]`,'/0',true],
  [String.raw`[\777]`,'/7',true],
]) add(pattern,'',path,expected);
// Decimal escape authority uses the complete pattern, including forward
// captures. Only non-backreference legacy forms are candidates for translation.
for(const [pattern,path,expected] of [
  [String.raw`\101cme`,'/Acme',true],
  [String.raw`\141cme`,'/acme',true],
  [String.raw`\57Acme`,'/Acme',true],
  [String.raw`\89`,'/89',true],
  [String.raw`(A)\8`,'/A8',true],
  [String.raw`(A)\80`,'/A80',true],
  [String.raw`(A)\101`,'/AA',true],
  [String.raw`\101(A)`,'/AA',true],
  [String.raw`\10189`,'/A89',true],
  [String.raw`\777`,'/Acme?7',true],
  [String.raw`\400`,'/0',false],
  [String.raw`\1`,'/Acme',false],
  [String.raw`[(]|\1`,'/Acme',false],
  [String.raw`\(\1`,'/Acme',false],
  [String.raw`(?:A)\1`,'/A',false],
  [String.raw`(A)\11`,'/AA',false],
  [String.raw`\k`,'/k',true],
  [String.raw`[\k]`,'/k',true],
  [String.raw`(A)\k`,'/Ak',true],
  [String.raw`(?:A)[\k]`,'/Ak',true],
]) add(pattern,'',path,expected);
const unsupported = [
  {pattern:String.raw`(Acme)\1`,flags:'',path:'/AcmeAcme',expected:true},
  {pattern:String.raw`\1(Acme)`,flags:'',path:'/Acme',expected:true},
  {pattern:String.raw`(?<n>Acme)\k<n>`,flags:'',path:'/AcmeAcme',expected:true},
  {pattern:String.raw`\k<n>(?<n>Acme)`,flags:'',path:'/Acme',expected:true},
  {pattern:String.raw`(A)(B)(C)(D)(E)(F)(G)(H)\8`,flags:'u',path:'/ABCDEFGHH',expected:true},
];
for(const row of [...cases,...unsupported]) {
  const regexp=new RegExp(row.pattern,row.flags);
  for(const scheme of ['http','https','ws','wss']) {
    const url=`${scheme}://example.com${row.path}`;
    regexp.lastIndex=99;
    const actual=urlMatches(undefined,url,regexp),repeat=urlMatches(undefined,url,regexp);
    assert.equal(actual,row.expected,JSON.stringify({row,url,actual}));
    assert.equal(repeat,row.expected,JSON.stringify({row,url,repeat}));
  }
}
const invalid=[];
for(const pattern of [String.raw`\012`,String.raw`\08`,String.raw`[\400]`,String.raw`[\777]`]) {
  assert.throws(()=>new RegExp(pattern,'u'),SyntaxError);invalid.push({pattern,flags:'u'});
}
for(const [pattern,flags] of [
  [String.raw`\1`,'u'],[String.raw`(A)\8`,'u'],
  [String.raw`\k`,'u'],[String.raw`[\k]`,'u'],
  [String.raw`(?<n>A)\k`,''],[String.raw`(?<n>A)\k<missing>`,''],
  [String.raw`(?<n>A)[\k]`,''],
  [String.raw`[\k](?<n>A)`,''],
]) {assert.throws(()=>new RegExp(pattern,flags),SyntaxError);invalid.push({pattern,flags});}
process.stdout.write(JSON.stringify({cases,unsupported,invalid,comparisons:(cases.length+unsupported.length)*8,bundleSha256:crypto.createHash('sha256').update(bundle).digest('hex')}));
