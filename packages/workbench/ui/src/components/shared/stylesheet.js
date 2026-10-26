/** Lazily parse a component's authored styles once per owner document. */
export function stylesFor(css) {
  const sheets = new WeakMap();
  return (document) => {
    let sheet = sheets.get(document);
    if (!sheet) {
      const Sheet = document.defaultView?.CSSStyleSheet || CSSStyleSheet;
      sheet = new Sheet();
      sheet.replaceSync(css);
      sheets.set(document, sheet);
    }
    return sheet;
  };
}
