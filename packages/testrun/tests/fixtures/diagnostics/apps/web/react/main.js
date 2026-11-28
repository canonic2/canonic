import React, {useEffect, useRef, useState} from 'react';
import {createRoot} from 'react-dom/client';
import {mountControls} from './controls.js';
function App() {
  const root = useRef(null), [state, setState] = useState({});
  useEffect(() => mountControls(root.current, setState), []);
  return React.createElement('section', null, React.createElement('h1', null, 'Acme React'), React.createElement('div', {ref: root}), React.createElement('output', null, JSON.stringify(state)));
}
const root = createRoot(document.querySelector('#app')); root.render(React.createElement(App));
if (import.meta.hot) import.meta.hot.dispose(() => root.unmount());
