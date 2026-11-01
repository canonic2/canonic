// Scripted MCP peer for host schema compatibility tests; all data is fixture-owned.
import { createInterface } from 'node:readline';
import { readFile, writeFile } from 'node:fs/promises';

const [questionFile, traceFile] = process.argv.slice(2);
const question = JSON.parse(await readFile(questionFile, 'utf8'));
const trace = [];
let waiting;
const send = message => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`);
for await (const line of createInterface({ input: process.stdin })) {
  const message = JSON.parse(line);
  trace.push(message);
  await writeFile(traceFile, JSON.stringify(trace));
  if (message.method === 'initialize') {
    send({ id: message.id, result: { protocolVersion: '2025-11-25', capabilities: { tools: {} }, serverInfo: { name: 'acme-forms', version: '1' } } });
  } else if (message.method === 'tools/list') {
    send({ id: message.id, result: { tools: [{ name: 'acme_form', description: 'Choose Acme settings', inputSchema: { type: 'object', properties: {}, additionalProperties: false } }] } });
  } else if (message.method === 'tools/call') {
    waiting = message.id;
    send({ id: 'acme-input', method: 'elicitation/create', params: question });
  } else if (message.id === 'acme-input') {
    send({ id: waiting, result: { content: [{ type: 'text', text: 'Acme form completed.' }] } });
  } else if (message.method === 'ping') {
    send({ id: message.id, result: {} });
  }
}
