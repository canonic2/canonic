// Enumerate the actual CLI-published schemas, including nested choices and
// shared definitions. Keep nested rows for compacted, operation-scoped shapes.
const contextualDefinitions = new Set(['pageCondition', 'httpRequestBody']);

export function publicSurface(tools) {
  if (!Array.isArray(tools)) throw new Error('Missing public tool catalog');
  const browser = tools.filter(tool => tool.name.startsWith('browser_') ||
    ['connect','act','observe','status','cancel','disconnect','control_status','control_acquire','control_release'].includes(tool.name));
  if (!browser.some(tool => tool.name === 'browser_send')) throw new Error('Missing browser operation schema');
  const rows = [];
  function visit(schema, name, operation, definitions) {
    if (!schema || typeof schema !== 'object' || Array.isArray(schema)) return;
    const reference = schema.$ref?.startsWith('#/$defs/') && schema.$ref.slice('#/$defs/'.length);
    if (contextualDefinitions.has(reference) && definitions[reference]) {
      visit(definitions[reference], name, operation, definitions);
      return;
    }
    const own = schema.properties?.operation?.const ?? schema.properties?.action?.const ?? operation;
    for (const [field, value] of Object.entries(schema.properties ?? {})) {
      const path = `${name}.${field}`;
      rows.push({ kind:'public-parameter', name:path, operationName:own ?? null, required:(schema.required ?? []).includes(field), schema:value });
      visit(value, path, own, definitions);
    }
    for (const keyword of ['oneOf','anyOf','allOf']) {
      for (const [index, value] of (schema[keyword] ?? []).entries()) visit(value, `${name}/${keyword}/${index}`, own, definitions);
    }
    for (const [key,value] of Object.entries(schema.$defs ?? {})) {
      if (!contextualDefinitions.has(key)) visit(value, `${name}/$defs/${key}`, own, definitions);
    }
    if (schema.items) visit(schema.items, `${name}[]`, own, definitions);
    if (typeof schema.additionalProperties === 'object') visit(schema.additionalProperties, `${name}.*`, own, definitions);
  }
  for (const tool of browser) {
    let schema = tool.inputSchema;
    if (!tool.name.startsWith('browser_') && schema.anyOf) {
      const branch = schema.anyOf[1];
      if (schema.anyOf.length !== 2 || !(branch?.properties?.engine?.enum?.includes('chrome') || /browser/i.test(branch?.properties?.targetId?.description ?? ''))) throw new Error('Shared browser branch changed');
      schema = {...branch, ...(schema.$defs ? {$defs:schema.$defs} : {})};
    }
    rows.push({kind:'public-tool-schema', name:tool.name, schema});
    visit(schema, tool.name, undefined, schema.$defs ?? {});
  }
  if (new Set(rows.map(row => `${row.kind}/${row.name}`)).size !== rows.length) throw new Error('Repeated public schema identity');
  return rows;
}
