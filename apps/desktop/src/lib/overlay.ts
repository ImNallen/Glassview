import type { Button, HoldView, OverlayEvent, PillPosition, Point, ScrollDirection } from './native';
import { pushStroke, type Pill } from './pill';

export interface Ripple { id: number; button: Button; mods: string | null; x: number; y: number }
export interface Hold extends HoldView { id: number; path: Point[] }
export interface Trail { id: number; button: Button; path: Point[] }
export interface Scroll { x: number; y: number; direction: ScrollDirection }
export interface OverlayView { ripples: Ripple[]; halo: Point | null; hold: Hold | null; trails: Trail[]; scroll: Scroll | null; pill: Pill | null; nextId: number }
export const initial: OverlayView = { ripples: [], halo: null, hold: null, trails: [], scroll: null, pill: null, nextId: 0 };
export const SCROLL_GLYPHS: Record<ScrollDirection, string> = { up: '↑', down: '↓', left: '←', right: '→' };
export const MAX_RIPPLES = 24;
const MAX_TRAILS = 8;
export const MAX_PATH = 400;
/** CSS px the pointer may wander during a click before it counts as a drag. */
export const DRAG_SLOP = 4;
const MIN_SEGMENT = 1;

export function reduce(view: OverlayView, event: OverlayEvent, now: number, fadeMs: number): OverlayView {
  switch (event.kind) {
    case 'click': {
      const ripple = { id: view.nextId, button: event.button, mods: event.mods, x: event.x, y: event.y };
      return { ...view, ripples: [...view.ripples, ripple].slice(-MAX_RIPPLES), nextId: view.nextId + 1 };
    }
    case 'halo': return { ...view, halo: event.at };
    case 'hold': return event.hold ? moveHold(view, event.hold) : endHold(view);
    case 'scroll': return { ...view, scroll: { x: event.x, y: event.y, direction: event.direction } };
    case 'key': return { ...view, pill: pushStroke(view.pill, event.stroke, now, fadeMs) };
  }
}
function moveHold(view: OverlayView, next: HoldView): OverlayView {
  const point = { x: next.x, y: next.y };
  if (!view.hold) return { ...view, hold: { ...next, id: view.nextId, path: [point] }, nextId: view.nextId + 1 };
  const { path } = view.hold;
  const last = path[path.length - 1];
  const moved = Math.hypot(point.x - last.x, point.y - last.y) >= (path.length === 1 ? DRAG_SLOP : MIN_SEGMENT);
  const grown = moved ? [...path, point] : path;
  return { ...view, hold: { ...view.hold, ...point, path: grown.length > MAX_PATH ? [grown[0], ...grown.slice(1 - MAX_PATH)] : grown } };
}
function endHold(view: OverlayView): OverlayView {
  const { hold } = view;
  if (!hold) return view;
  if (hold.path.length === 1) return { ...view, hold: null };
  return { ...view, hold: null, trails: [...view.trails, { id: hold.id, button: hold.button, path: hold.path }].slice(-MAX_TRAILS) };
}
export function dropTrail(view: OverlayView, id: number): OverlayView {
  return { ...view, trails: view.trails.filter(trail => trail.id !== id) };
}
export const pathPoints = (path: Point[]): string => path.map(({ x, y }) => `${x},${y}`).join(' ');
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
