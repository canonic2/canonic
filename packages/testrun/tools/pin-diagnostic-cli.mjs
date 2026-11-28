// Ask Cargo for its actual artifact; honor configured/shared target directories.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {link,readFile} from 'node:fs/promises';
import {isAbsolute,resolve} from 'node:path';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';

export async function pinDiagnosticCLI(cargo,destination,cwd=resolve(import.meta.dirname,'..')){
  assert.ok(isAbsolute(cargo)&&isAbsolute(destination),'Select absolute Cargo and new CLI destination');
  const child=spawn(cargo,['build','--locked','--message-format=json'],{cwd,stdio:['ignore','pipe','inherit']});
  const exited=new Promise((accept,reject)=>{child.once('error',reject);child.once('exit',(code,signal)=>code===0?accept():reject(Error(`CLI build failed (${code??signal})`)));});
  let executable,artifactError,pinned=false;
  for await(const line of createInterface({input:child.stdout})){
    try{
      assert.ok(line.length<8388608,'Cargo message bound');const message=JSON.parse(line);
      if(message.reason==='compiler-message'&&message.message?.rendered)process.stderr.write(message.message.rendered);
      if(message.reason==='compiler-artifact'&&message.target?.name==='testrun'&&message.target.kind.includes('bin')&&message.executable){
        assert.ok(isAbsolute(message.executable));assert.equal(executable,undefined,'Exactly one selected Testrun executable');
        executable=message.executable;
        // Pin as soon as Cargo publishes the artifact, while this build still
        // owns its output. Later builds may replace the reported filename.
        await link(executable,destination);pinned=true;
      }
    }catch(error){artifactError??=error;}
  }
  await exited;if(artifactError)throw artifactError;assert.ok(pinned,'Cargo must report the Testrun executable');
  return {source:executable,pinned:destination,sha256:createHash('sha256').update(await readFile(destination)).digest('hex')};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))console.log(JSON.stringify(await pinDiagnosticCLI(...process.argv.slice(2))));
