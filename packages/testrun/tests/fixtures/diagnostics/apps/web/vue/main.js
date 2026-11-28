import {createApp, h, ref, onMounted, onUnmounted} from 'vue';
import {mountControls} from './controls.js';
const app = createApp({setup() {
  const root = ref(), state = ref({}); let dispose;
  onMounted(() => { dispose = mountControls(root.value, value => { state.value = value; }); });
  onUnmounted(() => dispose?.());
  return () => h('section', [h('h1', 'Acme Vue'), h('div', {ref: root}), h('output', JSON.stringify(state.value))]);
}}); app.mount('#app');
if (import.meta.hot) import.meta.hot.dispose(() => app.unmount());
