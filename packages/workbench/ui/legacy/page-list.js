/* The page list
   -------------
   Built the way Sketch's sidebar is: the config's collections listed on top,
   as Sketch lists its pages, and the chosen collection's pages below —
   groups, states, and the filter over all of them.

   It is its own file because two places need the same list. In a browser it
   is the workbench's own left edge. In an editor it is the Canonic sidebar, a
   webview with VS Code's colours on it, standing where the file tree usually
   does. Same rows, same folds, same filter; what differs is what a pick means,
   and that arrives as onPick.

   So nothing here knows about hashes, frames, or editors. It is handed the
   config's collections and answers with a list that calls onPick(src, state) —
   state being null for the page as authored. Everything about what to *do*
   with that stays with the host.
*/

(function () {
  function isGroup(entry) {
    return !!(entry && entry.group);
  }

  /* A collection's entries are pages, groups of pages, or both. Everything
     downstream works on pages, so this is the one place that walks in. */
  function pagesIn(entries) {
    var out = [];
    entries.forEach(function (entry) {
      if (isGroup(entry)) out = out.concat(pagesIn(entry.items));
      else out.push(entry);
    });
    return out;
  }

  /* A single state is a page with a note attached, not a fold — the panel
     only splits a page open when there is a choice to make. */
  function statesOf(item) {
    return item && item.states && item.states.length > 1 ? item.states : null;
  }

  /* An authored page's state header carries two actions in one row: its
     caret folds, while its label selects the page. Imported titles are
     disclosure-only because their named states are the actual pages. */
  function stateHeadAction(target, selected, filtering, disclosureOnly) {
    var caret = target && target.closest && target.closest('.wb-fold');
    return disclosureOnly || caret || (selected && !filtering) ? 'toggle' : 'pick';
  }

  function statePick(item, state, index) {
    return item && item.implementationOnly ? state.id : index === 0 ? null : state.id;
  }

  function emptyText(filter) {
    return (filter || '').trim()
      ? 'Nothing matches “' + filter + '”.'
      : 'No pages available.';
  }

  /* src -> item, with each page told which collection it came from. A row wears
     its collection's glyph — the icon says which kind of thing the row is, the
     label says which one — so the item carries that too. */
  function index(collections) {
    var out = {};
    (collections || []).forEach(function (g) {
      pagesIn(g.items).forEach(function (item) {
        item.collection = g.name;
        item.icon = item.icon || g.icon;
        out[item.src] = item;
      });
    });
    return out;
  }

  /* options: list — where the pages go; collectionList — where the collections
     go, with collectionsBlock, the part of the panel to hide when there is only
     one; title — the heading over the pages, which names the collection showing;
     search — the filter; collections — the config's collections; onPick(src, state) —
     what a picked row does; and treeKeyboard, when the host wants the list to
     behave like a tree with one tab stop and arrow-key focus. Only list and
     collections are required. */
  function create(options) {
    var search = options.search || null;
    var pageList = options.list;
    var collectionList = options.collectionList || null;
    var collectionsBlock = options.collectionsBlock || null;
    var title = options.title || null;
    var collections = options.collections || [];
    var onPick = options.onPick || function () {};
    var treeKeyboard = !!options.treeKeyboard;

    var showing = collections.length ? collections[0].name : null; /* the collection showing */
    var current = null;      /* src of the picked page */
    var currentState = null; /* its state id, or null for the default one */

    /* ------------------------------------------------------ collections */

    /* Like the pages list above Sketch's layer list: every collection, with how
       many pages it holds; the one showing is tinted, and its pages are
       listed below. A config with one collection has nothing to choose, so the
       list steps aside. */
    function renderCollections() {
      if (!collectionList) return;
      collectionList.innerHTML = '';
      var several = collections.length > 1;
      if (collectionsBlock) collectionsBlock.hidden = !several;
      if (!several) return;
      collections.forEach(function (g) {
        var row = document.createElement('button');
        row.className = 'wb-collection-row';
        row.type = 'button';
        row.dataset.collection = g.name;
        if (g.icon) row.appendChild(window.wbIcon(g.icon, 14));
        row.appendChild(labelFor(g.name));
        var count = document.createElement('span');
        count.className = 'wb-count';
        count.textContent = String(pagesIn(g.items).length);
        row.appendChild(count);
        row.addEventListener('click', function () {
          setCollection(g.name);
        });
        collectionList.appendChild(row);
      });
      markCollection();
    }

    function markCollection() {
      if (title) title.textContent = filtering ? 'Results' : showing || '';
      if (!collectionList) return;
      Array.prototype.forEach.call(collectionList.children, function (row) {
        row.setAttribute('aria-current', String(row.dataset.collection === showing));
      });
    }

    function setCollection(name) {
      showing = name;
      render(search ? search.value : '');
    }

    /* ------------------------------------------------------------ panel */

    /* A long name ellipsizes rather than wrapping the row. */
    function labelFor(text) {
      var span = document.createElement('span');
      span.className = 'wb-item-label';
      span.textContent = text;
      return span;
    }

    /* How far in a row sits. A tree row's highlight runs the panel's full
       width, so the indent is padding inside the row, read from --wb-depth. */
    function indent(el, depth) {
      if (depth) el.style.setProperty('--wb-depth', String(depth));
    }

    function itemButton(item, depth) {
      var button = document.createElement('button');
      button.className = 'wb-item';
      button.type = 'button';
      indent(button, depth);
      button.appendChild(window.wbIcon(item.icon, 14));
      button.appendChild(labelFor(item.label));
      button.title = item.label;
      button.dataset.src = item.src;
      button.setAttribute('aria-current', String(item.src === current));
      button.addEventListener('click', function () {
        onPick(item.src, null);
      });
      return button;
    }

    /* Folds the user worked by hand, keyed by the fold's own key. A fold left
       alone follows its default — a group starts open, a page's states open
       while that page is the one showing — and a hand fold outranks it.
       Filtering opens everything: a row that matched the search has to be
       visible to be picked, and a fold that can't close shouldn't be recorded
       for when the filter clears. */
    var opened = {};
    var filtering = false;

    function foldOpen(box) {
      if (filtering) return true;
      var key = box.dataset.foldKey;
      if (Object.prototype.hasOwnProperty.call(opened, key)) return opened[key];
      return box.dataset.foldDefault === 'open' || box.dataset.src === current;
    }

    /* Every fold in the panel is the same shape: a header row that turns its
       caret, and the rows it holds a step further in. What differs is what
       the header does when you click it, which the caller wires up. */
    function fold(spec) {
      var box = document.createElement('div');
      box.className = 'wb-fold-group ' + spec.className;
      box.dataset.foldKey = spec.key;
      if (spec.src) box.dataset.src = spec.src;
      if (spec.defaultOpen) box.dataset.foldDefault = 'open';

      var head = document.createElement('button');
      head.className = 'wb-item wb-fold-head';
      head.type = 'button';
      indent(head, spec.depth);
      /* Caret, then the row's glyph, then its name — the caret belongs to the
         fold and is the only part of the row that turns. */
      var caret = window.wbIcon('chevron-down', 14);
      caret.classList.add('wb-fold');
      head.appendChild(caret);
      if (spec.icon) head.appendChild(window.wbIcon(spec.icon, 14));
      head.appendChild(labelFor(spec.label));
      if (spec.count != null) {
        var count = document.createElement('span');
        count.className = 'wb-count';
        count.textContent = String(spec.count);
        head.appendChild(count);
      }

      var list = document.createElement('div');
      list.className = 'wb-fold-list';
      /* The guide beside the rows is drawn at the header's depth. */
      indent(list, spec.depth);
      list.id = 'wb-fold-' + spec.key.replace(/[^a-z0-9]+/gi, '-');
      head.setAttribute('aria-controls', list.id);

      box.appendChild(head);
      box.appendChild(list);
      return { box: box, head: head, list: list };
    }

    function toggleFold(box) {
      opened[box.dataset.foldKey] = !foldOpen(box);
      markCurrent();
    }

    /* A group of pages that belong to one flow. Its header only folds —
       there is nothing to put on the canvas for "Auth" itself. */
    function groupFold(entry, g, depth) {
      var parts = fold({
        className: 'wb-group',
        key: 'group:' + g.name + '/' + entry.group,
        label: entry.group,
        icon: 'folder',
        defaultOpen: true,
        depth: depth,
      });

      parts.head.dataset.group = entry.group;
      parts.head.addEventListener('click', function () {
        toggleFold(parts.box);
      });

      entry.items.forEach(function (item) {
        parts.list.appendChild(rowFor(item, depth + 1));
      });

      return parts.box;
    }

    /* A page with states: the page's own row, then one row per state, the
       way Storybook stacks a component's stories. */
    function stateGroup(item, states, depth) {
      var parts = fold({
        className: 'wb-states',
        key: item.src,
        src: item.src,
        label: item.label,
        icon: item.icon,
        depth: depth,
      });

      parts.head.dataset.src = item.src;
      parts.head.title = item.label;

      states.forEach(function (state, i) {
        var row = document.createElement('button');
        row.className = 'wb-item wb-state';
        row.type = 'button';
        indent(row, depth + 1);
        /* Same glyph on every state row, on every page: these rows are all
           the same kind of thing, and the label is what tells them apart. */
        row.appendChild(window.wbIcon('circle-dot', 13));
        row.appendChild(labelFor(state.label));
        row.title = state.label;
        row.dataset.src = item.src;
        row.dataset.state = state.id;
        row.addEventListener('click', function () {
          onPick(item.src, statePick(item, state, i));
        });
        parts.list.appendChild(row);
      });

      /* An imported implementation has no page apart from its named states,
         so its title is disclosure-only. Authored pages retain the shortcut
         where their title picks the default state and folds when reselected. */
      parts.head.addEventListener('click', function (e) {
        if (stateHeadAction(
          e.target,
          current === item.src,
          filtering,
          !!item.implementationOnly
        ) === 'toggle') {
          toggleFold(parts.box);
          return;
        }
        opened[item.src] = true;
        onPick(item.src, null);
      });

      return parts.box;
    }

    function rowFor(item, depth) {
      var states = statesOf(item);
      return states ? stateGroup(item, states, depth) : itemButton(item, depth);
    }

    /* Group and state labels are searchable too, so "auth" or "wrong password"
       finds the pages they belong to rather than nothing. */
    function haystack(item) {
      var text = item.label + ' ' + item.src;
      (item.states || []).forEach(function (state) {
        text += ' ' + state.label;
      });
      return text.toLowerCase();
    }

    /* A group survives the filter whole if its own name matched — you asked
       for the flow, not for one page in it — and otherwise keeps the pages
       that matched. */
    function matching(entry, q) {
      if (!q) return entry;
      if (isGroup(entry)) {
        if (entry.group.toLowerCase().indexOf(q) > -1) return entry;
        var kept = entry.items.map(function (child) {
          return matching(child, q);
        }).filter(Boolean);
        return kept.length ? { group: entry.group, items: kept } : null;
      }
      return haystack(entry).indexOf(q) > -1 ? entry : null;
    }

    /* The collection showing, alone. A filter searches every collection instead,
       each under a small label, so typing "button" from Pages still finds
       the component. */
    function render(filter) {
      var q = (filter || '').trim().toLowerCase();
      pageList.innerHTML = '';
      filtering = !!q;
      var shown = 0;

      collections.forEach(function (g) {
        if (!q && g.name !== showing) return;
        var matches = g.items.map(function (entry) {
          return matching(entry, q);
        }).filter(Boolean);
        if (!matches.length) return;
        shown += pagesIn(matches).length;

        var block = document.createElement('div');
        block.className = 'wb-collection-results';
        if (q && collections.length > 1) {
          var label = document.createElement('p');
          label.className = 'wb-collection-label';
          label.textContent = g.name;
          block.appendChild(label);
        }
        matches.forEach(function (entry) {
          block.appendChild(isGroup(entry) ? groupFold(entry, g, 0) : rowFor(entry, 0));
        });
        pageList.appendChild(block);
      });

      markCollection();
      markCurrent();

      if (!shown) {
        var empty = document.createElement('p');
        empty.className = 'wb-empty';
        empty.textContent = emptyText(filter);
        pageList.appendChild(empty);
      }
    }

    /* Marks the picked row and opens or closes the folds, without rebuilding
         the list — a rebuild would drop focus mid-keyboard-walk.

       Only one row lights up. On a page with states that is the state row:
       the page's own row above it is a header, so it takes the weight of the
       current page but not the selected background. */
    function markCurrent() {
      Array.prototype.forEach.call(pageList.querySelectorAll('.wb-item'), function (b) {
        var mine = b.dataset.src === current;
        b.setAttribute(
          'aria-current',
          String(b.dataset.state ? mine && b.dataset.state === (currentState || 'default') : mine)
        );
      });

      Array.prototype.forEach.call(pageList.querySelectorAll('.wb-fold-group'), function (box) {
        var open = foldOpen(box);
        box.querySelector('.wb-fold-head').setAttribute('aria-expanded', String(open));
        box.querySelector('.wb-fold-list').hidden = !open;
      });

      syncTabStop();
    }

    /* A VS Code tree is one stop in the tab order. Once it has focus, arrows
       move the focus outline without changing the selected page; Enter or
       Space activates the focused button through its native button behaviour.
       That separation is what lets the current row stay blue while the user
       inspects a neighbouring row with the keyboard. */
    function visibleRows() {
      return Array.prototype.filter.call(pageList.querySelectorAll('.wb-item'), function (row) {
        return !row.closest('[hidden]');
      });
    }

    function selectedRow(rows) {
      var header = null;
      for (var i = 0; i < rows.length; i++) {
        if (rows[i].getAttribute('aria-current') !== 'true') continue;
        if (!rows[i].classList.contains('wb-fold-head')) return rows[i];
        if (!header) header = rows[i];
      }
      return header;
    }

    function syncTabStop() {
      if (!treeKeyboard) return;
      var rows = visibleRows();
      var focused = rows.indexOf(document.activeElement) > -1 ? document.activeElement : null;
      var target = focused || selectedRow(rows) || rows[0] || null;

      Array.prototype.forEach.call(pageList.querySelectorAll('.wb-item'), function (row) {
        row.tabIndex = row === target ? 0 : -1;
      });
    }

    function focusRow(row) {
      if (!row) return;
      Array.prototype.forEach.call(pageList.querySelectorAll('.wb-item'), function (candidate) {
        candidate.tabIndex = candidate === row ? 0 : -1;
      });
      row.focus();
    }

    function foldHead(box) {
      return box && box.firstElementChild && box.firstElementChild.classList.contains('wb-fold-head')
        ? box.firstElementChild
        : null;
    }

    function parentHead(row) {
      var list = row.parentElement && row.parentElement.closest('.wb-fold-list');
      return list ? foldHead(list.parentElement) : null;
    }

    function moveByKey(e) {
      if (!treeKeyboard) return;
      var row = e.target.closest && e.target.closest('.wb-item');
      if (!row || !pageList.contains(row)) return;

      var rows = visibleRows();
      var at = rows.indexOf(row);
      var next = null;

      if (e.key === 'ArrowDown') next = rows[Math.min(rows.length - 1, at + 1)];
      else if (e.key === 'ArrowUp') next = rows[Math.max(0, at - 1)];
      else if (e.key === 'Home') next = rows[0];
      else if (e.key === 'End') next = rows[rows.length - 1];
      else if (e.key === 'ArrowRight' && row.classList.contains('wb-fold-head')) {
        if (row.getAttribute('aria-expanded') === 'false') {
          toggleFold(row.parentElement);
          next = row;
        } else {
          var childList = row.nextElementSibling;
          next = rows.filter(function (candidate) {
            return childList && childList.contains(candidate);
          })[0] || row;
        }
      } else if (e.key === 'ArrowLeft') {
        if (row.classList.contains('wb-fold-head') && row.getAttribute('aria-expanded') === 'true') {
          toggleFold(row.parentElement);
          next = row;
        } else {
          next = parentHead(row) || row;
        }
      }

      if (!next) return;
      e.preventDefault();
      focusRow(next);
    }

    /* Navigation that didn't start here — a link followed in the preview, a
       page picked in the other half of the tool — can land inside a group
       the user collapsed. Open every ancestor plus the state fold so the
       active row is visible, not merely selected somewhere out of sight. */
    function reveal(src) {
      collections.forEach(function (collection) {
        function visit(entries, parents) {
          entries.forEach(function (entry) {
            if (entry.group && entry.items) {
              visit(entry.items, parents.concat('group:' + collection.name + '/' + entry.group));
              return;
            }
            if (entry.src !== src) return;
            parents.forEach(function (key) {
              opened[key] = true;
            });
            if (statesOf(entry)) opened[src] = true;
          });
        }
        visit(collection.items, []);
      });
    }

    /* What the host calls when the selection changes, wherever it changed.
       A new page brings its collection into view — a linked hash, or a pick
       made in the other half of the tool — but the same page reported
       again leaves the collection you went browsing in alone. */
    function setCurrent(src, state, collection) {
      var moved = (src || null) !== current;
      current = src || null;
      currentState = state || null;
      if (moved && current && collection && collection !== showing) setCollection(collection);
      else markCurrent();
    }

    /* A manifest can change while the workbench stays open. Replace the data
       behind the list without replacing this controller, so its search and
       keyboard listeners remain single and its hand-worked folds survive. */
    function update(nextCollections) {
      collections = nextCollections || [];
      var stillThere = collections.some(function (g) { return g.name === showing; });
      if (!stillThere) showing = collections.length ? collections[0].name : null;
      renderCollections();
      render(search ? search.value : '');
    }

    /* -------------------------------------------------------------- boot */

    if (search) {
      search.addEventListener('input', function () {
        render(search.value);
      });

      search.addEventListener('keydown', function (e) {
        if (e.key === 'ArrowDown' && treeKeyboard) {
          e.preventDefault();
          focusRow(visibleRows()[0]);
          return;
        }
        if (e.key === 'Escape') {
          search.value = '';
          render('');
        }
      });
    }

    pageList.addEventListener('keydown', moveByKey);

    renderCollections();
    render('');

    return {
      setCurrent: setCurrent,
      reveal: reveal,
      update: update,
    };
  }

  window.wbPageList = {
    index: index,
    create: create,
    states: statesOf,
    statePick: statePick,
    pagesIn: pagesIn,
    isGroup: isGroup,
  };
})();
