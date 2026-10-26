/* Existing shell-handler tests use the shipping owner instead of duplicate route variables. */
const { createTopBar } = require('../ui/src/modules/top-bar/index.js');

function installTopBar(context, { lenses, scenarios } = {}) {
  if (context.topBar?.state) return context.topBar;
  const owner = createTopBar({
    authoredLabel: item => context.window.wbManifest?.authoredLensLabel(item) || 'Design',
    writeHash: (...args) => context.writeHash(...args),
    size: () => context.sizeKey?.(),
  });
  const fields = { current: ['src','setPage'], currentScenario: ['scenario','setScenario'], currentExample: ['example','setExample'], currentIterations: ['iterations','setIterations'], lensPref: ['lensPref','setLensPref'], actionsOn: ['actionsOn','setActions'] };
  for (const [key, [field, action]] of Object.entries(fields)) {
    const value = context[key];
    if (value !== undefined) owner[action](value);
    Object.defineProperty(context, key, { configurable: true, get: () => owner.state.get()[field], set: value => owner[action](value) });
  }
  context.topBar = owner;
  context.ports ||= { clearPreview() {} };
  if (lenses) owner.state.watch(owner.select.lenses, value => lenses(value.choices.map(row => ({ textContent: row.label, dataset: { lens: row.key }, click: () => owner.chooseLens(row.key) }))));
  if (scenarios) owner.state.watch(owner.select.scenarios, value => scenarios(value.rows?.map(row => ({ ...row, pick: () => owner.chooseScenario(row.target) }))));
  return owner;
}
module.exports = { installTopBar };
