<script lang="ts">
  import { Keyboard, MousePointerClick, Power, RotateCw, ShieldCheck, X } from '@lucide/svelte';
  import Logo from './Logo.svelte';
  import { action, savePreferences, type Action, type KeyMode, type PillPosition, type PillSize, type Preferences, type Session } from './lib/native';
  let { session }: { session: Session } = $props();
  const mac = navigator.userAgent.includes('Mac');
  const BUTTONS: readonly { id: keyof Preferences['rippleColors']; label: string }[] = [{ id: 'left', label: 'Left' }, { id: 'right', label: 'Right' }, { id: 'middle', label: 'Middle' }];
  const MODES: readonly { id: KeyMode; label: string }[] = [{ id: 'shortcuts', label: 'Shortcuts only' }, { id: 'all', label: 'All keys' }];
  const POSITIONS: readonly { id: PillPosition; label: string }[] = [
    { id: 'top-left', label: 'Top left' }, { id: 'top-right', label: 'Top right' }, { id: 'bottom-left', label: 'Bottom left' },
    { id: 'bottom-center', label: 'Bottom center' }, { id: 'bottom-right', label: 'Bottom right' },
  ];
  const SIZES: readonly { id: PillSize; label: string }[] = [{ id: 'small', label: 'Small' }, { id: 'medium', label: 'Medium' }, { id: 'large', label: 'Large' }];
  const COMMAND_KEYS = mac ? '⌘, ⌃, or ⌥' : 'Ctrl, Alt, or Win';
  let preferences = $derived(session.preferences);
  let problem = $state('');
  let message = $derived(problem || session.error);
  const report = (error: unknown) => problem = String(error);
  const run = (name: Action) => action(name).catch(report);
  function update(patch: Partial<Preferences>) {
    problem = '';
    savePreferences({ ...session.preferences, ...patch }).catch(report);
  }
  function dismiss() {
    problem = '';
    if (session.error) run('dismiss-error');
  }
  let keyNote = $derived(preferences.keys === 'shortcuts'
    ? `Only keys pressed with ${COMMAND_KEYS} appear. Typed text, including passwords, is never shown.`
    : mac ? 'Everything you type appears on screen. Password fields stay hidden.' : 'Everything you type appears on screen, including passwords.');
</script>

<svelte:window onkeydown={event => { if (event.key === 'Escape') { event.preventDefault(); run('close-settings'); } }}/>

