import {LitElement, html} from 'lit';
import {mountControls} from './controls.js';
class AcmeApp extends LitElement {
  static properties = {state: {state: true}};
  constructor() { super(); this.state = {}; }
  connectedCallback() { super.connectedCallback(); void this.updateComplete.then(() => { if (this.isConnected && !this.dispose) this.dispose = mountControls(this.renderRoot.querySelector('#controls'), state => { this.state = state; }); }); }
  disconnectedCallback() { this.dispose?.(); this.dispose = null; super.disconnectedCallback(); }
  render() { return html`<h1>Acme Lit</h1><div id="controls"></div><output>${JSON.stringify(this.state)}</output>`; }
}
customElements.define('acme-diagnostics', AcmeApp);
document.querySelector('#app').append(document.createElement('acme-diagnostics'));
