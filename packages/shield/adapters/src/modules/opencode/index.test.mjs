import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, chmod, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import plugin from './index.mjs';

test('OpenCode V1 verifies result version, exact fields, exit and Deny before host execution', {skip:process.platform==='win32'}, async()=>{
  const root=await mkdtemp(join(tmpdir(),'shield-opencode-plugin-'));
  const previousPath=process.env.PATH;
  try {
    const binary=join(root,'shield');
    process.env.PATH=`${root}:${previousPath}`;
    const hooks=await plugin({directory:root});
    const input={tool:'read',sessionID:'fixture-session',callID:'fixture-call'};
    const args={filePath:join(root,'fixture.txt')};
    const fixture=async(value,exit=0)=>{await writeFile(binary,`#!/usr/bin/env node\nprocess.stdin.resume();process.stdin.on('end',()=>{process.stdout.write(${JSON.stringify(value)});process.exitCode=${exit};});\n`);await chmod(binary,0o755);};
    const allow={protocol_version:1,event:'tool.execute.before',decision:'allow',reason:''};
    await fixture(JSON.stringify(allow));
    const original={...args,options:{paths:['fixture']}};
    const output={args:original,unrelated:'preserved'};
    await hooks['tool.execute.before'](input,output);
    assert.notEqual(output.args,original);
    assert.deepEqual(output.args,original);
    original.options.paths[0]='changed-after-evaluation';
    assert.equal(output.args.options.paths[0],'fixture');
    const laterNestedPlugin=async()=>{output.args.options.paths[0]='unsafe';};
    const laterReplacementPlugin=async()=>{output.args={filePath:'/unsafe'};};
    await assert.rejects(laterNestedPlugin(),TypeError);
    await assert.rejects(laterReplacementPlugin(),TypeError);
    assert.throws(()=>Object.defineProperty(output,'args',{value:original}),TypeError);
    output.unrelated='still editable';
    assert.equal(output.unrelated,'still editable');
    assert.equal(output.args.filePath,args.filePath);
    for(const value of [{...allow,protocol_version:2},{...allow,event:'tool.execute.after'},{...allow,extra:true},{...allow,decision:'deny',reason:'Fixture refusal'},{...allow,decision:'ask',reason:'Native Ask unavailable'}]) {
      await fixture(JSON.stringify(value));
      await assert.rejects(hooks['tool.execute.before'](input,{args}));
    }
    await fixture(JSON.stringify(allow),2);
    await assert.rejects(hooks['tool.execute.before'](input,{args}));
    await fixture(JSON.stringify({...allow,decision:'deny',reason:'Unsupported tool'}));
    for (const tool of ['list','multiedit','future_effectful_tool']) {
      await assert.rejects(hooks['tool.execute.before']({...input,tool},{args}),/Unsupported tool/);
    }
    await fixture('{"protocol_version":1,"protocol_version":1,"event":"tool.execute.before","decision":"allow","reason":""}');
    await assert.rejects(hooks['tool.execute.before'](input,{args}));
    await fixture('{}');
    const unmodeledArgs={command:'git status',description:'Inspect status'};
    const unmodeledOutput={args:unmodeledArgs};
    await hooks['tool.execute.before']({...input,tool:'bash'},unmodeledOutput);
    assert.equal(unmodeledOutput.args,unmodeledArgs);
    unmodeledOutput.args={command:'host still owns permission'};
    await assert.rejects(hooks['tool.execute.before']({...input,callID:''},{args}));
  } finally { if(previousPath===undefined)delete process.env.PATH;else process.env.PATH=previousPath;await rm(root,{recursive:true,force:true}); }
});
