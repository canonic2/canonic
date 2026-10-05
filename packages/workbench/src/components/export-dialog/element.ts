import { dialogStyles } from '../size-dialog/styles.ts';
import type { CurrentView } from '../../modules/export/request.ts';
export class WbExportDialog extends HTMLElement {
  #pages: { id: string; label: string }[] = [];
  #collections: string[] = [];
  #current?: CurrentView;
  get pages() { return this.#pages; }
  set pages(value: { id: string; label: string }[]) { this.#pages = value; }
  get collections() { return this.#collections; }
  set collections(value: string[]) { this.#collections = value; }
  get current() { return this.#current; }
  set current(value: CurrentView | undefined) { this.#current = value; }
  #dialog: HTMLDialogElement;
  #connection?: AbortController;
  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(dialogStyles + 'select{font:inherit;padding:6px;color:var(--wb-fg,#222);background:var(--wb-raised,#fff);border:1px solid var(--wb-line,#bbb);border-radius:6px}');
    root.adoptedStyleSheets = [sheet];
    root.innerHTML = `<dialog aria-labelledby="title"><form><h2 id="title">Export</h2>
      <label>Format<select name="format"><option value="zip">Source package (ZIP)</option><option value="pdf">PDF</option><option value="images">Images (ZIP)</option><option value="browser">Portable browser (ZIP)</option></select></label>
      <label>Scope<select name="scope"><option value="page">Current page</option><option value="pages">Selected pages</option><option value="collection">Collection</option><option value="space">Whole space</option></select></label>
      <label data-field="pages">Pages<select name="pages" multiple size="5"></select></label>
      <label data-field="collection">Collection<select name="collection"></select></label>
      <label>Variants<select name="variants"><option value="all">All declared states and sizes</option><option value="current">Current view</option></select></label>
      <label data-field="paper">Documentation paper<select name="paper"><option>A4</option><option>Letter</option></select></label>
      <label data-field="imageFormat">Image format<select name="imageFormat"><option value="png">PNG</option><option value="jpeg">JPEG</option></select></label>
      <p class="hint">Visual PDF pages preserve their full height. Documentation uses paginated paper.</p>
      <div class="actions"><button type="button" class="secondary">Cancel</button><button class="primary">Export</button></div></form></dialog>`;
    this.#dialog = root.querySelector('dialog')!;
  }
  #select(name: string): HTMLSelectElement { return this.shadowRoot!.querySelector(`[name="${name}"]`)!; }
  connectedCallback() {
    for (const key of ['pages', 'collections', 'current']) {
      if (!Object.hasOwn(this, key)) continue;
      const inputs = this as unknown as Record<string, unknown>;
      const value = inputs[key]; delete inputs[key]; inputs[key] = value;
    }
    this.#connection?.abort();
    const signal = (this.#connection = new AbortController()).signal;
    this.#dialog.addEventListener('change', event => {
      const name = (event.target as HTMLSelectElement).name;
      if (name === 'format' || name === 'scope') this.#select('variants').value = this.#select('scope').value === 'page' && ['pdf', 'images'].includes(this.#select('format').value) ? 'current' : 'all';
      this.#update();
    }, { signal });
    this.shadowRoot!.querySelector('button[type=button]')!.addEventListener('click', () => this.#dialog.close(), { signal });
    this.shadowRoot!.querySelector('form')!.addEventListener('submit', event => {
      event.preventDefault();
      const scope = this.#select('scope').value;
      const pages = scope === 'page' ? [this.current!.page] : [...this.#select('pages').selectedOptions].map(o => o.value);
      if (scope === 'pages' && !pages.length) { this.#select('pages').focus(); return; }
      this.dispatchEvent(new CustomEvent('wb-export-request', { bubbles: true, composed: true, detail: {
        format: this.#select('format').value, scope, pages, collection: this.#select('collection').value,
        variants: this.#select('variants').value, current: this.current,
        paper: this.#select('paper').value, imageFormat: this.#select('imageFormat').value,
      } }));
      this.#dialog.close();
    }, { signal });
  }
  disconnectedCallback() { this.#connection?.abort(); this.#dialog.close(); }
  show() {
    for (const [name, choices] of [['pages', this.pages.map(p => [p.id, p.label])], ['collection', this.collections.map(c => [c, c])]] as const) {
      this.#select(name).replaceChildren(...choices.map(([id, label]) => new Option(label, id)));
    }
    this.#select('scope').options[0].disabled = !this.current;
    this.#select('scope').value = this.current ? 'page' : 'space';
    this.#select('format').value = 'zip'; this.#select('variants').value = 'all';
    this.#update(); this.#dialog.showModal();
  }
  #update() {
    const scope = this.#select('scope').value, format = this.#select('format').value;
    this.#select('pages').required = scope === 'pages';
    this.#select('collection').required = scope === 'collection';
    this.#select('variants').options[1].disabled = scope !== 'page' || format === 'browser';
    for (const [field, visible] of Object.entries({ pages: scope === 'pages', collection: scope === 'collection', paper: format === 'pdf', imageFormat: format === 'images' })) {
      (this.shadowRoot!.querySelector(`[data-field="${field}"]`) as HTMLElement).hidden = !visible;
    }
  }
}
if (!customElements.get('wb-export-dialog')) customElements.define('wb-export-dialog', WbExportDialog);
