// Immediate Frame/ElementHandle $eval/$$eval semantics, pinned to Playwright 1.64.1.
// Function normalization follows Playwright (Microsoft, Apache-2.0; upstream notices).
async (injected, root, options, argument) => {
  if (options.relative && !(root instanceof Element)) throw Error('canonic:INVALID_SCOPE');
  const selector = injected.parseSelector(options.selector);
  const until = options.waitForAttachment ? performance.now() + options.waitTimeoutMs : 0;
  let elements;
  for (;;) {
    if (options.waitForAttachment && options.relative && !root.isConnected) throw Error('canonic:SELECTOR_ROOT_DETACHED');
    elements = injected.querySelectorAll(selector, root);
    if (options.strict && elements.length > 1) throw Error('canonic:AMBIGUOUS_LOCATOR');
    if (options.waitForAttachment && performance.now() >= until) throw Error('canonic:SELECTOR_WAIT_EXPIRED');
    if (!options.waitForAttachment || elements.length) break;
    const remaining = until - performance.now();
    if (remaining <= 0) throw Error('canonic:SELECTOR_WAIT_EXPIRED');
    await new Promise(resolve => setTimeout(resolve, Math.min(50, remaining)));
  }
  if (!options.all && !elements.length) throw Error('canonic:ELEMENT_NOT_FOUND');
  if (!options.all && options.strict && elements.length > 1) throw Error('canonic:AMBIGUOUS_LOCATOR');
  let source = options.expression.trim();
  if (options.isFunction) {
    try { new Function('(' + source + ')'); }
    catch {
      source = source.startsWith('async ') ? 'async function ' + source.substring(6) : 'function ' + source;
      new Function('(' + source + ')');
    }
  }
  if (/^(async)?\s*function(\s|\()/.test(source)) source = '(' + source + ')';
  const value = (0, eval)(source);
  const call = options.isFunction === true || (options.isFunction == null && typeof value === 'function');
  return call ? value(options.all ? elements : elements[0], argument) : value;
}
