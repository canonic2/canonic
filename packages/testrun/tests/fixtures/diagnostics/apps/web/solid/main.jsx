import {createSignal, onMount, onCleanup} from 'solid-js';
import {render} from 'solid-js/web';
import {mountControls} from './controls.js';
function App() {
  let root; const [state, setState] = createSignal({}); let dispose;
  onMount(() => { dispose = mountControls(root, setState); }); onCleanup(() => dispose?.());
  return <section><h1>Acme Solid</h1><div ref={root} /><output>{JSON.stringify(state())}</output></section>;
}
const dispose = render(() => <App />, document.querySelector('#app'));
if (import.meta.hot) import.meta.hot.dispose(dispose);
