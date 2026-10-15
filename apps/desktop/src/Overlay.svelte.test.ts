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

function buttons(root: ParentNode = document) {
  return [...root.querySelectorAll('.chip')].map(chip => chip.querySelector('svg')?.getAttribute('data-button') ?? null);
}

it('draws a pill click as a mouse icon and leaves a key as its word', () => {
  overlay = mount(Overlay, { target: document.body, props: { session: structuredClone(DEFAULT_SESSION) } });
  flushSync();
  emit({ kind: 'key', stroke: { kind: 'chord', label: '⇧⌘Click' } });
  emit({ kind: 'key', stroke: { kind: 'chord', label: '⇧⌘Click' } });
  emit({ kind: 'key', stroke: { kind: 'chord', label: 'Ctrl+Shift+Right-click' } });
  emit({ kind: 'key', stroke: { kind: 'chord', label: '⌘K' } });
  emit({ kind: 'key', stroke: { kind: 'chord', label: 'Backspace' } });
  flushSync();
  const chips = [...document.querySelectorAll('.chip')];
  expect(chips.map(chip => chip.querySelector('.chip-label')?.textContent)).toEqual(['⇧⌘', 'Ctrl+Shift+', '⌘K', 'Backspace']);
  expect(chips[0].querySelector('.repeat')?.textContent).toBe('×2');
  expect(buttons()).toEqual(['left', 'right', null, null]);
  expect(chips[0].querySelectorAll('svg')).toHaveLength(1);
});

it('draws the middle, back, and forward buttons as icons', () => {
  overlay = mount(Overlay, { target: document.body, props: { session: structuredClone(DEFAULT_SESSION) } });
  flushSync();
  for (const label of ['Middle-click', 'Back', 'Forward']) emit({ kind: 'key', stroke: { kind: 'chord', label } });
  flushSync();
  expect([...document.querySelectorAll('.chip-label')].map(label => label.textContent)).toEqual(['', '', '']);
  expect(buttons()).toEqual(['middle', 'back', 'forward']);
});

it('sets the pill in the chosen font', () => {
  const session = structuredClone(DEFAULT_SESSION);
  session.preferences.pillFont = 'rounded';
  overlay = mount(Overlay, { target: document.body, props: { session } });
  flushSync();
  emit({ kind: 'key', stroke: { kind: 'chord', label: '⌘K' } });
  flushSync();
  const style = document.querySelector('.pill')?.getAttribute('style');
  expect(style).toContain("--font: 'Nunito Variable', ui-rounded, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif");
  expect(style).toContain("--mono: 'Geist Mono Variable', ui-monospace");
});
