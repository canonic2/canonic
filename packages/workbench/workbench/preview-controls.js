/* Inputs and actions are transient experiments. Definitions and reference
   state screenshots remain unchanged; Reset restores the selected state. */
(function () {
  var button = document.getElementById('previewControls');
  var panel = document.createElement('aside');
  panel.className = 'wb-preview-controls';
  panel.setAttribute('aria-label', 'Preview controls');
  panel.hidden = true;
  document.querySelector('.wb').appendChild(panel);
  var selected = null;
  var overrides = {};
  var log = [];
  var opened = false;
  function frame() { return document.querySelector('iframe.is-active'); }
  function send(command) {
    var active = frame();
    if (active && selected) active.contentWindow.postMessage({ type: 'workbench-preview-command', command: command, state: selected.state, inputs: overrides }, new URL(active.src).origin);
  }
  function element(tag, text) { var node = document.createElement(tag); if (text) node.textContent = text; return node; }
  function draw() {
    panel.replaceChildren();
    panel.appendChild(element('h2', 'Preview controls'));
    Object.keys(selected && selected.controls || {}).forEach(function (name) {
      var spec = selected.controls[name];
      var label = element('label');
      label.appendChild(element('span', spec.label || name));
      var input = element(spec.type === 'select' ? 'select' : spec.type === 'json' ? 'textarea' : 'input');
      var value = selected.inputs[name];
      if (spec.type === 'select') (spec.options || []).forEach(function (option) { var node = element('option', String(option)); node.value = String(option); input.appendChild(node); });
      else if (spec.type !== 'json') input.type = spec.type === 'boolean' ? 'checkbox' : spec.type === 'number' ? 'number' : 'text';
      if (spec.type === 'boolean') input.checked = !!value;
      else input.value = spec.type === 'json' ? JSON.stringify(value, null, 2) : value === undefined ? '' : String(value);
      ['min', 'max', 'step'].forEach(function (key) { if (spec[key] !== undefined) input[key] = spec[key]; });
      input.addEventListener('change', function () {
        try {
          var next = spec.type === 'boolean' ? input.checked : spec.type === 'number' ? Number(input.value) : spec.type === 'json' ? JSON.parse(input.value) : input.value;
          if (spec.type === 'select') next = spec.options.find(function (option) { return String(option) === input.value; });
          overrides[name] = next;
          input.setCustomValidity('');
          send();
        } catch (_) { input.setCustomValidity('Enter valid JSON'); input.reportValidity(); }
      });
      label.appendChild(input);
      panel.appendChild(label);
    });
    var reset = element('button', 'Reset state');
    reset.type = 'button';
    reset.addEventListener('click', function () { overrides = {}; log = []; send('reset'); draw(); });
    panel.appendChild(reset);
    panel.appendChild(element('h3', 'Actions'));
    var actions = element('ol');
    log.slice(-30).forEach(function (action) { actions.appendChild(element('li', action.name + (action.values.length ? ': ' + action.values.join(', ') : ''))); });
    if (!log.length) panel.appendChild(element('p', 'No actions yet.'));
    panel.appendChild(actions);
    if (selected && selected.docs) {
      var docs = element('details');
      docs.appendChild(element('summary', 'Documentation'));
      docs.appendChild(element('p', selected.docs));
      panel.appendChild(docs);
    }
  }
  button.addEventListener('click', function () { opened = !opened; panel.hidden = !opened; button.setAttribute('aria-expanded', String(opened)); });
  window.addEventListener('wb-frame-change', function () {
    var active = frame();
    if (active) active.contentWindow.postMessage({ type: 'workbench-preview-command', command: 'inspect' }, new URL(active.src).origin);
  });
  window.addEventListener('message', function (event) {
    var active = frame();
    if (!active || event.source !== active.contentWindow || event.origin !== location.origin || event.data && event.data.type !== 'workbench-preview') return;
    var data = event.data;
    if (!data) return;
    var requested = new URL(active.src).searchParams.get('state');
    if (requested && data.state && requested !== data.state) return;
    if (data.event === 'ready') {
      if (!selected || selected.id !== data.id || selected.state !== data.state) { overrides = {}; log = []; }
      selected = data;
      overrides = Object.assign({}, data.inputs);
      log = data.actions || [];
      button.hidden = false;
      draw();
    } else if (data.event === 'action' && selected && selected.id === data.id) {
      log.push({ name: data.name, values: data.values || [] });
      draw();
    }
  });
  window.wbPreviewControls = { reset: function () { selected = null; overrides = {}; log = []; button.hidden = true; panel.hidden = true; opened = false; button.setAttribute('aria-expanded', 'false'); } };
})();
