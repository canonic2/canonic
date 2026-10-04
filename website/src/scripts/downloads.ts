/* <canonic-downloads> marks the download for the visitor's computer in the
   list it wraps: each download (a card or a table row) carries data-target,
   and an optional [data-rec-badge] inside it is shown when recommended.
   DownloadCards.astro and PlatformTable.astro both use it. */
import { recommendation } from './platform';

class CanonicDownloads extends HTMLElement {
  connectedCallback() {
    const pick = recommendation();
    if (!pick) return;
    this.querySelectorAll<HTMLElement>(`[data-target="${pick.target}"]`).forEach(item => {
      item.classList.add('is-recommended');
      const badge = item.querySelector<HTMLElement>('[data-rec-badge]');
      if (badge) badge.hidden = false;
      const link = item.matches('a') ? item : item.querySelector('a[aria-label]');
      if (link) link.setAttribute('aria-label', `${link.getAttribute('aria-label')}, recommended for this computer`);
    });
  }
}

if (!customElements.get('canonic-downloads')) customElements.define('canonic-downloads', CanonicDownloads);
