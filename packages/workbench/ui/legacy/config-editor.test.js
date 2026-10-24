var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function editor() {
  var context = { window: {} };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, './config-editor.js'), 'utf8'), context);
  return context.window.wbConfigEditor;
}

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

test('a new group has a name and one page, so it can be saved as it is', function () {
  var configEditor = editor();
  var group = configEditor.newGroup();
  assert.deepEqual(plain(group), {
    group: 'New group',
    items: [{ label: 'Untitled page', src: 'pages/untitled.html' }],
  });
  assert.equal(configEditor.validate([{ name: 'Pages', items: [group] }]), null);
});

test('new groups are separate objects, so editing one leaves the next alone', function () {
  var configEditor = editor();
  var first = configEditor.newGroup();
  first.group = 'Authentication';
  first.items[0].label = 'Sign in';
  assert.equal(configEditor.newGroup().group, 'New group');
  assert.equal(configEditor.newGroup().items[0].label, 'Untitled page');
});

test('save refuses a group without a name', function () {
  var validate = editor().validate;
  var page = { label: 'Sign in', src: 'pages/sign-in.html' };
  assert.equal(
    validate([{ name: 'Pages', items: [{ group: '', items: [page] }] }]),
    'Every group needs a name and at least one page.'
  );
  assert.equal(
    validate([{ name: 'Pages', items: [{ group: '   ', items: [page] }] }]),
    'Every group needs a name and at least one page.'
  );
});

test('save refuses a group whose pages were all removed', function () {
  var validate = editor().validate;
  assert.equal(
    validate([{ name: 'Pages', items: [{ group: 'Authentication', items: [] }] }]),
    'Every group needs a name and at least one page.'
  );
});

test('save checks the pages inside a group', function () {
  var validate = editor().validate;
  assert.equal(
    validate([{ name: 'Pages', items: [{ group: 'Authentication', items: [{ label: 'Sign in', src: '' }] }] }]),
    'Every page needs a label and source path.'
  );
});

test('save still requires collection names and page fields', function () {
  var validate = editor().validate;
  assert.equal(validate([{ name: ' ', items: [] }]), 'Collection 1 needs a name.');
  assert.equal(
    validate([{ name: 'Pages', items: [{ label: 'Home', src: 'index.html' }, { label: '', src: 'a.html' }] }]),
    'Every page needs a label and source path.'
  );
  assert.equal(validate([{ name: 'Pages', items: [{ label: 'Home', src: 'index.html' }] }]), null);
});
