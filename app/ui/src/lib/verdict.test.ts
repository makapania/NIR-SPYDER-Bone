import { describe, expect, it } from 'vitest';
import { PROMISING_ORDER, VERDICT_META, toneForReading, toneVar, verdictSentence } from './verdict';
import type { Verdict } from './types';

const ALL: Verdict[] = ['good', 'borderline', 'unlikely', 'cant_tell', 'rescan', 'not_bone'];

describe('verdict glyph and colour mapping', () => {
  it('maps each verdict to the design-rev-3 shape and tone', () => {
    expect(VERDICT_META.good).toMatchObject({ word: 'Good candidate', shape: 'full', tone: 'good', fill: 1 });
    expect(VERDICT_META.borderline).toMatchObject({ shape: 'half', tone: 'mid', fill: 0.5 });
    expect(VERDICT_META.unlikely).toMatchObject({ shape: 'third', tone: 'low', fill: 0.3 });
    expect(VERDICT_META.cant_tell).toMatchObject({ shape: 'question', tone: 'none' });
    expect(VERDICT_META.rescan).toMatchObject({ shape: 'dashed', tone: 'none' });
    expect(VERDICT_META.not_bone).toMatchObject({ word: "Doesn't look like bone", shape: 'diamond', tone: 'none' });
  });
  it('reads without colour: every verdict has a different shape', () => {
    const shapes = new Set(ALL.map((v) => VERDICT_META[v].shape));
    expect(shapes.size).toBe(ALL.length);
  });
  it('builds the fixed colour tokens', () => {
    expect(toneVar('good')).toBe('var(--v-good)');
    expect(toneVar('mid', 'ink')).toBe('var(--v-mid-ink)');
    expect(toneVar('low', 'tint')).toBe('var(--v-low-tint)');
  });
  it('tones a bare reading by the 0.5 / 3 screening lines', () => {
    expect(toneForReading(0.49)).toBe('low');
    expect(toneForReading(0.5)).toBe('mid');
    expect(toneForReading(3)).toBe('good');
    expect(toneForReading(null)).toBe('none');
  });
  it('one verdict for every analysis: names them, never quotes a screening line, never denies ZooMS below Good', () => {
    for (const v of ALL) expect(verdictSentence(v)).not.toMatch(/\d+(\.\d+)?\s*%/);
    expect(verdictSentence('good')).toContain('ZooMS');
    expect(verdictSentence('borderline')).toContain('radiocarbon or isotope analysis');
    for (const v of ['borderline', 'unlikely'] as const) expect(verdictSentence(v)).not.toContain('ZooMS');
  });
  it("orders most promising first: Good, Borderline, Can't tell, Unlikely, then the rest", () => {
    const order = [...ALL].sort((a, b) => PROMISING_ORDER[a] - PROMISING_ORDER[b]);
    expect(order.slice(0, 4)).toEqual(['good', 'borderline', 'cant_tell', 'unlikely']);
  });
});
