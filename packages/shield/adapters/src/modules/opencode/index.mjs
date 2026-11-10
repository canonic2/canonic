// Shield's owned OpenCode V1 plugin. Tool execution remains with the host.
import { transport } from '../process-transport/index.mjs';
import { parseJSON } from '../enforcement-response/json.mjs';
import { isAbsolute } from 'node:path';

const VERSION = 1;
const HARMLESS = new Set(['todowrite', 'todoread']);
const text = (value, limit) => typeof value === 'string' && value.length <= limit && !value.includes('\0');
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);

function reply(bytes, exit, expected) {
  const value = parseJSON(bytes);
  // An empty disposition retains the host's permissions for unmodeled calls.
  if (exit === 0 && object(value) && Object.keys(value).length === 0
    && expected.event === 'tool.execute.before') return null;
  if (exit !== 0 || !object(value) || Object.keys(value).length !== 4
    || !['protocol_version','event','decision','reason'].every(key => Object.hasOwn(value,key))
    || value.protocol_version !== VERSION || value.event !== expected.event
    || !text(value.reason,65_536)
    || !(expected.event === 'tool.execute.before' ? ['allow','ask','deny'] : ['observed']).includes(value.decision)) throw Error('Invalid Shield plugin result');
  return value;
}

function immutable(value) {
  const pending = [value];
  while (pending.length) {
    const current = pending.pop();
    if (current === null || typeof current !== 'object') continue;
    for (const child of Object.values(current)) pending.push(child);
    Object.freeze(current);
  }
  return value;
}

export default async function shieldPlugin(context) {
  if (!object(context) || !text(context.directory,4096) || !isAbsolute(context.directory)) throw Error('Shield requires an absolute OpenCode instance directory');
  const directory = context.directory;
  const request = async (event,input,args) => {
    if (!object(input) || !text(input.tool,256) || !text(input.sessionID,4096) || !input.sessionID
      || !text(input.callID,4096) || !input.callID || !object(args)) throw Error('Invalid OpenCode tool context');
    // Config callbacks are mutable and do not prove the host's eventual shell
    // launch. Bash/Zsh also source startup state before evaluating the source.
    const envelope = {protocol_version:VERSION,hook_event_name:event,cwd:directory,tool:input.tool,sessionID:input.sessionID,callID:input.callID,arguments:args,shell:null};
    const bytes = Buffer.from(JSON.stringify(envelope));
    if (bytes.length > 8*1024*1024) throw Error('OpenCode tool input exceeds Shield limit');
    const captured = parseJSON(bytes);
    const result = await transport({binary:'shield',args:['__internal','native-hook','--project',directory,'--agent','opencode','--event',event],timeoutMs:10_000,input:bytes,expected:{event}},reply);
    return {result,args:captured.arguments};
  };
  return {
    'tool.execute.before': async (input,output) => {
      if (HARMLESS.has(input?.tool)) return;
      let captured;
      try { captured = await request('tool.execute.before',input,output?.args); }
      catch { throw Error('Shield could not verify this OpenCode request. Diagnose the project before retrying.'); }
      const {result,args} = captured;
      if (result === null) return;
      if (result.decision !== 'allow') throw Error(result.reason || 'Shield withheld this request; native approval is unavailable.');
      // Later host plugins share this output object. Bind execution to the
      // evaluated snapshot; subsequent transforms must throw or leave it intact.
      Object.defineProperty(output,'args',{value:immutable(args),enumerable:true,writable:false,configurable:false});
    },
    'tool.execute.after': async input => {
      if (HARMLESS.has(input?.tool)) return;
      // Optional observation failure cannot change or pretend to undo the tool
      // result. The core retains missing completion/correlation evidence.
      try { await request('tool.execute.after',input,input?.args); }
      catch { console.error('Shield completion history unavailable; outcome remains unverified.'); }
    },
  };
}
