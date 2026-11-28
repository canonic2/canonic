import {mountControls} from './controls.js';
const app = document.querySelector('#app');
const title = document.createElement('h1'); title.textContent = 'Acme Vite';
const controls = document.createElement('div'), output = document.createElement('output');
app.append(title, controls, output);
const dispose = mountControls(controls, state => { output.textContent = JSON.stringify(state); });
if (import.meta.hot) import.meta.hot.dispose(dispose);
