// Transparent fixture stdio observer. Record protocol shape, never arguments/tokens.
import { spawn } from 'node:child_process';
import { appendFileSync } from 'node:fs';
const child = spawn(process.argv[2], process.argv.slice(3), { stdio: ['pipe', 'pipe', 'pipe'] });
function observe(stream, destination, direction) {
  let pending = '';
  stream.on('data', bytes => {
    pending += bytes;
    let index;
    while ((index = pending.indexOf('\n')) !== -1) {
      const line = pending.slice(0, index);
      pending = pending.slice(index + 1);
      let message;
      try { message = JSON.parse(line); }
      catch { destination.write(line + '\n'); continue; }
      if (direction === 'client' && process.env.ACME_MCP_CLASSIC_ONLY === '1' && message.method === 'server/discover') {
        process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:message.id,error:{code:-32601,message:'Acme fixture selects classic MCP.'}})+'\n');
        continue;
      }
      destination.write(line + '\n');
      appendFileSync(process.env.ACME_MCP_TRACE, JSON.stringify({ direction,
        method: message.method, error: message.error,
        resultKeys: message.result && Object.keys(message.result),
        tools: message.result?.tools?.map(tool => tool.name),
        clientCapabilities: message.params?.capabilities,
        inputActions: message.params?.inputResponses && Object.fromEntries(Object.entries(message.params.inputResponses).map(([key, value]) => [key, value.action])),
        responseAction: message.result?.action,
        errorCode: message.result?.structuredContent?.error?.code,
      }) + '\n');
    }
  });
}
observe(process.stdin, child.stdin, 'client');
observe(child.stdout, process.stdout, 'server');
child.stderr.pipe(process.stderr);
process.stdin.on('end', () => child.stdin.end());
child.on('exit', code => process.exit(code ?? 1));
process.on('SIGTERM', () => child.kill('SIGTERM'));
