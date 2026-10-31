// A bounded loopback Anthropic protocol fixture; it never calls an external model.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';

export async function scriptedProvider(lifetime, steps) {
  const results = [];
  let count = 0;
  let requests = 0;
  const server = createServer(async (request, response) => {
    const chunks = [];
    let bytes = 0;
    for await (const chunk of request) {
      bytes += chunk.length;
      assert.ok(bytes <= 1_000_000, 'bounded local provider request');
      chunks.push(chunk);
    }
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}');
    if (request.url.includes('count_tokens')) {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ input_tokens: 10 }));
      return;
    }
    assert.ok(++requests <= 16, 'bounded local provider conversation');
    for (const message of body.messages ?? []) {
      for (const block of Array.isArray(message.content) ? message.content : []) {
        if (block.type === 'tool_result') results.push(block);
      }
    }
    const step = body.tools?.length ? steps[count++] : undefined;
    const content = step
      ? [{ type: 'tool_use', id: `toolu_acme_${count}`, name: step.name, input: step.input }]
      : [{ type: 'text', text: 'Acme fixture finished.' }];
    const message = { id: `msg_acme_${count}`, type: 'message', role: 'assistant',
      model: body.model ?? 'acme-model', content, stop_reason: step ? 'tool_use' : 'end_turn',
      stop_sequence: null, usage: { input_tokens: 10, output_tokens: 10 } };
    if (!body.stream) {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify(message));
      return;
    }
    response.writeHead(200, { 'content-type': 'text/event-stream' });
    const event = (type, value) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...value })}\n\n`);
    event('message_start', { message: { ...message, content: [], stop_reason: null,
      usage: { input_tokens: 10, output_tokens: 0 } } });
    for (const [index, block] of content.entries()) {
      event('content_block_start', { index, content_block: block.type === 'tool_use'
        ? { ...block, input: {} } : { type: 'text', text: '' } });
      event('content_block_delta', { index, delta: block.type === 'tool_use'
        ? { type: 'input_json_delta', partial_json: JSON.stringify(block.input) }
        : { type: 'text_delta', text: block.text } });
      event('content_block_stop', { index });
    }
    event('message_delta', { delta: { stop_reason: message.stop_reason, stop_sequence: null },
      usage: { output_tokens: 10 } });
    event('message_stop', {});
    response.end();
  });
  server.requestTimeout = 10_000;
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  lifetime.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  return { url: `http://127.0.0.1:${server.address().port}`, results };
}
