/* YAML, the part of it a workbench config needs
   ---------------------------------------------
   The same parser the workbench runs in the browser, as a module. Block
   mappings, block sequences, scalars, quotes and comments — the whole of what
   a workbench.yaml is written in, and nothing else: no anchors, aliases, tags,
   multi-line scalars, flow mappings or multiple documents.

   Two copies of this exist on purpose. The browser one can't be required from
   node, and pulling in a real YAML library would give this extension its first
   dependency for a file that is thirty lines of key-value. They parse the same
   subset; if one changes, change the other.

   Indentation is the structure, so tabs are rejected outright: a tab that
   looks like an indent in the editor and isn't one to the parser is exactly
   the kind of bug that costs an afternoon.
*/

/* A comment starts at a # that begins the line or follows a space, and only
   outside quotes — "#00a1ff" is a colour, not the start of a comment. */
function withoutComment(line) {
  var quote = null;
  for (var i = 0; i < line.length; i++) {
    var c = line.charAt(i);
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
    } else if (c === '#' && (i === 0 || line.charAt(i - 1) === ' ')) {
      return line.slice(0, i);
    }
  }
  return line;
}

function indentOf(line) {
  return line.length - line.replace(/^ +/, '').length;
}

/* Strings stay strings — nearly every value in a workbench config is one —
   except the three literals and plain numbers. */
function scalar(raw) {
  var text = raw.trim();
  if (!text) return '';
  var first = text.charAt(0);
  if ((first === '"' || first === "'") && text.charAt(text.length - 1) === first) {
    return text.slice(1, -1);
  }
  if (text === 'true') return true;
  if (text === 'false') return false;
  if (text === 'null' || text === '~') return null;
  if (/^-?\d+(\.\d+)?$/.test(text)) return Number(text);
  return text;
}

function fail(line, message) {
  throw new Error('line ' + (line.number + 1) + ': ' + message);
}

/* Every line that carries something, with the indent and text it carries.
   The original line number rides along so an error can point at the file the
   author is looking at. */
function significant(text) {
  var lines = [];
  text.split(/\r?\n/).forEach(function (raw, i) {
    var line = { number: i, raw: raw };
    if (/^\s*\t/.test(raw)) fail(line, 'indented with a tab — YAML indents with spaces.');
    var body = withoutComment(raw);
    if (!body.trim()) return;
    line.indent = indentOf(body);
    line.text = body.slice(line.indent).replace(/\s+$/, '');
    lines.push(line);
  });
  return lines;
}

/* One block — a mapping or a sequence — of everything indented at `indent` or
   deeper, starting at `at`. Answers with the value and the line after it. */
function block(lines, at, indent) {
  if (at >= lines.length || lines[at].indent < indent) return [null, at];
  return lines[at].text.charAt(0) === '-'
    ? sequence(lines, at, lines[at].indent)
    : mapping(lines, at, lines[at].indent);
}

function sequence(lines, at, indent) {
  var out = [];
  var i = at;

  while (i < lines.length && lines[i].indent === indent && lines[i].text.charAt(0) === '-') {
    var line = lines[i];
    var rest = line.text.slice(1);
    if (rest && rest.charAt(0) !== ' ') fail(line, 'a "-" starts a list item and needs a space after it.');
    rest = rest.replace(/^ +/, '');

    /* "- key: value" opens a mapping whose first key sits where `rest`
       starts: treat the dash as indent and the entry is an ordinary mapping,
       which is what it is. */
    if (/^[^\s:][^:]*:(\s|$)/.test(rest)) {
      var keyIndent = indent + (line.text.length - rest.length);
      var patched = lines.slice();
      patched[i] = { number: line.number, raw: line.raw, indent: keyIndent, text: rest };
      var pair = mapping(patched, i, keyIndent);
      out.push(pair[0]);
      i = pair[1];
      continue;
    }

    if (rest) {
      out.push(scalar(rest));
      i += 1;
      continue;
    }

    /* A bare dash with the entry underneath it. */
    var nested = block(lines, i + 1, indent + 1);
    out.push(nested[0]);
    i = nested[1];
  }

  return [out, i];
}

