// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Overlay from './Overlay.svelte';
import { DEFAULT_SESSION, type OverlayEvent } from './lib/native';

let emit: (event: OverlayEvent) => void = () => {};
vi.mock('./lib/native', async original => ({
  ...await original<typeof import('./lib/native')>(),
  onOverlay: (fn: (event: OverlayEvent) => void) => {
    emit = fn;
    return Promise.resolve(() => {});
  },
}));

let overlay: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (overlay) await unmount(overlay);
  overlay = undefined;
  document.body.replaceChildren();
});

it('draws the hold ring at the pointer and no drag line', () => {
  overlay = mount(Overlay, { target: document.body, props: { session: structuredClone(DEFAULT_SESSION) } });
  flushSync();
  emit({ kind: 'hold', hold: { button: 'left', mods: null, x: 40, y: 50 } });
  emit({ kind: 'hold', hold: { button: 'left', mods: null, x: 80, y: 90 } });
  flushSync();
  expect(document.querySelector('.hold')?.getAttribute('style')).toContain('translate(80px, 90px)');
  expect(document.querySelector('polyline')).toBeNull();
});
