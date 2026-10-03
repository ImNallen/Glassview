import { describe, expect, it } from 'vitest';
import { chipLabel, pushStroke, type Pill } from './pill';
import type { StrokeView } from './native';

const FADE = 1500;
const text = (text: string): StrokeView => ({ kind: 'text', text });
const chord = (label: string): StrokeView => ({ kind: 'chord', label });
/** Pushes each stroke 100 ms after the previous one and renders chips as "label ×count". */
function typed(strokes: StrokeView[], pill: Pill | null = null, start = 0): string[] {
  let at = start;
  for (const stroke of strokes) pill = pushStroke(pill, stroke, at += 100, FADE);
  return pill!.chips.map(chip => chip.count > 1 ? `${chipLabel(chip)} ×${chip.count}` : chipLabel(chip));
}
const chars = (word: string) => [...word].map(text);

describe('pushStroke', () => {
  it.each<[string, StrokeView[], string[]]>([
    ['typed text stays one chip', chars('hello'), ['hello']],
    ['five of one key collapse', chars('jjjjj'), ['j ×5']],
    ['a trailing run splits off the text before it', chars('abjjj'), ['ab', 'j ×3']],
    ['two in a row are still text', chars('abjj'), ['abjj']],
    ['a repeated shortcut counts up', [chord('⌘Z'), chord('⌘Z'), chord('⌘Z')], ['⌘Z ×3']],
    ['different shortcuts sit side by side', [chord('⌘Z'), chord('⇧⌘Z')], ['⌘Z', '⇧⌘Z']],
    ['a repeat chip keeps counting the same key', chars('jjjjjj'), ['j ×6']],
    ['a different key after a repeat starts new text', chars('jjjab'), ['j ×3', 'ab']],
    ['text after a shortcut starts a new chip', [chord('⌘K'), ...chars('hi')], ['⌘K', 'hi']],
    ['a word then Enter', [...chars('ok'), chord('↩')], ['ok', '↩']],
    ['a held space is visible', chars('a   '), ['a', '␣ ×3']],
    ['at most five chips, newest kept', ['A', 'B', 'C', 'D', 'E', 'F'].map(label => chord(`⌘${label}`)), ['⌘B', '⌘C', '⌘D', '⌘E', '⌘F']],
    ['text past 20 characters starts a new chip', chars('abcdefghijklmnopqrstuv'), ['abcdefghijklmnopqrst', 'uv']],
  ])('%s', (_, strokes, chips) => expect(typed(strokes)).toEqual(chips));

  it('starts a fresh pill once the previous one faded, and keeps it until then', () => {
    const pill = pushStroke(null, chord('⌘Z'), 1000, FADE);
    expect(pushStroke(pill, chord('⌘Z'), 2499, FADE)).toEqual({ chips: [{ kind: 'chord', label: '⌘Z', count: 2 }], lastAt: 2499 });
    expect(pushStroke(pill, chord('⌘Z'), 2500, FADE)).toEqual({ chips: [{ kind: 'chord', label: '⌘Z', count: 1 }], lastAt: 2500 });
  });
});
