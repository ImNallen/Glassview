<script lang="ts">
  import { onMount } from 'svelte';
  import Overlay from './Overlay.svelte';
  import Settings from './Settings.svelte';
  import { DEFAULT_SESSION, subscribe, type Session } from './lib/native';
  import { parseSurface } from './lib/surface';
  const surface = parseSurface(location.search);
  let session = $state<Session>(structuredClone(DEFAULT_SESSION));
  onMount(() => {
    let disposed = false, stop = () => {};
    subscribe(value => session = value).then(fn => { if (disposed) fn(); else stop = fn; }).catch(error => console.error(error));
    return () => { disposed = true; stop(); };
  });
</script>
{#if surface === 'overlay'}<Overlay {session}/>{:else if surface === 'settings'}<Settings {session}/>{/if}
