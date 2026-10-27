/** @file Browser entry: mounts once and ends the tutorial when the page leaves. */
import { mountTutorial } from './app.js';

const container = document.getElementById('tutorial');
if (!(container instanceof HTMLElement)) throw new Error('Missing tutorial container.');
const lifetime = new AbortController();
mountTutorial(container, { signal: lifetime.signal });
window.addEventListener('pagehide', () => lifetime.abort(), { once: true });
