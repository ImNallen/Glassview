<script lang="ts">
  import type { Button } from './lib/native';

  let { button }: { button: Button } = $props();

  const DOME = 'M11 1.3a8.6 8.6 0 0 0-8.6 8.6v2.4h17.2v-2.4A8.6 8.6 0 0 0 11 1.3Z';
  const MARK: Partial<Record<Button, string>> = {
    left: 'M9.15 1.5A8.6 8.6 0 0 0 2.4 9.9V12.3H9.15Z',
    right: 'M12.85 1.5A8.6 8.6 0 0 1 19.6 9.9V12.3H12.85Z',
    back: 'M8.3 6.3 4.3 8.9 8.3 11.5V9.7H11.5V8.1H8.3Z',
    forward: 'M13.7 6.3 17.7 8.9 13.7 11.5V9.7H10.5V8.1H13.7Z',
  };
  const mark = $derived(MARK[button]);
</script>

<svg class="click-icon" viewBox="0 0 22 14" data-button={button}>
  {#if mark}<path d={mark} fill="currentColor"/>{/if}
  <path d={DOME} fill="none" stroke="currentColor" stroke-width="1.5"/>
  {#if button === 'middle'}
    <rect x="8.85" y="2.55" width="4.3" height="6.7" rx="2.15" fill="currentColor"/>
  {:else if button === 'left' || button === 'right'}
    <rect x="9.7" y="3.35" width="2.6" height="4.6" rx="1.3" fill="none" stroke="currentColor" stroke-width="1.25"/>
  {/if}
</svg>

<style>
  .click-icon { height: .8em; width: auto; display: block; flex: none; }
</style>
