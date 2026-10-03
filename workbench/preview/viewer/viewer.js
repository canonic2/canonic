/* A static, portable reader for the exported versioned preview catalog. */
(function () {
  var frame = document.getElementById('previewFrame');
  var list = document.getElementById('previews');
  var search = document.getElementById('search');
  var state = document.getElementById('state');
  var viewport = document.getElementById('viewport');
  var status = document.getElementById('status');
  var selected = null;
  var catalog = null;
  var modes = { fit: ['Fit', null], desktop: ['Desktop', [1512, 982]], mobile: ['Mobile', [393, 852]], responsive: ['Resizable', null] };
  function notice(message, error) { status.textContent = message; status.toggleAttribute('data-error', !!error); }
  function options(select, values, current) {
    select.replaceChildren();
    values.forEach(function (value) { var option = document.createElement('option'); option.value = value.id; option.textContent = value.label; select.appendChild(option); });
    select.value = values.some(function (value) { return value.id === current; }) ? current : values[0].id;
  }
  function remember() {
    if (!selected) return;
    var url = new URL(location.href);
    url.searchParams.set('preview', selected.id);
    url.searchParams.set('state', state.value);
    url.searchParams.set('viewport', viewport.value);
    if (viewport.value === 'responsive') {
      url.searchParams.set('width', document.getElementById('width').value);
      url.searchParams.set('height', document.getElementById('height').value);
    } else { url.searchParams.delete('width'); url.searchParams.delete('height'); }
    history.replaceState(null, '', url);
  }
  function resize() {
    if (!selected) return;
    document.body.style.setProperty('--preview-toolbar-bottom', (document.querySelector('.toolbar').getBoundingClientRect().bottom + 10) + 'px');
    var canvas = document.getElementById('canvas');
    var padding = parseFloat(getComputedStyle(canvas).padding) || 0;
    var available = [Math.max(1, canvas.clientWidth - padding * 2), Math.max(1, canvas.clientHeight - padding * 2)];
    var dimensions = modes[viewport.value][1] || available;
    if (viewport.value === 'responsive') dimensions = ['width', 'height'].map(function (key) {
      var input = document.getElementById(key);
      var value = Math.min(3840, Math.max(240, Number(input.value) || 720)); input.value = value; return value;
    });
    var scale = Math.min(1, available[0] / dimensions[0], available[1] / dimensions[1]);
    frame.style.width = dimensions[0] + 'px'; frame.style.height = dimensions[1] + 'px'; frame.style.transform = 'scale(' + scale + ')';
    var stage = document.getElementById('stage'); stage.style.width = dimensions[0] * scale + 'px'; stage.style.height = dimensions[1] * scale + 'px';
    document.querySelectorAll('.dimensions').forEach(function (label) { label.hidden = viewport.value !== 'responsive'; });
    remember();
  }
  function target() {
    var url = new URL('./' + selected.directory.replace(/^browser\//, '') + '/index.html', location.href);
    url.searchParams.set('state', state.value); return url;
  }
  function load() {
    if (!selected) return;
    window.wbPreviewControls.reset();
    var url = target(); document.getElementById('open').href = url.href;
    document.getElementById('open').hidden = false;
    frame.title = selected.title + ' — ' + state.options[state.selectedIndex].textContent;
    notice('Rendering ' + frame.title + '…'); frame.src = url.href;
    resize();
  }
  function draw() {
    if (!catalog) return;
    list.replaceChildren();
    var query = search.value.trim().toLowerCase();
    catalog.previews.filter(function (preview) { return (preview.title + ' ' + preview.id).toLowerCase().includes(query); }).forEach(function (preview) {
      var button = document.createElement('button'); button.type = 'button'; button.textContent = preview.title;
      button.setAttribute('aria-current', String(selected && selected.id === preview.id));
      button.addEventListener('click', function () { choose(preview); }); list.appendChild(button);
    });
    if (!list.childNodes.length) { var empty = document.createElement('p'); empty.textContent = 'No matching previews'; list.appendChild(empty); }
  }
  function choose(preview, query) {
    selected = preview;
    document.getElementById('previewTitle').textContent = preview.title;
    document.title = preview.title + ' — ' + catalog.name;
    [state, viewport, document.getElementById('reload')].forEach(function (control) { control.disabled = false; });
    options(state, preview.states, query && query.get('state'));
    options(viewport, (preview.viewports || Object.keys(modes)).map(function (id) { return { id: id, label: modes[id][0] }; }), query && query.get('viewport') || viewport.value || 'fit');
    if (query) ['width', 'height'].forEach(function (key) { if (query.has(key)) document.getElementById(key).value = query.get(key); });
    draw(); load();
  }
  search.addEventListener('input', draw);
  state.addEventListener('change', load);
  viewport.addEventListener('change', resize);
  ['width', 'height'].forEach(function (key) { document.getElementById(key).addEventListener('change', resize); });
  document.getElementById('reload').addEventListener('click', load);
  frame.addEventListener('load', function () { window.dispatchEvent(new CustomEvent('wb-frame-change', { detail: { frame: frame } })); });
  window.addEventListener('message', function (event) {
    if (event.source !== frame.contentWindow || event.origin !== location.origin || event.data?.type !== 'workbench-preview' || event.data.id !== selected?.id) return;
    if (event.data.state && event.data.state !== state.value) return;
    if (event.data.event === 'ready') notice('Ready · ' + selected.adapter);
    if (event.data.event === 'error') notice(event.data.message, true);
  });
  new ResizeObserver(resize).observe(document.getElementById('canvas'));
  fetch('./workbench.json').then(function (response) { if (!response.ok) throw new Error('Catalog could not be loaded'); return response.json(); }).then(function (data) {
    if (data.version !== 1 || !Array.isArray(data.previews)) throw new Error('Unsupported preview catalog');
    catalog = data; document.getElementById('projectName').textContent = data.name;
    var warnings = document.getElementById('warnings'); warnings.hidden = !data.warnings.length;
    data.warnings.forEach(function (warning) { var row = document.createElement('li'); row.textContent = warning; warnings.querySelector('ul').appendChild(row); });
    if (!data.previews.length) { notice('No previews were built. Check the build warnings.', true); return; }
    var query = new URL(location.href).searchParams;
    choose(data.previews.find(function (preview) { return preview.id === query.get('preview'); }) || data.previews[0], query);
  }).catch(function (error) { notice(error.message + '. Serve this directory with a static HTTP server.', true); });
})();
