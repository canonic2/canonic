/* Classic control wiring; the shell retains route, lens and Actions state. */
window.wbTopBarBindings = function (options) {
  var scenarioButton = document.getElementById('scenarioButton');
  var sourceButton = document.getElementById('openSource');
  var scenarioMenu = document.getElementById('scenarioMenu');
  var sourceMenu = document.getElementById('sourceMenu');
  var openSources = options.openSources;
  var openScenarios = options.openScenarios;
  document.getElementById('actionsToggle').addEventListener('wb-actions-toggle', function (e) {
    options.actions(e.detail.checked);
  });
  document.getElementById('reload').addEventListener('click', options.reload);
  scenarioButton.addEventListener('click', function () {
    openSources(false);
    openScenarios(scenarioMenu.hidden);
  });

  sourceButton.addEventListener('click', function () {
    openScenarios(false);
    openSources(sourceMenu.hidden);
  });

  document.addEventListener('pointerdown', function (e) {
    if (!scenarioMenu.hidden && !e.target.closest('.wb-scenarios-control')) openScenarios(false);
    if (!sourceMenu.hidden && !e.target.closest('.wb-sources')) openSources(false);
  });

  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Escape') return;
    openScenarios(false);
    openSources(false);
  });
};
