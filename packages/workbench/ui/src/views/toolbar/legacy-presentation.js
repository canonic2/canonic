/* Classic annotation toolbar presentation. Annotation and review state arrive as callbacks. */
window.wbAnnotationToolbar = function () {
  var shapeButton = document.getElementById('shapeTool');
  var shapeCaret = document.getElementById('shapeCaret');
  var shapeMenu = document.getElementById('shapeMenu');
  var handoffControl = document.getElementById('handoffControl');
  var handoffButton = document.getElementById('handoff');
  var annotationCount = document.getElementById('annotationCount');
  var toolButtons = Array.prototype.slice.call(document.querySelectorAll('.wb-tool'));
  var menuItems = shapeMenu
    ? Array.prototype.slice.call(shapeMenu.querySelectorAll('.wb-menu-item'))
    : [];
  var SHAPE_ICONS = { line: 'minus', rect: 'square', ellipse: 'circle' };
  var SHAPE_NAMES = { line: 'Line', rect: 'Rectangle', ellipse: 'Circle' };

  function openMenu(open) {
    if (!shapeMenu || !shapeCaret) return;
    shapeMenu.hidden = !open;
    shapeCaret.setAttribute('aria-expanded', String(open));
  }
  function tool(name) {
    toolButtons.forEach(function (button) {
      button.setAttribute('aria-pressed', String(button.dataset.tool === name));
    });
  }
  function shape(name) {
    if (!shapeButton) return;
    shapeButton.dataset.tool = name;
    shapeButton.title = SHAPE_NAMES[name];
    shapeButton.setAttribute('aria-label', SHAPE_NAMES[name]);
    shapeButton.innerHTML = '';
    shapeButton.appendChild(window.wbIcon(SHAPE_ICONS[name]));
    menuItems.forEach(function (button) {
      button.setAttribute('aria-current', String(button.dataset.shape === name));
    });
  }
  function count(value) {
    if (!annotationCount) return;
    annotationCount.hidden = !value;
    annotationCount.textContent = value === 1 ? '1 annotation' : value + ' annotations';
  }
  function connect(actions) {
    toolButtons.forEach(function (button) {
      if (button === shapeButton) return;
      button.addEventListener('click', function () {
        actions.setTool(actions.tool() === button.dataset.tool ? 'pointer' : button.dataset.tool);
        openMenu(false);
      });
    });
    if (shapeButton)
      shapeButton.addEventListener('click', function () {
        actions.setTool(actions.tool() === actions.shape() ? 'pointer' : actions.shape());
        openMenu(false);
      });
    if (shapeCaret)
      shapeCaret.addEventListener('click', function () {
        openMenu(shapeMenu.hidden);
      });
    menuItems.forEach(function (button) {
      button.addEventListener('click', function () {
        actions.setShape(button.dataset.shape);
        openMenu(false);
      });
    });
    document.addEventListener('pointerdown', function (event) {
      if (shapeMenu && !shapeMenu.hidden && !event.target.closest('.wb-shape')) openMenu(false);
    });
    [
      ['undo', actions.undo],
      ['clearAnnotations', actions.clear],
      ['shoot', actions.shoot],
      ['handoff', actions.handoff],
    ].forEach(function (entry) {
      var button = document.getElementById(entry[0]);
      if (button) button.addEventListener('click', entry[1]);
    });
  }
  return {
    tool: tool,
    shape: shape,
    count: count,
    connect: connect,
    shapeName: function () {
      return shapeButton ? shapeButton.dataset.tool : 'rect';
    },
    close: function () {
      openMenu(false);
    },
    hasHandoff: !!handoffControl,
    enableHandoff: function () {
      handoffControl.hidden = false;
    },
    busyHandoff: function (busy) {
      handoffButton.disabled = busy;
      if (busy) handoffButton.setAttribute('aria-busy', 'true');
      else handoffButton.removeAttribute('aria-busy');
    },
  };
};
