// Acquire only the two digest-pinned development assets used by real React tests.
import {mkdir,readFile,writeFile,lstat} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
const directory=process.argv[2];
if(!directory)throw Error('Select a private React fixture directory');
await mkdir(directory,{recursive:true,mode:0o700});
if(!(await lstat(directory)).isDirectory())throw Error('React fixture root must be a directory, not a symlink');
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
for(const [name,url,digest] of [
  ['testrun-react-18.3.1.js','https://unpkg.com/react@18.3.1/umd/react.development.js','28348fef6cb0ed8b2ceeb22deaf824428fd13875d84c73d38f77dd216fc24e7f'],
  ['testrun-react-dom-18.3.1.js','https://unpkg.com/react-dom@18.3.1/umd/react-dom.development.js','f9044a5e9c39db8bb1a204dff924e526ec0a621e695bb69de1035811be8709e4'],
]){
  const path=resolve(directory,name);
  try{
    const metadata=await lstat(path);
    if(!metadata.isFile()||metadata.size>2*1024*1024||sha(await readFile(path))!==digest)throw Error(`Modified React fixture: ${name}`);
    process.stdout.write(`Verified ${name}\n`);continue;
  }catch(error){if(error.code!=='ENOENT')throw error;}
  const response=await fetch(url,{signal:AbortSignal.timeout(30000)});
  if(!response.ok)throw Error(`React fixture download failed: ${response.status}`);
  const chunks=[];let size=0;
  for await(const chunk of response.body){size+=chunk.length;if(size>2*1024*1024)throw Error('React fixture download exceeds its bound');chunks.push(chunk);}
  const bytes=Buffer.concat(chunks);
  if(sha(bytes)!==digest)throw Error(`React fixture digest changed: ${name}`);
  await writeFile(path,bytes,{flag:'wx',mode:0o600});process.stdout.write(`Acquired ${name}\n`);
}
