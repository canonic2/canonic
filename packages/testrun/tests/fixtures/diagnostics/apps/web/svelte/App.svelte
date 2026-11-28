<script>
  import {onMount} from 'svelte';
  import {mountControls} from './controls.js';
  let root, state = {};
  onMount(() => mountControls(root, value => { state = value; }));
</script>
<h1>Acme Svelte</h1><div bind:this={root}></div><output>{JSON.stringify(state)}</output>
