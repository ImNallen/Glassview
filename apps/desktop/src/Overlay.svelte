<script lang="ts">
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import { onOverlay, type Session } from './lib/native';
  import { dropRipple, dropTrail, expirePill, initial, pathPoints, pillAnchor, reduce, SCROLL_GLYPHS } from './lib/overlay';
  import { chipLabel } from './lib/pill';
  let { session }: { session: Session } = $props();
  const FADE_OUT_MS = 200;
  const SCROLL_MS = 400;
  let preferences = $derived(session.preferences);
  let view = $state(initial);
  let fading = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined, scrollTimer: ReturnType<typeof setTimeout> | undefined;
  // A hidden overlay's timers may be throttled, so nothing stale survives turning Glassview off.
  $effect(() => { if (!session.enabled) { clearTimeout(timer); clearTimeout(scrollTimer); view = initial; fading = false; } });
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
      if (event.kind === 'scroll') { clearTimeout(scrollTimer); scrollTimer = setTimeout(() => view = { ...view, scroll: null }, SCROLL_MS); }
    }).then(fn => { if (disposed) fn(); else stop = fn; }).catch(error => console.error(error));
    return () => { disposed = true; stop(); clearTimeout(timer); clearTimeout(scrollTimer); };
  });
</script>

<div class="overlay" aria-hidden="true">
  {#if view.hold || view.trails.length}
    <svg class="trails">
      {#each view.trails as trail (trail.id)}
        <polyline class="fading" points={pathPoints(trail.path)} style:--color={preferences.rippleColors[trail.button]} onanimationend={() => view = dropTrail(view, trail.id)}/>
      {/each}
      {#if view.hold}<polyline points={pathPoints(view.hold.path)} style:--color={preferences.rippleColors[view.hold.button]}/>{/if}
    </svg>
  {/if}
  {#each view.ripples as ripple (ripple.id)}
    {@const drop = () => view = dropRipple(view, ripple.id)}
    <span class="ripple" style:left={`${ripple.x}px`} style:top={`${ripple.y}px`} style:--size={`${preferences.rippleSize}px`} style:--color={preferences.rippleColors[ripple.button]}
      onanimationend={ripple.mods ? undefined : drop}></span>
    {#if ripple.mods}
      <span class="chip chord mods clicked" style:left={`${ripple.x}px`} style:top={`${ripple.y}px`} style:--size={`${preferences.rippleSize}px`} onanimationend={drop}>{ripple.mods}</span>
    {/if}
  {/each}
  {#if view.hold}
    {#key view.hold.id}
      <span class="hold" style:transform={`translate(${view.hold.x}px, ${view.hold.y}px)`} style:--size={`${preferences.rippleSize}px`} style:--color={preferences.rippleColors[view.hold.button]}>
        {#if view.hold.mods}<span class="chip chord mods">{view.hold.mods}</span>{/if}
      </span>
    {/key}
  {/if}
  {#if view.scroll}
    <span class="scroll" style:transform={`translate(${view.scroll.x}px, ${view.scroll.y}px)`} transition:fade={{ duration: 150 }}>{SCROLL_GLYPHS[view.scroll.direction]}</span>
  {/if}
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
  .trails { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
  .trails polyline { fill: none; stroke: var(--color); stroke-width: 4; stroke-linecap: round; stroke-linejoin: round; opacity: .75; }
  .trails .fading { animation: trail-out 400ms ease-out forwards; }
  @keyframes trail-out { to { opacity: 0; } }
  /* Delayed so a click released sooner never shows the ring. */
  .hold { position: absolute; left: 0; top: 0; width: calc(var(--size) * .6); height: calc(var(--size) * .6); margin: calc(var(--size) * -.3) 0 0 calc(var(--size) * -.3); border-radius: 50%; border: 3px solid var(--color); box-sizing: border-box; background: color-mix(in srgb, var(--color) 18%, transparent); transition: transform 16ms linear; animation: hold-in 160ms ease-out 200ms both; }
  @keyframes hold-in { from { opacity: 0; scale: .6; } }
  .mods { position: absolute; color: var(--gb-text); font-size: 13px; white-space: pre; }
  /* Takes over from the click's chip, in the same spot, as that one fades. */
  .hold .mods { left: 50%; top: 50%; transform: translate(calc(var(--size) / 2 + 6px), -50%); animation: appear 150ms ease-out 750ms both; }
  .mods.clicked { transform: translate(calc(var(--size) / 2 + 6px), -50%); animation: mods 900ms ease-out forwards; }
  @keyframes mods { from { opacity: 0; } 8%, 70% { opacity: 1; } to { opacity: 0; } }
  .scroll { position: absolute; left: 14px; top: -14px; display: grid; place-items: center; width: 28px; height: 28px; border-radius: 50%; color: var(--gb-text); font-size: 16px; font-weight: 600; background: color-mix(in srgb, var(--gb-surface) 94%, transparent); box-shadow: inset 0 0 0 1px var(--gb-border), 0 2px 4px var(--gb-shadow); transition: transform 16ms linear; }
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
  @keyframes appear { from { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .ripple { animation-name: fade; } .hold { animation-name: appear; } .halo, .hold, .scroll, .pill { transition: none; } }
</style>
