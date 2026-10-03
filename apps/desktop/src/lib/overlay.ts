import type { Button, OverlayEvent, PillPosition, Point } from './native';
import { pushStroke, type Pill } from './pill';

export interface Ripple { id: number; button: Button; x: number; y: number }
export interface OverlayView { ripples: Ripple[]; halo: Point | null; pill: Pill | null; nextId: number }
export const initial: OverlayView = { ripples: [], halo: null, pill: null, nextId: 0 };
/** A click storm cannot grow the DOM past this many ripples. */
export const MAX_RIPPLES = 24;

export function reduce(view: OverlayView, event: OverlayEvent, now: number, fadeMs: number): OverlayView {
  switch (event.kind) {
    case 'click': {
      const ripple = { id: view.nextId, button: event.button, x: event.x, y: event.y };
      return { ...view, ripples: [...view.ripples, ripple].slice(-MAX_RIPPLES), nextId: view.nextId + 1 };
    }
    case 'halo': return { ...view, halo: event.at };
    case 'key': return { ...view, pill: pushStroke(view.pill, event.stroke, now, fadeMs) };
  }
}
/** Removes a ripple once its animation ends. */
export function dropRipple(view: OverlayView, id: number): OverlayView {
  return { ...view, ripples: view.ripples.filter(ripple => ripple.id !== id) };
}
/** Clears a faded pill, unless a newer stroke arrived after the timer was set. */
export function expirePill(view: OverlayView, now: number, fadeMs: number): OverlayView {
  return view.pill && now - view.pill.lastAt >= fadeMs ? { ...view, pill: null } : view;
}

/** Pill placement: 24px from the screen edges, bottom center raised to clear docks and taskbars. */
export function pillAnchor(position: PillPosition): string {
  switch (position) {
    case 'bottom-center': return 'left: 50%; bottom: 72px; transform: translateX(-50%)';
    case 'bottom-left': return 'left: 24px; bottom: 72px';
    case 'bottom-right': return 'right: 24px; bottom: 72px';
    case 'top-left': return 'left: 24px; top: 48px';
    case 'top-right': return 'right: 24px; top: 48px';
  }
}
