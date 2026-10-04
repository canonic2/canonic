/* YAML, the part of it a config file needs
   ----------------------------------------
   The workbench has no build step and no dependencies — it is HTML and script
   files a browser opens — so reading workbench.yaml means parsing it
   here. This is not a YAML implementation. It is block mappings, block
   sequences, scalars, quotes and comments, which is the whole of what a
   workbench config is written in:

     name: Acme
     collections:
       - name: Pages
         icon: file-text
         items:
           - label: Sign in           # a mapping opened by its first key
             src: pages/sign-in.html
             states:
               - id: default
                 label: Default

   What it deliberately doesn't do: anchors, aliases, tags, multi-line scalars,
   flow mappings, multiple documents. Anything it doesn't understand it reports
   with a line number rather than guessing — a config that half-parses is worse
   than one that refuses.

   Indentation is the structure, so tabs are rejected outright: a tab that
   looks like an indent in the editor and isn't one to the parser is exactly
   the kind of bug that costs an afternoon.
*/

(function () {
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

  /* Strings stay strings — every value in a workbench config is one — except
     the three literals and plain numbers, which are worth having as
     themselves the day a config wants a width or a flag. */
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
     Blank and comment-only lines are dropped here so nothing downstream has to
     keep stepping over them, and the original line number rides along so an
     error can point at the file the author is looking at. */
  function significant(text) {
    var lines = [];
    text.split(/\r?\n/).forEach(function (raw, i) {
      var line = { number: i, raw: raw };
      if (raw.indexOf('\t') > -1 && /^\s*\t/.test(raw)) {
        fail(line, 'indented with a tab — YAML indents with spaces.');
      }
      var body = withoutComment(raw);
      if (!body.trim()) return;
      line.indent = indentOf(body);
      line.text = body.slice(line.indent).replace(/\s+$/, '');
      lines.push(line);
    });
    return lines;
  }

  /* One block — a mapping or a sequence — of everything indented at `indent`
     or deeper, starting at `at`. Answers with the value and the line after it.

     A sequence entry that opens a mapping on its own dash line ("- id: x") is
     handled by treating the dash as two spaces of indent: the entry's keys are
     then an ordinary mapping starting two columns in, which is exactly what
     they are. */
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

      /* "- key: value" and "- key:" open a mapping whose first key sits where
         `rest` starts; everything else on this dash is a plain value. */
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

      /* An empty value is either a block underneath, or nothing at all. */
      var nested = block(lines, i + 1, indent + 1);
      out[key] = nested[0] === null ? '' : nested[0];
      i = nested[1];
    }

    return [out, i];
  }

  /* The document, as a plain object. Throws with a line number on anything it
     can't read. */
  window.wbYaml = function (text) {
    var lines = significant(String(text || ''));
    if (!lines.length) return {};
    if (lines[0].indent !== 0) fail(lines[0], 'the document starts indented.');

    var parsed = block(lines, 0, 0);
    if (parsed[1] < lines.length) {
      fail(lines[parsed[1]], 'this line is indented in a way the block above doesn’t account for.');
    }
    return parsed[0];
  };
})();
