/** @file Consumer type fixture: uses of the element helpers that must type-check with the DOM library. */
import {
  captureInputs,
  defineOnce,
  replayInputs,
  skipUpgradeAttribute,
} from '@canonic2/lattice/elements';

type IsAny<T> = 0 extends 1 & T ? true : false;
type Equal<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
type Expect<T extends true> = T;

export type Imports = [
  Expect<Equal<IsAny<typeof captureInputs>, false>>,
  Expect<Equal<IsAny<typeof replayInputs>, false>>,
  Expect<Equal<IsAny<typeof skipUpgradeAttribute>, false>>,
  Expect<Equal<IsAny<typeof defineOnce>, false>>,
];

/** An element written with the helpers, checked by the consumer's compiler. */
export class AcmeLabel extends HTMLElement {
  #pending: Map<string, unknown> | null = null;
  label = '';

  /** Captures a label assigned before upgrade. */
  constructor() {
    super();
    const pending = captureInputs(this, ['label'], { label: 'label' });
    if (pending !== null) {
      this.#pending = pending;
      queueMicrotask(() => replayInputs(this, pending));
    }
  }

  /**
   * Skips the upgrade's own attribute callback.
   * @param {string} name The attribute name.
   */
  attributeChangedCallback(name: string) {
    skipUpgradeAttribute(this.#pending, name);
  }
}

defineOnce(customElements, 'acme-label', AcmeLabel);
