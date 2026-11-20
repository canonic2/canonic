// Deterministic loopback Gemini API peer; no external model or account is used.
import { createServer } from 'node:http';

export async function geminiProvider(t, steps) {
  const requests = [];
  const results = [];
  let index = 0;
  const server = createServer(async (request, response) => {
    let bytes = '';
    for await (const chunk of request) bytes += chunk;
    const body = JSON.parse(bytes || '{}');
    if (request.url.includes('countTokens')) {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ totalTokens: 10 }));
      return;
    }
    requests.push(body);
    for (const content of body.contents ?? []) {
      for (const part of content.parts ?? []) if (part.functionResponse) results.push(part.functionResponse);
    }
    const step = body.tools?.some(tool => tool.functionDeclarations?.length) ? steps[index++] : undefined;
    const name = typeof step?.name === 'function' ? step.name(body) : step?.name;
    const args = typeof step?.input === 'function' ? step.input(body) : step?.input;
    const parts = step && name ? [{ functionCall: { name, args } }]
      : [{ text: 'Acme fixture finished.' }];
    const result = { candidates: [{ content: { role: 'model', parts }, finishReason: 'STOP', index: 0 }],
      usageMetadata: { promptTokenCount: 10, candidatesTokenCount: 10, totalTokenCount: 20 }, modelVersion: 'gemini-2.5-flash' };
    if (request.url.includes('streamGenerateContent')) {
      response.writeHead(200, { 'content-type': 'text/event-stream' });
      response.end(`data: ${JSON.stringify(result)}\n\n`);
    } else {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify(result));
    }
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  t.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  return { url: `http://127.0.0.1:${server.address().port}`, requests, results };
}
