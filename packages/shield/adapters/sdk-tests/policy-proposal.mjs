// A host proposes a real policy change and applies it on its own connection:
// only the connection that prepared a review can apply it, and its closing
// withdraws an unapplied one.
export const policyChange = { kind: 'rule_add', rule: { id: 'acme-host-approved', decision: 'allow',
  conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: ['inspect'] } } } };

/** How a review ends for each answer to its form: Shield records a decline as
 * rejected, and a cancelled or unavailable form as withdrawn. */
export const reviewStatus = { accept: 'applied', decline: 'rejected', cancel: 'withdrawn', unavailable: 'withdrawn' };

/** The `data` of every JSON tool result in a Claude-format request body. */
export function returnedData(body) {
  const results = body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
    .filter(block => block.type === 'tool_result');
  return results.flatMap(result => Array.isArray(result.content) ? result.content : [{ text: result.content }])
    .flatMap(block => {
      try { return [JSON.parse(block.text).data]; } catch { return []; }
    }).filter(Boolean);
}

function find(value) {
  if (typeof value === 'string') {
    // Some hosts put a text header, such as Codex's wall time, before the JSON.
    for (const text of [value, value.slice(value.indexOf('{'))]) {
      try { return find(JSON.parse(text)); } catch { /* not JSON */ }
    }
    return undefined;
  }
  if (value && typeof value === 'object') {
    return value.apply_arguments ?? (Array.isArray(value) ? value : Object.values(value))
      .reduce((found, item) => found ?? find(item), undefined);
  }
  return undefined;
}

/** The first `apply_arguments` in a host's conversation, whatever JSON text wraps it.
 * Pass the messages, not the whole request, whose tool schemas name the field. */
export function applyArguments(messages) {
  const found = find(messages);
  if (!found) throw new Error(`No proposal in ${JSON.stringify(messages)}`);
  return found;
}

const steps = (name, messages) => [
  { name: name('policy_propose'), input: { change: policyChange } },
  { name: name('policy_apply'), input: body => applyArguments(messages(body)) },
];

/** Scripted Claude steps that propose `policyChange`, then apply it with the returned arguments. */
export const policyChangeSteps = () => steps(tool => `mcp__shield__${tool}`, body => body.messages);

/** The same steps for the Gemini API peer, whose tool names carry a server prefix. */
export const geminiPolicyChangeSteps = () => steps(suffix => body => body.tools
  ?.flatMap(tool => tool.functionDeclarations ?? [])
  .find(tool => tool.name === suffix || tool.name?.endsWith(`_${suffix}`))?.name, body => body.contents);
