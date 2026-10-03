import type { StrokeView } from './native';

export interface Chip { kind: 'chord' | 'text'; label: string; count: number }
export interface Pill { chips: Chip[]; lastAt: number }

export const MAX_CHIPS = 5;
export const MAX_TEXT = 20;
const REPEAT_THRESHOLD = 3;

export function pushStroke(pill: Pill | null, stroke: StrokeView, now: number, fadeMs: number): Pill {
  const chips = pill && now - pill.lastAt < fadeMs ? pill.chips : [];
  const last = chips.at(-1);
  const rest = chips.slice(0, -1);
  let next: Chip[];
  if (stroke.kind === 'chord') {
    next = last?.kind === 'chord' && last.label === stroke.label
      ? [...rest, { ...last, count: last.count + 1 }]
      : [...chips, { kind: 'chord', label: stroke.label, count: 1 }];
  } else {
    const c = stroke.text;
    if (last?.kind !== 'text') next = [...chips, { kind: 'text', label: c, count: 1 }];
    else if (last.count > 1) next = last.label === c ? [...rest, { ...last, count: last.count + 1 }] : [...chips, { kind: 'text', label: c, count: 1 }];
    else if ([...last.label].slice(1 - REPEAT_THRESHOLD).join('') === c.repeat(REPEAT_THRESHOLD - 1)) {
      const run = [...last.label].slice(0, 1 - REPEAT_THRESHOLD).join('');
      next = [...rest, ...(run ? [{ ...last, label: run }] : []), { kind: 'text', label: c, count: REPEAT_THRESHOLD }];
    }
    else if ([...last.label].length >= MAX_TEXT) next = [...chips, { kind: 'text', label: c, count: 1 }];
    else next = [...rest, { ...last, label: last.label + c }];
  }
  return { chips: next.slice(-MAX_CHIPS), lastAt: now };
}

export function chipLabel(chip: Chip): string {
  return chip.kind === 'text' && chip.count > 1 && chip.label === ' ' ? '␣' : chip.label;
}
