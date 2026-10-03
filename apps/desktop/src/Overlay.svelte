<script lang="ts">
  import { onMount } from 'svelte';
  import { onOverlay, type Session } from './lib/native';
  import { dropRipple, expirePill, initial, pillAnchor, reduce } from './lib/overlay';
  import { chipLabel } from './lib/pill';
  let { session }: { session: Session } = $props();
  const FADE_OUT_MS = 200;
  let preferences = $derived(session.preferences);
  let view = $state(initial);
  let fading = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  // A hidden overlay's timers may be throttled, so nothing stale survives turning Glassview off.
  $effect(() => { if (!session.enabled) { clearTimeout(timer); view = initial; fading = false; } });
  function keyPressed(fadeMs: number) {
    clearTimeout(timer);
    fading = false;
    timer = setTimeout(() => {
      fading = true;
      timer = setTimeout(() => { view = expirePill(view, performance.now(), fadeMs); fading = false; }, FADE_OUT_MS);
    }, fadeMs - FADE_OUT_MS);
  }
  onMount(() => {
    let disposed = false, stop = () => {};
    onOverlay(event => {
      const fadeMs = preferences.fadeMs;
      view = reduce(view, event, performance.now(), fadeMs);
      if (event.kind === 'key') keyPressed(fadeMs);
    }).then(fn => { if (disposed) fn(); else stop = fn; }).catch(error => console.error(error));
    return () => { disposed = true; stop(); clearTimeout(timer); };
  });
</script>

<div class="overlay" aria-hidden="true">
  {#each view.ripples as ripple (ripple.id)}
    <span class="ripple" style:left={`${ripple.x}px`} style:top={`${ripple.y}px`} style:--size={`${preferences.rippleSize}px`} style:--color={preferences.rippleColors[ripple.button]}
      onanimationend={() => view = dropRipple(view, ripple.id)}></span>
  {/each}
  {#if preferences.halo && view.halo}
    <span class="halo" style:transform={`translate(${view.halo.x}px, ${view.halo.y}px)`} style:--color={preferences.rippleColors.left}></span>
  {/if}
  {#if view.pill}
    <div class={`pill ${preferences.pillSize}`} class:fading style={pillAnchor(preferences.pillPosition)} style:--fade-out={`${FADE_OUT_MS}ms`}>
      {#each view.pill.chips as chip, index (index)}
        <span class={`chip ${chip.kind}`}>
          <span class="chip-label">{chipLabel(chip)}</span>
          {#if chip.count > 1}<small class="repeat">×{chip.count}</small>{/if}
        </span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .overlay { position: fixed; inset: 0; overflow: hidden; pointer-events: none; }
  .ripple { position: absolute; width: var(--size); height: var(--size); margin: calc(var(--size) / -2) 0 0 calc(var(--size) / -2); border-radius: 50%; border: 3px solid var(--color); background: color-mix(in srgb, var(--color) 22%, transparent); animation: ripple 480ms cubic-bezier(.2, .7, .3, 1) forwards; }
  @keyframes ripple { from { transform: scale(.25); opacity: 1; } 60% { opacity: .9; } to { transform: scale(1); opacity: 0; } }
  .halo { position: absolute; left: -22px; top: -22px; width: 44px; height: 44px; border-radius: 50%; background: radial-gradient(circle, color-mix(in srgb, var(--color) 45%, transparent) 0%, color-mix(in srgb, var(--color) 18%, transparent) 55%, transparent 72%); transition: transform 16ms linear; }
  .pill { position: absolute; display: flex; align-items: center; gap: .4em; max-width: calc(100vw - 48px); color: var(--gb-text); font-weight: 500; white-space: pre; transition: opacity var(--fade-out) ease-out; }
  .pill.fading { opacity: 0; }
  .small { font-size: 15px; }
  .medium { font-size: 21px; }
  .large { font-size: 30px; }
  .chip { display: inline-flex; align-items: center; gap: .4em; padding: .22em .48em; border-radius: .48em; background: color-mix(in srgb, var(--gb-surface) 94%, transparent); box-shadow: inset 0 0 0 1px var(--gb-border), 0 2px 4px var(--gb-shadow); -webkit-backdrop-filter: blur(16px); backdrop-filter: blur(16px); line-height: 1.25; }
  .chip.chord { background: linear-gradient(180deg, color-mix(in srgb, var(--gb-input-surface) 96%, transparent), color-mix(in srgb, var(--gb-surface) 96%, transparent)); font-weight: 600; letter-spacing: .02em; }
  .chip.text { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; color: var(--gb-secondary-text); }
  .repeat { padding: .12em .36em; border-radius: .4em; background: var(--gb-hover); color: var(--gb-muted); font-size: .62em; font-weight: 500; line-height: 1.25; font-variant-numeric: tabular-nums; }
  @keyframes fade { from { opacity: 1; } to { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .ripple { animation-name: fade; } .halo, .pill { transition: none; } }
</style>
