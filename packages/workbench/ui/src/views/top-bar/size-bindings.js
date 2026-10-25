/* Existing dialog event wiring; size rules and persistence stay with their owners. */
window.wbSizeBindings = function (options) {
  var sizeSwitcher = document.getElementById('sizeSwitcher');
  var actionsToggle = document.getElementById('actionsToggle');
  var sizeDialog = document.getElementById('sizeDialog');
  var sizesEditor = document.getElementById('sizesEditor');
  sizeSwitcher.iconRenderer = window.wbIcon;
  actionsToggle.iconRenderer = window.wbIcon;
  sizeDialog.iconRenderer = window.wbIcon;
  sizesEditor.iconRenderer = window.wbIcon;
  sizeSwitcher.addEventListener('wb-size-pick', function (e) {
    options.choose(e.detail.key);
  });
  sizeSwitcher.addEventListener('wb-size-custom', function () {
    var context = options.context();
    sizeDialog.allowPageOnly = context.allowPageOnly;
    sizeDialog.notice = context.notice;
    sizeDialog.open = true;
  });
  sizeSwitcher.addEventListener('wb-size-edit', function () {
    sizesEditor.sizes = options.sizes();
    sizesEditor.open = true;
  });
  sizeDialog.addEventListener('wb-size-add', function (e) {
    var context = options.context();
    var size = Object.assign({}, e.detail, { page: context.page });
    sizeDialog.error = '';
    sizeDialog.pending = true;
    options.post({ action: 'add', size: size }).then(
      function (result) {
        sizeDialog.pending = false;
        sizeDialog.open = false;
        options.added(result, e.detail, context.docs);
      },
      function (error) {
        sizeDialog.pending = false;
        sizeDialog.error = String(error.message || error);
      },
    );
  });
  sizeDialog.addEventListener('wb-size-dialog-close', function () {
    sizeDialog.open = false;
  });
  sizesEditor.addEventListener('wb-sizes-save', function (e) {
    sizesEditor.error = '';
    sizesEditor.pending = true;
    options.post({ action: 'update', sizes: e.detail.sizes }).then(
      function () {
        sizesEditor.pending = false;
        sizesEditor.open = false;
        options.updated();
      },
      function (error) {
        sizesEditor.pending = false;
        sizesEditor.error = String(error.message || error);
      },
    );
  });
  sizesEditor.addEventListener('wb-sizes-close', function () {
    sizesEditor.open = false;
  });
};
