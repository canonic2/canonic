import test from 'node:test';
import assert from 'node:assert/strict';
import {chmod,mkdir,mkdtemp,readFile,rename,rm,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pinDiagnosticCLI} from './pin-diagnostic-cli.mjs';

async function fixture(t,artifact=true){
  const root=await mkdtemp(join(tmpdir(),'testrun-cli-pin-'));
  t.after(()=>rm(root,{recursive:true,force:true}));
  await mkdir(join(root,'actual'));await mkdir(join(root,'target/debug'),{recursive:true});
  const source=join(root,'actual/testrun'),cargo=join(root,'cargo-peer.cjs');
  await writeFile(source,'actual Cargo artifact');await writeFile(join(root,'target/debug/testrun'),'stale assumed artifact');
  await writeFile(cargo,`#!${process.execPath}\nconst assert=require('node:assert/strict');assert.deepEqual(process.argv.slice(2),['build','--locked','--message-format=json']);console.log(JSON.stringify(${JSON.stringify({reason:'compiler-artifact',target:{name:'testrun',kind:[artifact?'bin':'lib']},executable:source})}));\n`);
  await chmod(cargo,0o700);return {root,source,cargo};
}
test('CLI pin follows Cargo artifact metadata and survives later output replacement',async t=>{
  const f=await fixture(t),destination=join(f.root,'pinned');
  const result=await pinDiagnosticCLI(f.cargo,destination,f.root);
  assert.equal(result.source,f.source);assert.equal(await readFile(destination,'utf8'),'actual Cargo artifact');
  await writeFile(join(f.root,'replacement'),'later build');await rename(join(f.root,'replacement'),f.source);
  assert.equal(await readFile(destination,'utf8'),'actual Cargo artifact');
  await assert.rejects(pinDiagnosticCLI(f.cargo,destination,f.root),{code:'EEXIST'});
  assert.equal(await readFile(destination,'utf8'),'actual Cargo artifact');
});
test('CLI pin refuses Cargo output without the selected executable artifact',async t=>{
  const f=await fixture(t,false);
  await assert.rejects(pinDiagnosticCLI(f.cargo,join(f.root,'pinned'),f.root),/must report the Testrun executable/);
});