function mapping(lines, at, indent) {
  var out = {};
  var i = at;

  while (i < lines.length && lines[i].indent === indent) {
    var line = lines[i];
    if (line.text.charAt(0) === '-') break;

    var colon = line.text.indexOf(':');
    if (colon === -1) fail(line, 'expected "key: value" — there is no colon on this line.');

    var key = line.text.slice(0, colon).trim();
    var value = line.text.slice(colon + 1).trim();
    if (!key) fail(line, 'a key can’t be empty.');

    if (value) {
      out[key] = scalar(value);
      i += 1;
      continue;
    }

    var nested = block(lines, i + 1, indent + 1);
    out[key] = nested[0] === null ? '' : nested[0];
    i = nested[1];
  }

  return [out, i];
}

/* The document, as a plain object. Throws with a line number on anything it
   can't read. */
function parse(text) {
  var lines = significant(String(text || ''));
  if (!lines.length) return {};
  if (lines[0].indent !== 0) fail(lines[0], 'the document starts indented.');

  var parsed = block(lines, 0, 0);
  if (parsed[1] < lines.length) {
    fail(lines[parsed[1]], 'this line is indented in a way the block above doesn’t account for.');
  }
  return parsed[0];
}

function scalarText(value) {
  if (value === true) return 'true';
  if (value === false) return 'false';
  if (value === null) return 'null';
  if (typeof value === 'number' && Number.isFinite(value)) return String(value);
  var valueText = String(value === undefined ? '' : value);
  if (/\r|\n/.test(valueText)) throw new Error('Workbench YAML values must stay on one line.');
  var reserved = !valueText || /^(?:true|false|null|~|-?\d+(?:\.\d+)?)$/.test(valueText);
  var unsafe = reserved || /^[-?:]\s/.test(valueText) || /:\s|\s#|^#/.test(valueText) || /^\s|\s$/.test(valueText);
  if (!unsafe) return valueText;
  if (valueText.indexOf("'") === -1) return "'" + valueText + "'";
  if (valueText.indexOf('"') === -1) return '"' + valueText + '"';
  throw new Error('A workbench YAML value cannot contain both quote styles.');
}

function writeMap(value, indent) {
  var pad = new Array(indent + 1).join(' ');
  var out = [];
  Object.keys(value || {}).forEach(function (key) {
    var child = value[key];
    if (child !== null && typeof child === 'object') {
      out.push(pad + key + ':');
      out = out.concat(writeNode(child, indent + 2));
    } else {
      out.push(pad + key + ': ' + scalarText(child));
    }
  });
  return out;
}

function writeList(value, indent) {
  var pad = new Array(indent + 1).join(' ');
  var out = [];
  value.forEach(function (child) {
    if (Array.isArray(child)) {
      out.push(pad + '-');
      out = out.concat(writeNode(child, indent + 2));
      return;
    }
    if (child !== null && typeof child === 'object') {
      var keys = Object.keys(child);
      if (!keys.length) throw new Error('Empty maps are not supported in workbench YAML.');
      var first = keys[0];
      var firstValue = child[first];
      if (firstValue !== null && typeof firstValue === 'object') {
        out.push(pad + '- ' + first + ':');
        out = out.concat(writeNode(firstValue, indent + 4));
      } else {
        out.push(pad + '- ' + first + ': ' + scalarText(firstValue));
      }
      var rest = {};
      keys.slice(1).forEach(function (key) { rest[key] = child[key]; });
      out = out.concat(writeMap(rest, indent + 2));
      return;
    }
    out.push(pad + '- ' + scalarText(child));
  });
  return out;
}

function writeNode(value, indent) {
  return Array.isArray(value) ? writeList(value, indent) : writeMap(value, indent);
}

function stringify(value) {
  return writeNode(value || {}, 0).join('\n') + '\n';
}

module.exports = { parse: parse, stringify: stringify };
