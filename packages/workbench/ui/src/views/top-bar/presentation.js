/* Existing top-bar DOM rendering; callers supply choices and pick callbacks. */
window.wbTopBarPresentation = function (options) {
  var lensesBox = options.lensesBox;
  var scenarioMenu = options.scenarioMenu;
  var scenarioButton = options.scenarioButton;
  var scenarioControl = options.scenarioControl;
  var scenarioName = options.scenarioName;
  var sourceMenu = options.sourceMenu;
  var sourceButton = options.sourceButton;
  function openScenarios(open) {
    scenarioMenu.hidden = !open;
    scenarioButton.setAttribute('aria-expanded', String(open));
  }

  function drawScenarioMenu(rows, kind) {
    scenarioMenu.innerHTML = '';
    openScenarios(false);
    scenarioControl.hidden = !rows;
    if (!rows) {
      scenarioName.textContent = '';
      scenarioButton.removeAttribute('title');
      return;
    }
    var picked =
      rows.filter(function (row) {
        return row.current;
      })[0] || rows[0];
    scenarioName.textContent = picked.label;
    scenarioButton.title =
      (kind === 'story' ? 'Story: ' : 'Scenario: ') + picked.label + ' — pick another';
    scenarioMenu.setAttribute('aria-label', kind === 'story' ? 'Story' : 'Scenario');
    rows.forEach(function (entry) {
      var row = document.createElement('button');
      row.className = 'wb-menu-row';
      row.type = 'button';
      row.setAttribute('role', 'menuitem');
      row.setAttribute('aria-current', String(entry === picked));
      row.textContent = entry.label;
      row.dataset.scenario = entry.target || '';
      row.addEventListener('click', function () {
        openScenarios(false);
        entry.pick();
      });
      scenarioMenu.appendChild(row);
    });
  }

  function openSources(open) {
    sourceMenu.hidden = !open;
    sourceButton.setAttribute('aria-expanded', String(open));
  }

  function sourceRow(label, sub, enabled, title, onPick) {
    var row = document.createElement('button');
    row.className = 'wb-menu-row';
    row.type = 'button';
    row.setAttribute('role', 'menuitem');
    row.disabled = !enabled;
    row.title = title;
    row.appendChild(document.createTextNode(label));
    var line = document.createElement('span');
    line.className = 'wb-menu-sub';
    line.textContent = sub;
    row.appendChild(line);
    if (enabled) {
      row.addEventListener('click', function () {
        openSources(false);
        onPick();
      });
    }
    sourceMenu.appendChild(row);
  }
  function drawLenses(choices, showing, pick) {
    lensesBox.innerHTML = '';
    lensesBox.hidden = choices.length < 2;
    if (lensesBox.hidden) return;
    choices.forEach(function (choice) {
      var button = document.createElement('button');
      button.className = 'wb-lens';
      button.type = 'button';
      button.dataset.lens = choice.key;
      button.textContent = choice.label;
      button.title = choice.title;
      button.setAttribute('aria-pressed', String(showing === choice.key));
      button.addEventListener('click', function () {
        pick(choice.key);
      });
      lensesBox.appendChild(button);
    });
  }
  return { openScenarios, drawScenarioMenu, openSources, sourceRow, drawLenses };
};
