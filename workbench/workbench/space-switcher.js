/* The space switcher
   ------------------
   Which space is showing, and the others to switch to: a button with the
   space's mark and name, and under it a menu of every space, the current
   one checked. Two hosts draw it — the Workbench view in VS Code's sidebar,
   and the canvas's own sidebar when it runs in a browser — and both get the
   spaces from somewhere else: the extension, or the server's
   /_workbench/spaces. This only draws them and says what was picked.

   A space arrives as { id, name, initial, color, icon, image, root,
   removable }. The mark is a coloured square holding the space's image,
   its Lucide icon, or the name's first letter; all of it comes with the
   space — from its workbench.yaml, or chosen from its folder — so it
   matches wherever the space is shown.

     wbSpaces.create({ button, menu, onPick, onAdd, onRemove, onToggle })
       button     the element that opens the menu; its contents are drawn here
       menu       an empty element the menu is drawn into
       onPick     called with a space's id when another space is picked
       onAdd      optional: draws Add a space… at the bottom
       onRemove   optional: called with an id from a removable row's button
       onToggle   optional: called with true and false as the menu opens
                  and closes
     → { set(spaces, currentId), toggle(open) }

     wbSpaces.mark(space) → the coloured square, for a breadcrumb
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

  /* The space's image, else its Lucide icon, else its initial — on its
     named or hex colour. An image with no colour stands on its own. */
  function mark(space) {
    space = space || {};
    var square = document.createElement('span');
    square.className = 'wb-space-mark';
    square.setAttribute('aria-hidden', 'true');
    var color = String(space.color || '');
    if (HEX.test(color)) {
      square.style.background = color;
      if (isLight(color)) square.classList.add('is-light');
    } else if (color) {
      square.dataset.color = color;
    } else {
      square.classList.add('is-bare');
    }
    if (space.image) {
      var image = document.createElement('img');
      image.src = space.image;
      image.alt = '';
      square.classList.add('has-image');
      square.appendChild(image);
    } else if (space.icon && window.wbIcon) {
      square.appendChild(window.wbIcon(space.icon, 14));
    } else {
      square.textContent = space.initial || '?';
    }
    return square;
  }

  function create(options) {
    var button = options.button;
    var menu = options.menu;
    var spaces = [];
    var current = null;
    var open = false;

    button.setAttribute('aria-haspopup', 'menu');
    button.setAttribute('aria-expanded', 'false');
    menu.setAttribute('role', 'menu');
    menu.setAttribute('aria-label', 'Spaces');
    menu.hidden = true;

    function items() {
      return Array.prototype.slice.call(menu.querySelectorAll('[data-space-item]'));
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
      var shown = spaces.filter(function (space) { return space.id === current; })[0] || null;
      button.innerHTML = '';
      if (shown) button.appendChild(mark(shown));
      var name = document.createElement('span');
      name.className = 'wb-spaces-name';
      name.textContent = shown ? shown.name : 'Workbench';
      button.appendChild(name);
      var caret = icon('chevrons-up-down', 14);
      caret.classList.add('wb-spaces-caret');
      button.appendChild(caret);
      button.title = shown ? shown.name + (shown.root ? ' — ' + shown.root : '') + '\nSwitch space' : 'Switch space';
    }

    function drawMenu() {
      menu.innerHTML = '';
      var heading = document.createElement('div');
      heading.className = 'wb-spaces-heading';
      heading.textContent = 'Spaces';
      heading.setAttribute('role', 'presentation');
      menu.appendChild(heading);

      spaces.forEach(function (space) {
        var row = document.createElement('div');
        row.className = 'wb-spaces-row';
        row.setAttribute('role', 'none');

        var pick = document.createElement('button');
        pick.type = 'button';
        pick.className = 'wb-spaces-item';
        pick.dataset.spaceItem = '';
        pick.setAttribute('role', 'menuitemradio');
        pick.setAttribute('aria-checked', String(space.id === current));
        pick.title = space.root || space.name;
        pick.appendChild(mark(space));
        var label = document.createElement('span');
        label.className = 'wb-spaces-label';
        label.textContent = space.name;
        pick.appendChild(label);
        var check = icon('check', 14);
        check.classList.add('wb-spaces-check');
        pick.appendChild(check);
        pick.addEventListener('click', function () {
          close(true);
          if (space.id !== current) options.onPick(space.id);
        });
        row.appendChild(pick);

        if (space.removable && options.onRemove) {
          var remove = document.createElement('button');
          remove.type = 'button';
          remove.className = 'wb-spaces-remove';
          remove.dataset.spaceItem = '';
          remove.setAttribute('role', 'menuitem');
          remove.setAttribute('aria-label', 'Remove ' + space.name + ' from Workbench');
          remove.title = 'Remove from Workbench';
          remove.appendChild(icon('x', 14));
          remove.addEventListener('click', function () {
            close(true);
            options.onRemove(space.id);
          });
          row.appendChild(remove);
        }
        menu.appendChild(row);
      });

      if (options.onAdd) {
        var rule = document.createElement('div');
        rule.className = 'wb-spaces-rule';
        rule.setAttribute('role', 'separator');
        menu.appendChild(rule);

        var add = document.createElement('button');
        add.type = 'button';
        add.className = 'wb-spaces-item wb-spaces-add';
        add.dataset.spaceItem = '';
        add.setAttribute('role', 'menuitem');
        add.appendChild(icon('plus', 16));
        var text = document.createElement('span');
        text.className = 'wb-spaces-label';
        text.textContent = 'Add a space…';
        add.appendChild(text);
        add.addEventListener('click', function () {
          close(true);
          options.onAdd();
        });
        menu.appendChild(add);
      }
    }

    function set(next, currentId) {
      spaces = (next || []).slice();
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

  window.wbSpaces = { create: create, mark: mark };
})();
