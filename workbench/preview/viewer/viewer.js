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
    url.searchParams.set(selected.docs ? 'lens' : 'state', state.value);
    url.searchParams.delete(selected.docs ? 'state' : 'lens');
    if (selected.docs) url.searchParams.delete('viewport'); else url.searchParams.set('viewport', viewport.value);
    if (viewport.value === 'responsive') {
      url.searchParams.set('width', document.getElementById('width').value);
      url.searchParams.set('height', document.getElementById('height').value);
    } else { url.searchParams.delete('width'); url.searchParams.delete('height'); }
    history.replaceState(null, '', url);
  }
  function resize() {
    if (!selected) return;
    document.body.style.setProperty('--preview-topbar-bottom', (document.querySelector('.topbar').getBoundingClientRect().bottom + 10) + 'px');
    var canvas = document.getElementById('canvas');
    var padding = parseFloat(getComputedStyle(canvas).padding) || 0;
    var available = [Math.max(1, canvas.clientWidth - padding * 2), Math.max(1, canvas.clientHeight - padding * 2)];
    /* A docs page fills the space, as on the canvas. */
    var dimensions = selected.docs ? available : modes[viewport.value][1] || available;
    if (viewport.value === 'responsive') dimensions = ['width', 'height'].map(function (key) {
      var input = document.getElementById(key);
      var value = Math.min(3840, Math.max(240, Number(input.value) || 720)); input.value = value; return value;
    });
    var scale = Math.min(1, available[0] / dimensions[0], available[1] / dimensions[1]);
    frame.style.width = dimensions[0] + 'px'; frame.style.height = dimensions[1] + 'px'; frame.style.transform = 'scale(' + scale + ')';
    var artboard = document.getElementById('artboard'); artboard.style.width = dimensions[0] * scale + 'px'; artboard.style.height = dimensions[1] * scale + 'px';
    document.querySelectorAll('.dimensions').forEach(function (label) { label.hidden = !!selected.docs || viewport.value !== 'responsive'; });
    remember();
  }
  function target() {
    if (selected.docs) {
      var lens = selected.lenses.find(function (entry) { return entry.key === state.value; }) || selected.lenses[0];
      return new URL('./' + lens.directory.replace(/^browser\//, '') + '/index.html', location.href);
    }
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
    entries().filter(function (preview) { return (preview.title + ' ' + preview.id).toLowerCase().includes(query); }).forEach(function (preview) {
      var button = document.createElement('button'); button.type = 'button'; button.textContent = preview.title;
      if (preview.docs) { var tag = document.createElement('small'); tag.textContent = ' · Docs'; button.appendChild(tag); }
      button.setAttribute('aria-current', String(selected && selected.id === preview.id));
      button.addEventListener('click', function () { choose(preview); }); list.appendChild(button);
    });
    if (!list.childNodes.length) { var empty = document.createElement('p'); empty.textContent = 'No matching previews'; list.appendChild(empty); }
  }
  /* Previews, then docs pages, which carry docs: true. */
  function entries() {
    return catalog.previews.concat((catalog.docs || []).map(function (page) { return Object.assign({ docs: true }, page); }));
  }
  function choose(preview, query) {
    selected = preview;
    document.getElementById('previewTitle').textContent = preview.title;
    document.title = preview.title + ' — ' + catalog.name;
    [state, viewport, document.getElementById('reload')].forEach(function (control) { control.disabled = false; });
    state.parentElement.firstChild.textContent = preview.docs ? 'Lens' : 'State';
    viewport.parentElement.hidden = !!preview.docs;
    if (preview.docs) {
      options(state, preview.lenses.map(function (lens) { return { id: lens.key, label: lens.label }; }), query && query.get('lens') || preview.lens);
      draw(); load();
      return;
    }
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
    if (event.data.event === 'docs-navigate') {
      var page = entries().find(function (entry) { return entry.docs && entry.src === event.data.page; });
      if (page) choose(page, new URLSearchParams({ lens: state.value }));
      return;
    }
    if (event.data.event === 'navigate') {
      var next = catalog.previews.find(function (preview) { return preview.id === event.data.preview; });
      if (next) choose(next, new URLSearchParams({ state: event.data.state || '', viewport: viewport.value }));
      return;
    }
    if (!selected.docs && event.data.state && event.data.state !== state.value) return;
    if (event.data.event === 'ready') notice('Ready · ' + (selected.docs ? 'docs page' : selected.adapter));
    if (event.data.event === 'error') notice(event.data.message, true);
  });
  new ResizeObserver(resize).observe(document.getElementById('canvas'));
  fetch('./workbench.json').then(function (response) { if (!response.ok) throw new Error('Catalog could not be loaded'); return response.json(); }).then(function (data) {
    if (data.version !== 1 || !Array.isArray(data.previews)) throw new Error('Unsupported preview catalog');
    catalog = data; document.getElementById('spaceName').textContent = data.name;
    var warnings = document.getElementById('warnings'); warnings.hidden = !data.warnings.length;
    data.warnings.forEach(function (warning) { var row = document.createElement('li'); row.textContent = warning; warnings.querySelector('ul').appendChild(row); });
    var all = entries();
    if (!all.length) { notice('No previews were built. Check the build warnings.', true); return; }
    var query = new URL(location.href).searchParams;
    choose(all.find(function (preview) { return preview.id === query.get('preview'); }) || all[0], query);
  }).catch(function (error) { notice(error.message + '. Serve this directory with a static HTTP server.', true); });
})();
