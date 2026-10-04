/* The project switcher
   --------------------
   Which workbench is showing, and the others to switch to: a button with the
   project's mark and name, and under it a menu of every project, the current
   one checked. Two hosts draw it — the Workbench view in VS Code's sidebar,
   and the canvas's own sidebar when it runs in a browser — and both get the
   projects from somewhere else: the extension, or the server's
   /_workbench/projects. This only draws them and says what was picked.

   A project arrives as { id, name, initial, color, icon, image, root,
   removable }. The mark is a coloured square holding the project's image,
   its Lucide icon, or the name's first letter; all of it comes with the
   project — from its workbench.yaml, or chosen from its folder — so it
   matches wherever the project is shown.

     wbProjects.create({ button, menu, onPick, onAdd, onRemove, onToggle })
       button     the element that opens the menu; its contents are drawn here
       menu       an empty element the menu is drawn into
       onPick     called with a project's id when another project is picked
       onAdd      optional: draws Add a project… at the bottom
       onRemove   optional: called with an id from a removable row's button
       onToggle   optional: called with true and false as the menu opens
                  and closes
     → { set(projects, currentId), toggle(open) }

     wbProjects.mark(project) → the coloured square, for a breadcrumb
*/

(function () {
  function icon(name, size) {
    return window.wbIcon ? window.wbIcon(name, size) : document.createElement('span');
  }

  var HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

  /* A light custom colour takes dark text, by its relative luminance. */
  function isLight(hex) {
    var digits = hex.slice(1);
    if (digits.length === 3) digits = digits.replace(/./g, '$&$&');
    var channels = [0, 2, 4].map(function (at) {
      var value = parseInt(digits.slice(at, at + 2), 16) / 255;
      return value <= 0.03928 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2] > 0.4;
  }

  /* The project's image, else its Lucide icon, else its initial — on its
     named or hex colour. An image with no colour stands on its own. */
  function mark(project) {
    project = project || {};
    var square = document.createElement('span');
    square.className = 'wb-project-mark';
    square.setAttribute('aria-hidden', 'true');
    var color = String(project.color || '');
    if (HEX.test(color)) {
      square.style.background = color;
      if (isLight(color)) square.classList.add('is-light');
    } else if (color) {
      square.dataset.color = color;
    } else {
      square.classList.add('is-bare');
    }
    if (project.image) {
      var image = document.createElement('img');
      image.src = project.image;
      image.alt = '';
      square.classList.add('has-image');
      square.appendChild(image);
    } else if (project.icon && window.wbIcon) {
      square.appendChild(window.wbIcon(project.icon, 14));
    } else {
      square.textContent = project.initial || '?';
    }
    return square;
  }

  function create(options) {
    var button = options.button;
    var menu = options.menu;
    var projects = [];
    var current = null;
    var open = false;

    button.setAttribute('aria-haspopup', 'menu');
    button.setAttribute('aria-expanded', 'false');
    menu.setAttribute('role', 'menu');
    menu.setAttribute('aria-label', 'Projects');
    menu.hidden = true;

    function items() {
      return Array.prototype.slice.call(menu.querySelectorAll('[data-project-item]'));
    }

    /* Opened from the keyboard, the menu takes focus so the arrows work;
       opened with the pointer, it leaves focus alone rather than ringing a
       row nobody tabbed to. */
    function toggle(next, fromKeyboard) {
      next = !!next;
      if (next === open) return;
      open = next;
      menu.hidden = !open;
      button.setAttribute('aria-expanded', String(open));
      if (options.onToggle) options.onToggle(open);
      if (open && fromKeyboard) {
        var checked = menu.querySelector('[aria-checked="true"]') || items()[0];
        if (checked) checked.focus();
      }
    }

    function close(focusButton) {
      if (!open) return;
      toggle(false);
      if (focusButton) button.focus();
    }

    function drawButton() {
      var shown = projects.filter(function (project) { return project.id === current; })[0] || null;
      button.innerHTML = '';
      if (shown) button.appendChild(mark(shown));
      var name = document.createElement('span');
      name.className = 'wb-projects-name';
      name.textContent = shown ? shown.name : 'Workbench';
      button.appendChild(name);
      var caret = icon('chevrons-up-down', 14);
      caret.classList.add('wb-projects-caret');
      button.appendChild(caret);
      button.title = shown ? shown.name + (shown.root ? ' — ' + shown.root : '') + '\nSwitch project' : 'Switch project';
    }

    function drawMenu() {
      menu.innerHTML = '';
      var heading = document.createElement('div');
      heading.className = 'wb-projects-heading';
      heading.textContent = 'Projects';
      heading.setAttribute('role', 'presentation');
      menu.appendChild(heading);

      projects.forEach(function (project) {
        var row = document.createElement('div');
        row.className = 'wb-projects-row';
        row.setAttribute('role', 'none');

        var pick = document.createElement('button');
        pick.type = 'button';
        pick.className = 'wb-projects-item';
        pick.dataset.projectItem = '';
        pick.setAttribute('role', 'menuitemradio');
        pick.setAttribute('aria-checked', String(project.id === current));
        pick.title = project.root || project.name;
        pick.appendChild(mark(project));
        var label = document.createElement('span');
        label.className = 'wb-projects-label';
        label.textContent = project.name;
        pick.appendChild(label);
        var check = icon('check', 14);
        check.classList.add('wb-projects-check');
        pick.appendChild(check);
        pick.addEventListener('click', function () {
          close(true);
          if (project.id !== current) options.onPick(project.id);
        });
        row.appendChild(pick);

        if (project.removable && options.onRemove) {
          var remove = document.createElement('button');
          remove.type = 'button';
          remove.className = 'wb-projects-remove';
          remove.dataset.projectItem = '';
          remove.setAttribute('role', 'menuitem');
          remove.setAttribute('aria-label', 'Remove ' + project.name + ' from Workbench');
          remove.title = 'Remove from Workbench';
          remove.appendChild(icon('x', 14));
          remove.addEventListener('click', function () {
            close(true);
            options.onRemove(project.id);
          });
          row.appendChild(remove);
        }
        menu.appendChild(row);
      });

      if (options.onAdd) {
        var rule = document.createElement('div');
        rule.className = 'wb-projects-rule';
        rule.setAttribute('role', 'separator');
        menu.appendChild(rule);

        var add = document.createElement('button');
        add.type = 'button';
        add.className = 'wb-projects-item wb-projects-add';
        add.dataset.projectItem = '';
        add.setAttribute('role', 'menuitem');
        add.appendChild(icon('plus', 16));
        var text = document.createElement('span');
        text.className = 'wb-projects-label';
        text.textContent = 'Add a project…';
        add.appendChild(text);
        add.addEventListener('click', function () {
          close(true);
          options.onAdd();
        });
        menu.appendChild(add);
      }
    }

    function set(next, currentId) {
      projects = (next || []).slice();
      current = currentId || null;
      drawButton();
      drawMenu();
    }

    /* A click with no pointer position is Enter or Space on the button. */
    button.addEventListener('click', function (e) {
      toggle(!open, e.detail === 0);
    });

    button.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        if (!open) {
          toggle(true, true);
          return;
        }
        var checked = menu.querySelector('[aria-checked="true"]') || items()[0];
        if (checked) checked.focus();
      } else if (e.key === 'Escape' && open) {
        e.preventDefault();
        close(true);
      }
    });

    /* Up and down move through the rows, wrapping; Home and End jump to the
       ends; Escape and Tab close it. */
    menu.addEventListener('keydown', function (e) {
      var list = items();
      var at = list.indexOf(document.activeElement);
      var next = null;
      if (e.key === 'ArrowDown') next = list[(at + 1) % list.length];
      else if (e.key === 'ArrowUp') next = list[(at - 1 + list.length) % list.length];
      else if (e.key === 'Home') next = list[0];
      else if (e.key === 'End') next = list[list.length - 1];
      else if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        close(true);
        return;
      } else if (e.key === 'Tab') {
        close(false);
        return;
      }
      if (next) {
        e.preventDefault();
        next.focus();
      }
    });

    document.addEventListener('pointerdown', function (e) {
      if (open && !menu.contains(e.target) && !button.contains(e.target)) close(false);
    });

    window.addEventListener('blur', function () {
      close(false);
    });

    return { set: set, toggle: toggle };
  }

  window.wbProjects = { create: create, mark: mark };
})();
