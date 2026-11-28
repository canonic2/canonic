// Keep the upstream tree/locator semantics; mask secure input values before rendering.
(Constructor, generateTree, elementFor, renderTree, renderYaml, browserName, referenceEngine) => {
  const injected = new Constructor(globalThis, {
    isUnderTest:false,sdkLanguage:'javascript',testIdAttributeName:'data-testid',
    stableRafCount:2,browserName,customEngines:[{name:'testrun',source:referenceEngine}],isUtilityWorld:true,
  });
  injected.testrunAriaSnapshot = element => {
    const tree = generateTree(element, {mode:'default'});
    const visit = node => {
      if (typeof node === 'string') return;
      const element = elementFor(node);
      if (element?.matches('input[type="password" i],input[autocomplete~="current-password" i],input[autocomplete~="new-password" i]')) {
        node.children = ['[masked]'];
      } else {
        for (const child of node.children) visit(child);
      }
    };
    visit(tree.root);
    return renderYaml(renderTree(tree, {mode:'default',depth:12}).json);
  };
  return injected;
}
