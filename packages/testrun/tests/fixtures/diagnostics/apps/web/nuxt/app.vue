<script setup>
import {ref, onMounted, onUnmounted} from 'vue';
import {mountControls} from './src/controls.js';
const root = ref(), state = ref({}); let dispose;
onMounted(() => { dispose = mountControls(root.value, value => { state.value = value; }); });
onUnmounted(() => dispose?.());
</script>
<template><section><h1>Acme Nuxt</h1><div ref="root"></div><output>{{JSON.stringify(state)}}</output></section></template>
