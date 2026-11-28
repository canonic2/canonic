import {mount, unmount} from 'svelte';
import App from './App.svelte';
const app = mount(App, {target: document.querySelector('#app')});
if (import.meta.hot) import.meta.hot.dispose(() => { void unmount(app); });
