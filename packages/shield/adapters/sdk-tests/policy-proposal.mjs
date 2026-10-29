// Prepare a real immutable candidate before testing the native host's apply prompt.
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { binary } from '../fixtures/runtime.mjs';

export async function preparePolicyChange(root, agent) {
  const client = new Client({ name: 'acme-policy-test', version: '1' });
  try {
    await client.connect(new StdioClientTransport({ command: binary,
      args: ['mcp', 'serve', '--project', root, '--agent', agent === 'claude' ? 'claude-code' : agent], stderr: 'pipe' }));
    const result = await client.callTool({ name: 'policy_propose', arguments: {
      change: { kind: 'rule_add', rule: { id: 'acme-host-approved', decision: 'allow',
        conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: ['inspect'] } } } },
    } });
    if (result.isError || !result.structuredContent?.data?.apply_arguments) throw new Error(JSON.stringify(result));
    return result.structuredContent.data.apply_arguments;
  } finally { await client.close(); }
}
