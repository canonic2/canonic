/* A focused, master-detail editor for the handwritten sections in
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

  function screenViewports(screen) {
    return window.wbManifest.screenViewports(screen.viewports, screen.label || 'Screen', []);
  }

  function canonicalize(entries) {
    (entries || []).forEach(function (entry) {
      if (entry.folder) {
        canonicalize(entry.items);
        return;
      }
      var selected = screenViewports(entry);
      if (selected.length === VIEWPORTS.length) delete entry.viewports;
      else entry.viewports = selected;
    });
  }

  function create(options) {
    var dialog = options.dialog;
    var body = options.body;
    var status = options.status;
    var sections = [];
    var selection = null;
    var navPane = null;
    var inspector = null;

    function setStatus(message, error) {
      status.textContent = message || '';
      status.classList.toggle('is-error', !!error);
    }

    function pageCount(entries) {
      return (entries || []).reduce(function (count, entry) {
        return count + (entry.folder ? pageCount(entry.items) : 1);
      }, 0);
    }

    function totalPages() {
      return sections.reduce(function (count, section) { return count + pageCount(section.items); }, 0);
    }

    function select(kind, value, owner, section) {
      selection = { kind: kind, value: value, owner: owner, section: section || value };
      renderNavigation();
      renderInspector();
    }

    function selected(value) {
      return !!selection && selection.value === value;
    }

    function addPage(owner, section) {
      var page = { label: 'Untitled page', src: 'pages/untitled.html' };
      owner.push(page);
      select('page', page, owner, section);
    }

    function removeSelected(message) {
      if (!selection || !window.confirm(message)) return;
      var owner = selection.owner;
      var at = owner.indexOf(selection.value);
      if (at !== -1) owner.splice(at, 1);
      var section = selection.section;
      selection = section && sections.indexOf(section) !== -1
        ? { kind: 'section', value: section, owner: sections, section: section }
        : sections.length ? { kind: 'section', value: sections[0], owner: sections, section: sections[0] } : null;
      render();
    }

    function navPage(page, owner, section, nested) {
      var row = element('button', 'wb-config-nav-page' + (nested ? ' is-nested' : ''));
      row.type = 'button';
      row.setAttribute('aria-current', String(selected(page)));
      row.appendChild(icon(page.icon || 'file-text', 15));
      var copy = element('span', 'wb-config-nav-copy');
      copy.appendChild(element('strong', '', page.label || 'Untitled page'));
      copy.appendChild(element('small', '', page.src || 'No source path'));
      row.appendChild(copy);
      row.addEventListener('click', function () { select('page', page, owner, section); });
      return row;
    }

    function navFolder(folder, owner, section) {
      var group = element('div', 'wb-config-nav-folder');
      var head = element('button', 'wb-config-nav-folder-head');
      head.type = 'button';
      head.setAttribute('aria-current', String(selected(folder)));
      head.appendChild(icon('folder', 15));
      head.appendChild(element('strong', '', folder.folder || 'Untitled folder'));
      head.appendChild(element('small', '', String(pageCount(folder.items))));
      head.addEventListener('click', function () { select('folder', folder, owner, section); });
      group.appendChild(head);
      (folder.items || []).forEach(function (page) {
        group.appendChild(navPage(page, folder.items, section, true));
      });
      return group;
    }

    function navSection(section) {
      var group = element('section', 'wb-config-nav-section');
      var head = element('div', 'wb-config-nav-section-head');
      var pick = element('button', 'wb-config-nav-section-pick');
      pick.type = 'button';
      pick.setAttribute('aria-current', String(selected(section)));
      pick.appendChild(element('span', '', section.name || 'Untitled section'));
      pick.appendChild(element('small', '', String(pageCount(section.items))));
      pick.addEventListener('click', function () { select('section', section, sections, section); });
      head.appendChild(pick);
      head.appendChild(iconButton('Add page to ' + (section.name || 'section'), 'plus', function () {
        addPage(section.items || (section.items = []), section);
      }, 'wb-config-nav-add'));
      group.appendChild(head);
      (section.items || []).forEach(function (entry) {
        group.appendChild(entry.folder
          ? navFolder(entry, section.items, section)
          : navPage(entry, section.items, section, false));
      });
      return group;
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
      if (!sections.length) list.appendChild(element('p', 'wb-config-nav-empty', 'No handwritten sections yet.'));
      else sections.forEach(function (section) { list.appendChild(navSection(section)); });
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
      var active = screenViewports(page);
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

    function renderSection(section) {
      inspector.appendChild(inspectorHeader('Section', section.name || 'Untitled section', pageCount(section.items) + ' pages', section.icon || 'layers-3'));
      var form = element('div', 'wb-config-inspector-form');
      form.appendChild(field('Section name', section.name, function (value) {
        section.name = value;
        renderNavigation();
      }, { placeholder: 'Pages' }));
      form.appendChild(field('Lucide icon', section.icon, function (value) { section.icon = value; }, {
        placeholder: 'file-text', help: 'A kebab-case Lucide icon name.',
      }));
      inspector.appendChild(form);
      var primary = element('button', 'wb-config-inspector-action');
      primary.type = 'button';
      primary.appendChild(icon('plus', 15));
      primary.appendChild(document.createTextNode('Add page to this section'));
      primary.addEventListener('click', function () { addPage(section.items || (section.items = []), section); });
      inspector.appendChild(primary);
      inspector.appendChild(danger('Remove section', function () {
        removeSelected('Remove “' + (section.name || 'this section') + '” and all of its pages?');
      }));
    }

    function renderFolder(folder) {
      inspector.appendChild(inspectorHeader('Folder', folder.folder || 'Untitled folder', pageCount(folder.items) + ' pages', 'folder'));
      var form = element('div', 'wb-config-inspector-form');
      form.appendChild(field('Folder name', folder.folder, function (value) {
        folder.folder = value;
        renderNavigation();
      }, { placeholder: 'Authentication' }));
      inspector.appendChild(form);
      var primary = element('button', 'wb-config-inspector-action');
      primary.type = 'button';
      primary.appendChild(icon('plus', 15));
      primary.appendChild(document.createTextNode('Add page to this folder'));
      primary.addEventListener('click', function () { addPage(folder.items || (folder.items = []), selection.section); });
      inspector.appendChild(primary);
      inspector.appendChild(danger('Remove folder', function () {
        removeSelected('Remove “' + (folder.folder || 'this folder') + '” and all of its pages?');
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
      else if (selection.kind === 'folder') renderFolder(selection.value);
      else renderSection(selection.value);
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
        sections = result.sections || [];
        selection = sections.length ? { kind: 'section', value: sections[0], owner: sections, section: sections[0] } : null;
        setStatus('');
        render();
      }).catch(function (error) { setStatus(String(error.message || error), true); });
    }

    function validate() {
      var problem = null;
      sections.some(function (section, index) {
        if (!String(section.name || '').trim()) { problem = 'Section ' + (index + 1) + ' needs a name.'; return true; }
        function visit(entries) {
          return (entries || []).some(function (entry) {
            if (entry.folder) return visit(entry.items);
            if (!String(entry.label || '').trim() || !String(entry.src || '').trim()) {
              problem = 'Every page needs a label and source path.';
              return true;
            }
            return false;
          });
        }
        return visit(section.items);
      });
      return problem;
    }

    function save() {
      var problem = validate();
      if (problem) { setStatus(problem, true); return; }
      sections.forEach(function (section) { canonicalize(section.items); });
      options.save.disabled = true;
      setStatus('Saving…');
      fetch('/_workbench/config-file', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sections: sections }),
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
    options.addSection.addEventListener('click', function () {
      var section = { name: 'New section', icon: 'file-text', items: [] };
      sections.push(section);
      select('section', section, sections, section);
    });
    options.save.addEventListener('click', save);
    dialog.addEventListener('click', function (event) {
      if (event.target === dialog) dialog.close();
    });
    return { open: open };
  }

  window.wbConfigEditor = { create: create, screenViewports: screenViewports };
})();
