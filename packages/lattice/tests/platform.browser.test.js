/** @file Verifies the real-browser runner supports native shadow DOM and lifecycle. */
import { expect, test } from 'vitest';
import '../src/index.js';

test('runs a native custom element connection and disconnection in Chromium', () => {
  /** A test-only element that exposes lifecycle calls through DOM attributes. */
  class PlatformProbe extends HTMLElement {
    /** Creates a real shadow root for the browser fixture. */
    constructor() {
      super();
      this.attachShadow({ mode: 'open' }).textContent = 'native shadow DOM';
    }

    /** Records the browser's connection callback. */
    connectedCallback() {
      this.setAttribute('data-connected', 'true');
    }

    /** Records the browser's disconnection callback. */
    disconnectedCallback() {
      this.setAttribute('data-connected', 'false');
    }
  }

  const name = `lattice-probe-${crypto.randomUUID()}`;
  customElements.define(name, PlatformProbe);
  const element = document.createElement(name);
  try {
    document.body.append(element);
    expect(element.shadowRoot?.textContent).toBe('native shadow DOM');
    expect(element.getAttribute('data-connected')).toBe('true');
    element.remove();
    expect(element.getAttribute('data-connected')).toBe('false');
  } finally {
    element.remove();
  }
});
