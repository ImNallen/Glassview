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

it('draws a press as a ripple and its modifiers, with nothing left to follow the pointer', () => {
  overlay = mount(Overlay, { target: document.body, props: { session: structuredClone(DEFAULT_SESSION) } });
  flushSync();
  emit({ kind: 'click', button: 'left', mods: '⌘', x: 40, y: 50 });
  flushSync();
  const drawn = [...document.querySelectorAll('.overlay > *')];
  expect(drawn.map(element => element.className.replace(/\s*svelte-\S+/g, ''))).toEqual(['ripple', 'chip chord mods clicked']);
  expect(drawn.map(element => element.getAttribute('style'))).toEqual([
    expect.stringContaining('left: 40px; top: 50px'),
    expect.stringContaining('left: 40px; top: 50px'),
  ]);
  expect(drawn[1].textContent).toBe('⌘');
});