<main class="settings" aria-label="Glassview settings">
  <header>
    <span class="brand"><Logo size={18}/>Glassview<span class="version">v{__APP_VERSION__}</span></span>
  </header>

  <div class="list">
    <div class="row">
      <span class="row-label" id="enabled-label">Show clicks and keys<small>Toggle anytime with <kbd>{session.shortcut}</kbd></small></span>
      <button class="switch" role="switch" aria-checked={session.enabled} aria-labelledby="enabled-label" onclick={() => run('toggle')}><span></span></button>
    </div>
  </div>
  {#if session.shortcutUnavailable}<p class="note warning" role="alert">Another app is using {session.shortcut}, so it can’t toggle Glassview. Use the {mac ? 'menu bar' : 'tray'} icon instead.</p>{/if}

  {#if session.keyAccess !== 'granted' || session.grantedWhileRunning}
    <section class="card" aria-labelledby="access-title">
      <ShieldCheck size={18} strokeWidth={1.9}/>
      <div>
        {#if session.keyAccess === 'unknown'}
          <h2 id="access-title">Allow key display</h2>
          <p>To show shortcuts, macOS needs to let Glassview use Input Monitoring. Clicks already work without it.</p>
          <button class="button primary" onclick={() => run('request-key-access')}>Allow…</button>
        {:else if session.keyAccess === 'denied'}
          <h2 id="access-title">Key display is off</h2>
          <p>Turn on Glassview in System Settings › Privacy & Security › Input Monitoring. Clicks still show.</p>
          <button class="button primary" onclick={() => run('request-key-access')}>Open System Settings</button>
        {:else}
          <h2 id="access-title">Key display is allowed</h2>
          <p>If shortcuts don’t appear, relaunch Glassview.</p>
          <button class="button" onclick={() => run('relaunch')}><RotateCw size={12}/>Relaunch</button>
        {/if}
      </div>
    </section>
  {/if}

  <section class="group" aria-labelledby="keys-title">
    <h2 id="keys-title"><Keyboard size={12}/>Keys</h2>
    <div class="segmented" role="group" aria-labelledby="keys-title">
      {#each MODES as mode}
        <button class:chosen={preferences.keys === mode.id} aria-pressed={preferences.keys === mode.id} onclick={() => update({ keys: mode.id })}>{mode.label}</button>
      {/each}
    </div>
    <p class="note" class:warning={preferences.keys === 'all'}>{keyNote}{mac ? '' : ' Apps running as administrator can’t be seen.'}</p>
    <div class="list">
      <div class="row"><span class="row-label" id="size-label">Pill size</span>
        <div class="segmented compact" role="group" aria-labelledby="size-label">
          {#each SIZES as size}<button class:chosen={preferences.pillSize === size.id} aria-pressed={preferences.pillSize === size.id} onclick={() => update({ pillSize: size.id })}>{size.label}</button>{/each}
        </div>
      </div>
      <label class="row"><span class="row-label">Fade after<small>{(preferences.fadeMs / 1000).toFixed(1)} s without a key</small></span>
        <input type="range" min="500" max="5000" step="100" value={preferences.fadeMs} oninput={event => update({ fadeMs: event.currentTarget.valueAsNumber })}/></label>
      <div class="row"><span class="row-label" id="position-label">Pill position</span>
        <div class="positions" role="group" aria-labelledby="position-label">
          {#each POSITIONS as position}
            <button class={`position ${position.id}`} class:chosen={preferences.pillPosition === position.id} aria-pressed={preferences.pillPosition === position.id}
              title={position.label} aria-label={position.label} onclick={() => update({ pillPosition: position.id })}><span></span></button>
          {/each}
        </div>
      </div>
    </div>
  </section>

  <section class="group" aria-labelledby="clicks-title">
    <h2 id="clicks-title"><MousePointerClick size={12}/>Clicks</h2>
    <div class="list">
      <div class="row"><span class="row-label">Ripple colors</span>
        {#each BUTTONS as button}
          <label class="color" title={`${button.label} button`}><input type="color" aria-label={`${button.label} button color`} value={preferences.rippleColors[button.id]}
            oninput={event => update({ rippleColors: { ...preferences.rippleColors, [button.id]: event.currentTarget.value } })}/><small>{button.label}</small></label>
        {/each}
      </div>
      <label class="row"><span class="row-label">Ripple size<small>{preferences.rippleSize} px</small></span>
        <input type="range" min="24" max="160" step="4" value={preferences.rippleSize} oninput={event => update({ rippleSize: event.currentTarget.valueAsNumber })}/></label>
      <div class="row">
        <span class="row-label" id="halo-label">Pointer halo<small>A soft glow that follows the pointer</small></span>
        <button class="switch" role="switch" aria-checked={preferences.halo} aria-labelledby="halo-label" onclick={() => update({ halo: !preferences.halo })}><span></span></button>
      </div>
    </div>
  </section>

  <div class="status" aria-live="polite">
    {#if message}<p class="error" role="alert">{message}<button class="dismiss" aria-label="Dismiss error" onclick={dismiss}><X size={12}/></button></p>{/if}
  </div>

  <footer>
    <button onclick={() => run('quit')}><Power size={13}/>Quit Glassview</button>
  </footer>
</main>

<style>
  .settings { color-scheme: light dark; display: flex; flex-direction: column; height: 100vh; border-radius: 12px; overflow-y: auto; padding: 16px 14px 0; background: var(--gb-surface); scrollbar-width: thin; }
  header { display: flex; align-items: center; margin: 0 2px 14px; }
  .brand { display: flex; align-items: center; gap: 7px; font-size: 14px; font-weight: 600; letter-spacing: -.2px; color: var(--gb-strong-text); }
  .version { margin-left: 1px; padding: 2px 6px; border-radius: 999px; background: color-mix(in srgb, var(--gb-text) 7%, transparent); font-size: 10px; font-weight: 500; letter-spacing: 0; color: var(--gb-muted); font-variant-numeric: tabular-nums; }

  .group { margin-top: 20px; }
  h2 { display: flex; align-items: center; gap: 5px; margin: 0 0 8px 2px; font-size: 10px; font-weight: 600; letter-spacing: .8px; text-transform: uppercase; color: var(--gb-muted); }
  .note { margin: 8px 2px 10px; font-size: 11px; line-height: 1.4; color: var(--gb-muted); }
  .note.warning { color: var(--gb-error-text); }
  kbd { font: inherit; font-size: 10.5px; padding: 1px 5px; border-radius: 4px; background: var(--gb-key-surface); color: var(--gb-secondary-text); }

  .list { overflow: hidden; border: 1px solid var(--gb-divider); border-radius: 10px; background: var(--gb-subtle-surface); }
  .row { display: flex; align-items: center; gap: 10px; min-height: 42px; padding: 5px 8px 5px 12px; }
  .row + .row { border-top: 1px solid var(--gb-divider); }
  .row-label { flex: 1; min-width: 0; font-size: 12.5px; color: var(--gb-text); }
  .row-label small { display: block; margin-top: 1px; font-size: 10.5px; color: var(--gb-muted); }
  input[type="range"] { width: 130px; accent-color: var(--gb-focus-ring); }

  .segmented { display: flex; gap: 2px; padding: 3px; border-radius: 9px; background: color-mix(in srgb, var(--gb-text) 6%, transparent); }
  .segmented button { flex: 1; height: 26px; padding: 0 8px; border-radius: 6px; font-size: 12px; font-weight: 500; color: var(--gb-secondary-text); transition: background .12s, color .12s, box-shadow .12s; }
  .segmented button:hover { color: var(--gb-strong-text); }
  .segmented button.chosen { background: var(--gb-input-surface); color: var(--gb-strong-text); box-shadow: 0 0 0 1px var(--gb-selected-border), 0 1px 2px var(--gb-shadow); }
  .segmented.compact button { height: 22px; font-size: 11px; }

  .positions { display: grid; grid-template-columns: repeat(5, 30px); gap: 5px; }
  .position { position: relative; height: 28px; border-radius: 7px; border: 1px solid var(--gb-border); background: var(--gb-input-surface); transition: border-color .12s, box-shadow .12s; }
  .position span { position: absolute; width: 14px; height: 4px; border-radius: 2px; background: var(--gb-muted); opacity: .55; }
  .position.top-left span { top: 5px; left: 5px; }
  .position.top-right span { top: 5px; right: 5px; }
  .position.bottom-left span { bottom: 5px; left: 5px; }
  .position.bottom-right span { bottom: 5px; right: 5px; }
  .position.bottom-center span { bottom: 5px; left: 50%; translate: -50% 0; }
  .position.chosen { border-color: var(--gb-focus-ring); box-shadow: inset 0 0 0 1px var(--gb-focus-ring); }
  .position.chosen span { background: var(--gb-focus-ring); opacity: 1; }

  .color { display: flex; flex-direction: column; align-items: center; gap: 1px; cursor: pointer; }
  .color small { font-size: 9.5px; color: var(--gb-muted); }
  input[type="color"] { width: 26px; height: 22px; padding: 0; border: 0; border-radius: 6px; background: none; cursor: pointer; }
  input[type="color"]::-webkit-color-swatch-wrapper { padding: 0; }
  input[type="color"]::-webkit-color-swatch { border: 0; border-radius: 6px; box-shadow: inset 0 0 0 1px var(--gb-swatch-border); }

  .switch { position: relative; flex-shrink: 0; width: 34px; height: 20px; padding: 0; border-radius: 10px; background: color-mix(in srgb, var(--gb-text) 18%, transparent); transition: background .16s; }
  .switch span { position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: #fff; box-shadow: 0 1px 2px #0000004d; transition: translate .16s ease; }
  .switch[aria-checked="true"] { background: var(--gb-focus-ring); }
  .switch[aria-checked="true"] span { translate: 14px 0; }
  @media (prefers-reduced-motion: reduce) { .switch, .switch span { transition: none; } }

  .card { display: flex; gap: 10px; margin-top: 14px; padding: 12px; border-radius: 10px; border: 1px solid color-mix(in srgb, var(--gb-focus-ring) 40%, transparent); background: color-mix(in srgb, var(--gb-focus-ring) 8%, transparent); color: var(--gb-focus-ring); }
  .card h2 { margin: 0 0 4px; font-size: 12.5px; letter-spacing: 0; text-transform: none; color: var(--gb-strong-text); }
  .card p { margin: 0 0 10px; font-size: 11.5px; line-height: 1.45; color: var(--gb-secondary-text); }
  .button { display: inline-flex; align-items: center; gap: 5px; height: 26px; padding: 0 11px; border-radius: 7px; background: var(--gb-input-surface); box-shadow: 0 0 0 1px var(--gb-border), 0 1px 1px var(--gb-shadow); font-size: 11.5px; font-weight: 500; color: var(--gb-strong-text); }
  .button.primary { background: var(--gb-focus-ring); box-shadow: none; color: var(--gb-surface); }

  .status { padding: 10px 2px 0; font-size: 11px; line-height: 1.4; }
  .status p { margin: 0; }
  .error { display: flex; align-items: flex-start; justify-content: space-between; gap: 8px; color: var(--gb-error-text); overflow-wrap: anywhere; }
  .dismiss { display: grid; place-items: center; flex-shrink: 0; width: 18px; height: 18px; border-radius: 5px; color: var(--gb-muted); }
  .dismiss:hover { background: var(--gb-hover); color: var(--gb-strong-text); }
  footer { display: flex; justify-content: flex-end; margin: auto -14px 0; padding: 10px 12px; border-top: 1px solid var(--gb-divider); }
  footer button { display: flex; align-items: center; gap: 6px; padding: 5px 6px; border-radius: 6px; font-size: 11.5px; color: var(--gb-secondary-text); }
  footer button:hover { background: var(--gb-hover); color: var(--gb-strong-text); }
</style>
