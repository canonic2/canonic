import {h, render} from 'preact';
import {useEffect, useRef, useState} from 'preact/hooks';
import {mountControls} from './controls.js';
function App() {
  const root = useRef(), [state, setState] = useState({});
  useEffect(() => mountControls(root.current, setState), []);
  return h('section', null, h('h1', null, 'Acme Preact'), h('div', {ref: root}), h('output', null, JSON.stringify(state)));
}
const root = document.querySelector('#app'); render(h(App), root);
if (import.meta.hot) import.meta.hot.dispose(() => render(null, root));
