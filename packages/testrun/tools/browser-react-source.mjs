// Copy the reviewed upstream React DevTools hook without transforming it.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const source=process.argv[2];
if(!source)throw Error('Supply the pinned agent-browser React installHook.js');
const bytes=await readFile(source);
const sha256=createHash('sha256').update(bytes).digest('hex');
if(sha256!=='0f76517ae59068da5aa0e80dce8eb5ed1ced20173489a7830276afc2e3e5c581')throw Error('React hook differs from the reviewed pin');
const base=new URL('../',import.meta.url);
async function save(path,content){
  const file=new URL(path,base);
  try{if(Buffer.compare(await readFile(file),Buffer.from(content))===0)return;}catch(error){if(error.code!=='ENOENT')throw error;}
  await writeFile(file,content);
}
await mkdir(new URL('third-party/browser/react-notices/',base),{recursive:true});
const notice=bytes.toString('utf8').match(/^\/\*![\s\S]*?\*\//)?.[0];
if(!notice?.includes('MIT license'))throw Error('React license notice missing');
await save('src/browser/reading/react-hook.js',bytes);
await save('src/browser/reading/react-hook-source.json',JSON.stringify({repository:'vercel-labs/agent-browser',commit:'04ba5264dade678062c3cdcce0f4f293e748249f',path:'cli/src/native/react/installHook.js',bytes:bytes.length,sha256,license:'MIT',upstream:'facebook/react'},null,2)+'\n');
await save('third-party/browser/react-notices/LICENSE',notice+'\n');
process.stdout.write(`Pinned React hook: ${bytes.length} bytes (${fileURLToPath(base)}).\n`);
