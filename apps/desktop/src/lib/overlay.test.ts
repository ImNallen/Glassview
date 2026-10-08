import { describe, expect, it } from 'vitest';
import { dropRipple, expirePill, initial, MAX_RIPPLES, pillAnchor, reduce, rippleColor, type OverlayView } from './overlay';
import type { Button, HoldView } from './native';

const FADE = 1500;
const hold = (view: OverlayView, x: number, y: number, mods: string | null = null) =>
  reduce(view, { kind: 'hold', hold: { button: 'left', mods, x, y } satisfies HoldView }, 0, FADE);
const release = (view: OverlayView) => reduce(view, { kind: 'hold', hold: null }, 0, FADE);

describe('overlay view', () => {
  it('adds a ripple per click with increasing ids and drops it when its animation ends', () => {
    let view = reduce(initial, { kind: 'click', button: 'left', mods: '⇧⌘', x: 412, y: 88 }, 0, FADE);
    view = reduce(view, { kind: 'click', button: 'middle', mods: null, x: 1, y: 2 }, 0, FADE);
    expect(view.ripples).toEqual([{ id: 0, button: 'left', mods: '⇧⌘', x: 412, y: 88 }, { id: 1, button: 'middle', mods: null, x: 1, y: 2 }]);
    expect(dropRipple(view, 0).ripples).toEqual([{ id: 1, button: 'middle', mods: null, x: 1, y: 2 }]);
  });

  it('keeps only the newest ripples during a click storm', () => {
    let view = initial;
    for (let i = 0; i < 30; i++) view = reduce(view, { kind: 'click', button: 'right', mods: null, x: i, y: 0 }, 0, FADE);
    expect(view.ripples.length).toBe(MAX_RIPPLES);
    expect(view.ripples[0]).toEqual({ id: 6, button: 'right', mods: null, x: 6, y: 0 });
  });

  it('moves and clears the halo', () => {
    const shown = reduce(initial, { kind: 'halo', at: { x: 10, y: 20 } }, 0, FADE);
    expect(shown.halo).toEqual({ x: 10, y: 20 });
    expect(reduce(shown, { kind: 'halo', at: null }, 0, FADE).halo).toBeNull();
  });

  it('follows a held button until release, then starts the next hold fresh', () => {
    let view = hold(initial, 10, 10, '⌘');
    expect(view.hold).toEqual({ id: 0, button: 'left', mods: '⌘', x: 10, y: 10 });
    view = hold(view, 20.5, 10, '⌘');
    expect(view.hold).toEqual({ id: 0, button: 'left', mods: '⌘', x: 20.5, y: 10 });
    view = release(view);
    expect(view.hold).toBeNull();
    expect(release(view)).toBe(view);
    view = hold(view, 5, 5);
    expect(view.hold).toEqual({ id: 1, button: 'left', mods: null, x: 5, y: 5 });
  });

  it('moves one scroll indicator in place while scrolling continues', () => {
    let view = reduce(initial, { kind: 'scroll', x: 10, y: 20, direction: 'down' }, 0, FADE);
    view = reduce(view, { kind: 'scroll', x: 12, y: 24, direction: 'up' }, 16, FADE);
    expect(view.scroll).toEqual({ x: 12, y: 24, direction: 'up' });
  });

  it('colors back and forward like the middle button', () => {
    const colors = { left: '#111111', right: '#222222', middle: '#333333' };
    const buttons: Button[] = ['left', 'right', 'middle', 'back', 'forward'];
    expect(buttons.map(button => rippleColor(colors, button))).toEqual(['#111111', '#222222', '#333333', '#333333', '#333333']);
  });

  it('builds the pill from keys and expires it only after the fade', () => {
    let view = reduce(initial, { kind: 'key', stroke: { kind: 'chord', label: '⇧⌘P' } }, 1000, FADE);
    view = reduce(view, { kind: 'key', stroke: { kind: 'chord', label: '⇧⌘P' } }, 2000, FADE);
    expect(view.pill).toEqual({ chips: [{ kind: 'chord', label: '⇧⌘P', count: 2 }], lastAt: 2000 });
    expect(expirePill(view, 2500, FADE)).toBe(view);
    expect(expirePill(view, 3500, FADE).pill).toBeNull();
  });

  it('anchors the pill to each position', () => {
    expect(pillAnchor('bottom-center')).toBe('left: 50%; bottom: 72px; transform: translateX(-50%)');
    expect(pillAnchor('top-right')).toBe('right: 24px; top: 48px');
  });
});
