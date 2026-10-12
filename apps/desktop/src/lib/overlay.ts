import type { Button, OverlayEvent, PillPosition, Point, Preferences, ScrollDirection } from './native';
import { pushStroke, type Pill } from './pill';

export interface Ripple { id: number; button: Button; mods: string | null; x: number; y: number }
export interface Scroll { x: number; y: number; direction: ScrollDirection }
export interface OverlayView { ripples: Ripple[]; halo: Point | null; scroll: Scroll | null; pill: Pill | null; nextId: number }
export const initial: OverlayView = { ripples: [], halo: null, scroll: null, pill: null, nextId: 0 };
/** Back and forward borrow the middle color, like the "other" buttons they are on macOS. A new nested
 * `rippleColors` field would fail to parse in every saved preferences file and reset the user's colors. */
export const rippleColor = (colors: Preferences['rippleColors'], button: Button): string =>
  button === 'back' || button === 'forward' ? colors.middle : colors[button];
export const BUTTON_GLYPHS: Partial<Record<Button, string>> = { back: '‹', forward: '›' };
export const SCROLL_GLYPHS: Record<ScrollDirection, string> = { up: '↑', down: '↓', left: '←', right: '→' };
export const MAX_RIPPLES = 24;

export function reduce(view: OverlayView, event: OverlayEvent, now: number, fadeMs: number): OverlayView {
  switch (event.kind) {
    case 'click': {
      const ripple = { id: view.nextId, button: event.button, mods: event.mods, x: event.x, y: event.y };
      return { ...view, ripples: [...view.ripples, ripple].slice(-MAX_RIPPLES), nextId: view.nextId + 1 };
    }
    case 'halo': return { ...view, halo: event.at };
    case 'scroll': return { ...view, scroll: { x: event.x, y: event.y, direction: event.direction } };
    case 'key': return { ...view, pill: pushStroke(view.pill, event.stroke, now, fadeMs) };
  }
}
export function dropRipple(view: OverlayView, id: number): OverlayView {
  return { ...view, ripples: view.ripples.filter(ripple => ripple.id !== id) };
}
export function expirePill(view: OverlayView, now: number, fadeMs: number): OverlayView {
  return view.pill && now - view.pill.lastAt >= fadeMs ? { ...view, pill: null } : view;
}

export function pillAnchor(position: PillPosition): string {
  switch (position) {
    case 'bottom-center': return 'left: 50%; bottom: 72px; transform: translateX(-50%)';
    case 'bottom-left': return 'left: 24px; bottom: 72px';
    case 'bottom-right': return 'right: 24px; bottom: 72px';
    case 'top-left': return 'left: 24px; top: 48px';
    case 'top-right': return 'right: 24px; top: 48px';
  }
}
