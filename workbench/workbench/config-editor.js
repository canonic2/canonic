/* A focused, master-detail editor for the handwritten collections in
   workbench.yaml. The local server owns persistence; this surface keeps the
   raw objects intact so states, implementation mappings, and code pointers
   survive edits even when they are not exposed as form controls here. */
(function () {
  var VIEWPORTS = [
    { id: 'fit', label: 'Fit', icon: 'minimize-2', description: 'Fill the available workbench', exportLabel: '1440 × 900 reference' },
    { id: 'desktop', label: 'Desktop', icon: 'monitor', description: 'MacBook Pro 14 frame', exportLabel: '1512 × 982 reference' },
    { id: 'mobile', label: 'Mobile', icon: 'smartphone', description: 'iPhone 15 Pro frame', exportLabel: '393 × 852 reference' },
    { id: 'responsive', label: 'Responsive', icon: 'scaling', description: 'Drag to test any size', exportLabel: 'Desktop + mobile references' },
  ];

  function element(name, className, text) {
    var node = document.createElement(name);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function icon(name, size) {
    return window.wbIcon(name, size || 16);
  }

  function iconButton(label, iconName, onClick, className) {
    var button = element('button', className || 'wb-config-icon');
    button.type = 'button';
    button.title = label;
    button.setAttribute('aria-label', label);
    button.appendChild(icon(iconName, 15));
    button.addEventListener('click', function (event) {
      event.stopPropagation();
      onClick();
    });
    return button;
  }

  function field(label, value, onChange, options) {
    options = options || {};
    var wrapper = element('label', 'wb-config-field');
    wrapper.appendChild(element('span', 'wb-config-field-label', label));
    var control = element('input');
    control.type = 'text';
    control.value = value || '';
    control.placeholder = options.placeholder || '';
    control.addEventListener('input', function () { onChange(control.value); });
    wrapper.appendChild(control);
    if (options.help) wrapper.appendChild(element('small', '', options.help));
    return wrapper;
  }

  function pageViewports(page) {
    return window.wbManifest.pageViewports(page.viewports, page.label || 'Page', []);
  }

  function canonicalize(entries) {
    (entries || []).forEach(function (entry) {
      if (entry.group) {
        canonicalize(entry.items);
        return;
      }
      var selected = pageViewports(entry);
      if (selected.length === VIEWPORTS.length) delete entry.viewports;
      else entry.viewports = selected;
    });
  }

  function create(options) {
    var dialog = options.dialog;
    var body = options.body;
    var status = options.status;
    var collections = [];
    var selection = null;
    var navPane = null;
    var inspector = null;

    function setStatus(message, error) {
      status.textContent = message || '';
      status.classList.toggle('is-error', !!error);
    }

    function pageCount(entries) {
      return (entries || []).reduce(function (count, entry) {
        return count + (entry.group ? pageCount(entry.items) : 1);
      }, 0);
    }

    function totalPages() {
      return collections.reduce(function (count, collection) { return count + pageCount(collection.items); }, 0);
    }

    function select(kind, value, owner, collection) {
      selection = { kind: kind, value: value, owner: owner, collection: collection || value };
      renderNavigation();
      renderInspector();
    }

    function selected(value) {
      return !!selection && selection.value === value;
    }

    function addPage(owner, collection) {
      var page = { label: 'Untitled page', src: 'pages/untitled.html' };
      owner.push(page);
      select('page', page, owner, collection);
    }

    function removeSelected(message) {
      if (!selection || !window.confirm(message)) return;
      var owner = selection.owner;
      var at = owner.indexOf(selection.value);
      if (at !== -1) owner.splice(at, 1);
      var collection = selection.collection;
      selection = collection && collections.indexOf(collection) !== -1
        ? { kind: 'collection', value: collection, owner: collections, collection: collection }
        : collections.length ? { kind: 'collection', value: collections[0], owner: collections, collection: collections[0] } : null;
      render();
    }

    function navPage(page, owner, collection, nested) {
      var row = element('button', 'wb-config-nav-page' + (nested ? ' is-nested' : ''));
      row.type = 'button';
      row.setAttribute('aria-current', String(selected(page)));
      row.appendChild(icon(page.icon || 'file-text', 15));
      var copy = element('span', 'wb-config-nav-copy');
      copy.appendChild(element('strong', '', page.label || 'Untitled page'));
      copy.appendChild(element('small', '', page.src || 'No source path'));
      row.appendChild(copy);
      row.addEventListener('click', function () { select('page', page, owner, collection); });
      return row;
    }

    function navGroup(group, owner, collection) {
      var block = element('div', 'wb-config-nav-group');
      var head = element('button', 'wb-config-nav-group-head');
      head.type = 'button';
      head.setAttribute('aria-current', String(selected(group)));
      head.appendChild(icon('folder', 15));
      head.appendChild(element('strong', '', group.group || 'Untitled group'));
      head.appendChild(element('small', '', String(pageCount(group.items))));
      head.addEventListener('click', function () { select('group', group, owner, collection); });
      block.appendChild(head);
      (group.items || []).forEach(function (page) {
        block.appendChild(navPage(page, group.items, collection, true));
      });
      return block;
    }

    function navCollection(collection) {
      var block = element('section', 'wb-config-nav-collection');
      var head = element('div', 'wb-config-nav-collection-head');
      var pick = element('button', 'wb-config-nav-collection-pick');
      pick.type = 'button';
      pick.setAttribute('aria-current', String(selected(collection)));
      pick.appendChild(element('span', '', collection.name || 'Untitled collection'));
      pick.appendChild(element('small', '', String(pageCount(collection.items))));
      pick.addEventListener('click', function () { select('collection', collection, collections, collection); });
      head.appendChild(pick);
      head.appendChild(iconButton('Add page to ' + (collection.name || 'collection'), 'plus', function () {
        addPage(collection.items || (collection.items = []), collection);
      }, 'wb-config-nav-add'));
      block.appendChild(head);
      (collection.items || []).forEach(function (entry) {
        block.appendChild(entry.group
          ? navGroup(entry, collection.items, collection)
          : navPage(entry, collection.items, collection, false));
      });
      return block;
    }

    function renderNavigation() {
      if (!navPane) return;
      navPane.innerHTML = '';
      var head = element('div', 'wb-config-nav-title');
      var count = totalPages();
      head.appendChild(element('strong', '', 'Pages'));
      head.appendChild(element('span', '', count + (count === 1 ? ' page' : ' pages')));
      navPane.appendChild(head);
      var list = element('nav', 'wb-config-nav-list');
      list.setAttribute('aria-label', 'Workbench pages');
      if (!collections.length) list.appendChild(element('p', 'wb-config-nav-empty', 'No handwritten collections yet.'));
      else collections.forEach(function (collection) { list.appendChild(navCollection(collection)); });
      navPane.appendChild(list);
    }

    function inspectorHeader(kind, title, description, iconName) {
      var header = element('header', 'wb-config-inspector-head');
      var glyph = element('span', 'wb-config-inspector-icon');
      glyph.appendChild(icon(iconName, 18));
      header.appendChild(glyph);
      var copy = element('div');
      copy.appendChild(element('span', 'wb-config-eyebrow', kind));
      copy.appendChild(element('h3', '', title));
      if (description) copy.appendChild(element('p', '', description));
      header.appendChild(copy);
      return header;
    }

    function danger(label, onClick) {
      var footer = element('div', 'wb-config-danger');
      var button = element('button', '', label);
      button.type = 'button';
      button.appendChild(icon('trash-2', 14));
      button.addEventListener('click', onClick);
      footer.appendChild(button);
      return footer;
    }

    function renderPage(page) {
      inspector.appendChild(inspectorHeader(
        'Page', page.label || 'Untitled page', page.src || 'Add a source path', page.icon || 'file-text'
      ));
      var form = element('div', 'wb-config-inspector-form');
      form.appendChild(field('Page label', page.label, function (value) {
        page.label = value;
        renderNavigation();
      }, { placeholder: 'Sign in', help: 'Shown in the workbench sidebar and export guide.' }));
      form.appendChild(field('Source path', page.src, function (value) {
        page.src = value;
        renderNavigation();
      }, { placeholder: 'pages/sign-in.html', help: 'Relative to the project root.' }));
      inspector.appendChild(form);

      var viewportSection = element('section', 'wb-config-option-section');
      viewportSection.appendChild(element('h4', '', 'Supported viewports'));
      viewportSection.appendChild(element('p', '', 'Only selected modes are available in the workbench. Responsive exports desktop and mobile references.'));
      var grid = element('div', 'wb-config-viewport-grid');
      var active = pageViewports(page);
      VIEWPORTS.forEach(function (viewport) {
        var card = element('button', 'wb-config-viewport-card');
        card.type = 'button';
        var on = active.indexOf(viewport.id) !== -1;
        card.setAttribute('aria-pressed', String(on));
        var cardIcon = element('span', 'wb-config-viewport-icon');
        cardIcon.appendChild(icon(viewport.icon, 19));
        card.appendChild(cardIcon);
        var cardCopy = element('span', 'wb-config-viewport-copy');
        cardCopy.appendChild(element('strong', '', viewport.label));
        cardCopy.appendChild(element('span', '', viewport.description));
        cardCopy.appendChild(element('small', '', viewport.exportLabel));
        card.appendChild(cardCopy);
        var check = element('span', 'wb-config-viewport-check');
        check.appendChild(icon(on ? 'check' : 'plus', 13));
        card.appendChild(check);
        card.addEventListener('click', function () {
          var next = active.slice();
          var at = next.indexOf(viewport.id);
          if (at === -1) next.push(viewport.id);
          else if (next.length > 1) next.splice(at, 1);
          if (next.length === VIEWPORTS.length) delete page.viewports;
          else page.viewports = VIEWPORTS.map(function (item) { return item.id; }).filter(function (id) { return next.indexOf(id) !== -1; });
          renderInspector();
        });
        grid.appendChild(card);
      });
      viewportSection.appendChild(grid);
      inspector.appendChild(viewportSection);

      var advanced = element('div', 'wb-config-preserved');
      advanced.appendChild(icon('braces', 16));
      advanced.appendChild(element('span', '', 'States, implementations, and code mappings stay in the YAML and are preserved when you save.'));
      inspector.appendChild(advanced);
      inspector.appendChild(danger('Remove page', function () {
        removeSelected('Remove “' + (page.label || 'this page') + '” from the workbench?');
      }));
    }

    function renderCollection(collection) {
      inspector.appendChild(inspectorHeader('Collection', collection.name || 'Untitled collection', pageCount(collection.items) + ' pages', collection.icon || 'layers-3'));
      var form = element('div', 'wb-config-inspector-form');
      form.appendChild(field('Collection name', collection.name, function (value) {
        collection.name = value;
        renderNavigation();
      }, { placeholder: 'Pages' }));
      form.appendChild(field('Lucide icon', collection.icon, function (value) { collection.icon = value; }, {
        placeholder: 'file-text', help: 'A kebab-case Lucide icon name.',
      }));
      inspector.appendChild(form);
      var primary = element('button', 'wb-config-inspector-action');
      primary.type = 'button';
      primary.appendChild(icon('plus', 15));
      primary.appendChild(document.createTextNode('Add page to this collection'));
      primary.addEventListener('click', function () { addPage(collection.items || (collection.items = []), collection); });
      inspector.appendChild(primary);
      inspector.appendChild(danger('Remove collection', function () {
        removeSelected('Remove “' + (collection.name || 'this collection') + '” and all of its pages?');
      }));
    }

    function renderGroup(group) {
      inspector.appendChild(inspectorHeader('Group', group.group || 'Untitled group', pageCount(group.items) + ' pages', 'folder'));
      var form = element('div', 'wb-config-inspector-form');
      form.appendChild(field('Group name', group.group, function (value) {
        group.group = value;
        renderNavigation();
      }, { placeholder: 'Authentication' }));
      inspector.appendChild(form);
      var primary = element('button', 'wb-config-inspector-action');
      primary.type = 'button';
      primary.appendChild(icon('plus', 15));
      primary.appendChild(document.createTextNode('Add page to this group'));
      primary.addEventListener('click', function () { addPage(group.items || (group.items = []), selection.collection); });
      inspector.appendChild(primary);
      inspector.appendChild(danger('Remove group', function () {
        removeSelected('Remove “' + (group.group || 'this group') + '” and all of its pages?');
      }));
    }

    function renderInspector() {
      if (!inspector) return;
      inspector.innerHTML = '';
      if (!selection) {
        var empty = element('div', 'wb-config-inspector-empty');
        empty.appendChild(icon('panel-left', 28));
        empty.appendChild(element('h3', '', 'Choose a page'));
        empty.appendChild(element('p', '', 'Select an item on the left to edit its workbench settings.'));
        inspector.appendChild(empty);
        return;
      }
      if (selection.kind === 'page') renderPage(selection.value);
      else if (selection.kind === 'group') renderGroup(selection.value);
      else renderCollection(selection.value);
    }

    function render() {
      body.innerHTML = '';
      var workspace = element('div', 'wb-config-workspace');
      navPane = element('aside', 'wb-config-nav');
      inspector = element('section', 'wb-config-inspector');
      workspace.appendChild(navPane);
      workspace.appendChild(inspector);
      body.appendChild(workspace);
      renderNavigation();
      renderInspector();
    }

    function open() {
      setStatus('Loading…');
      dialog.showModal();
      fetch('/_workbench/config-file').then(function (response) {
        return response.json().then(function (result) {
          if (!response.ok || !result.ok) throw new Error(result.error || 'Could not read workbench.yaml.');
          return result;
        });
      }).then(function (result) {
        collections = result.collections || [];
        selection = collections.length ? { kind: 'collection', value: collections[0], owner: collections, collection: collections[0] } : null;
        setStatus('');
        render();
      }).catch(function (error) { setStatus(String(error.message || error), true); });
    }

    function validate() {
      var problem = null;
      collections.some(function (collection, index) {
        if (!String(collection.name || '').trim()) { problem = 'Collection ' + (index + 1) + ' needs a name.'; return true; }
        function visit(entries) {
          return (entries || []).some(function (entry) {
            if (entry.group) return visit(entry.items);
            if (!String(entry.label || '').trim() || !String(entry.src || '').trim()) {
              problem = 'Every page needs a label and source path.';
              return true;
            }
            return false;
          });
        }
        return visit(collection.items);
      });
      return problem;
    }

    function save() {
      var problem = validate();
      if (problem) { setStatus(problem, true); return; }
      collections.forEach(function (collection) { canonicalize(collection.items); });
      options.save.disabled = true;
      setStatus('Saving…');
      fetch('/_workbench/config-file', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ collections: collections }),
      }).then(function (response) {
        return response.json().then(function (result) {
          if (!response.ok || !result.ok) throw new Error(result.error || 'Could not save workbench.yaml.');
          return result;
        });
      }).then(function () {
        dialog.close();
        options.onSaved();
      }).catch(function (error) {
        setStatus(String(error.message || error), true);
      }).then(function () { options.save.disabled = false; });
    }

    options.button.addEventListener('click', open);
    options.close.addEventListener('click', function () { dialog.close(); });
    options.addCollection.addEventListener('click', function () {
      var collection = { name: 'New collection', icon: 'file-text', items: [] };
      collections.push(collection);
      select('collection', collection, collections, collection);
    });
    options.save.addEventListener('click', save);
    dialog.addEventListener('click', function (event) {
      if (event.target === dialog) dialog.close();
    });
    return { open: open };
  }

  window.wbConfigEditor = { create: create, pageViewports: pageViewports };
})();
