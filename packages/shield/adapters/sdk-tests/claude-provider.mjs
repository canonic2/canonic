import { createServer } from 'node:http';

/** Owned local provider script; no external model, token or network response is used. */
export async function scriptedProvider(t, steps) {
  const results = [];
  const requests = [];
  let count = 0;
  const server = createServer(async (request, response) => {
    let bytes = '';
    for await (const chunk of request) bytes += chunk;
    const body = JSON.parse(bytes || '{}');
    if (request.url.includes('count_tokens')) {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ input_tokens: 10 }));
      return;
    }
    requests.push(body);
    for (const message of body.messages ?? []) for (const block of Array.isArray(message.content) ? message.content : []) {
      if (block.type === 'tool_result') results.push(block);
    }
    const step = body.tools?.length ? steps[count++] : undefined;
    await step?.before?.();
    const input = typeof step?.input === 'function' ? await step.input(body) : step?.input;
    const model = body.model ?? 'acme-model';
    const content = step ? [{ type: 'tool_use', id: `toolu_acme_${count}`, name: step.name, input }]
      : [{ type: 'text', text: 'Acme fixture finished.' }];
    const message = { id: `msg_acme_${count}`, type: 'message', role: 'assistant', model, content,
      stop_reason: step ? 'tool_use' : 'end_turn', stop_sequence: null,
      usage: { input_tokens: 10, output_tokens: 10 } };
    if (!body.stream) {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify(message));
      return;
    }
    response.writeHead(200, { 'content-type': 'text/event-stream' });
    const event = (type, value) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...value })}\n\n`);
    event('message_start', { message: { ...message, content: [], stop_reason: null, usage: { input_tokens: 10, output_tokens: 0 } } });
    for (let index = 0; index < content.length; index++) {
      const block = content[index];
      event('content_block_start', { index, content_block: block.type === 'tool_use' ? { ...block, input: {} } : { type: 'text', text: '' } });
      event('content_block_delta', { index, delta: block.type === 'tool_use'
        ? { type: 'input_json_delta', partial_json: JSON.stringify(block.input) }
        : { type: 'text_delta', text: block.text } });
      event('content_block_stop', { index });
    }
    event('message_delta', { delta: { stop_reason: message.stop_reason, stop_sequence: null }, usage: { output_tokens: 10 } });
    event('message_stop', {});
    response.end();
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  t.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  return { url: `http://127.0.0.1:${server.address().port}`, results, requests };
}
