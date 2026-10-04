import { describe, expect, it } from 'vitest';
import { MOCK_SPECS, buildResult } from './scans';
import { pickNotes } from '../notes';

const byN = (n: number) => buildResult(MOCK_SPECS.find((s) => s.n === n)!, 'radiocarbon', 'hires');

describe('mock session covers every state the screen must show', () => {
  it('has each radiocarbon verdict, and no contamination "Can\'t tell" (DECISIONS 53)', () => {
    const vs = new Set(MOCK_SPECS.map((s) => buildResult(s, 'radiocarbon', 'hires').verdict));
    for (const v of ['good', 'borderline', 'unlikely', 'rescan', 'not_bone']) expect(vs.has(v as never)).toBe(true);
    expect(vs.has('cant_tell')).toBe(false);
  });

  it("the ZooMS band check can say Can't tell on noisy scans; that never makes a ZooMS line", () => {
    const r = byN(36);
    expect(r.zooms.verdict).toBe('cant_tell');
    expect(r.notes.some((n) => n.key.startsWith('zooms_better'))).toBe(false);
  });

  it("Matt's case: Borderline ≈ 1.2%, all six bands lit, clean → the ZooMS line and positive signs", () => {
    const r = byN(39);
    expect(r.verdict).toBe('borderline');
    expect(r.zooms.litCount).toBe(6);
    expect(pickNotes(r.notes).headline.map((n) => n.key)).toEqual(['zooms_better_good', 'positive_signs_all_six']);
  });

  it('the example session shows every ZooMS line and the faint sign (DECISIONS 80 amended), never on Good', () => {
    const all = MOCK_SPECS.map((s) => buildResult(s, 'radiocarbon', 'hires'));
    const keys = new Set(all.flatMap((r) => r.notes.map((n) => n.key)));
    for (const k of ['zooms_better_good', 'zooms_better_protein', 'zooms_better_1545', 'zooms_better_1545_flat', 'zooms_better_ryder', 'zooms_faint_protein'])
      expect(keys.has(k as never), k).toBe(true);
    for (const r of all) {
      const line = r.notes.find((n) => n.key.startsWith('zooms_'));
      if (line) expect(['unlikely', 'borderline']).toContain(r.verdict);
      if (r.verdict === 'good') expect(line).toBeUndefined();
    }
  });

  it('the faint protein sign is the quietest note: it takes a slot only when one is free', () => {
    const r = byN(26);
    expect(pickNotes(r.notes).headline.map((n) => n.key)).toEqual(['models_disagree', 'zooms_faint_protein']);
    expect(byN(34).notes.map((n) => n.key)).toContain('zooms_better_protein');
  });

  it('a fired wax / ester / plaster / C1 sign suppresses the ZooMS line; the heat sign does not', () => {
    const wax = byN(30);
    expect(wax.signs.find((s) => s.id === 'wax')?.fired).toBe(true);
    expect(wax.zooms.verdict).toBe('good');
    expect(wax.notes.some((n) => n.key.startsWith('zooms_better'))).toBe(false);
    const heat = byN(23);
    expect(heat.signs.find((s) => s.id === 'burnt')?.fired).toBe(true);
    expect(heat.notes.map((n) => n.key)).toContain('zooms_better_good');
  });

  it('zooms_vote_preserves_six_band_pattern_and_counts', () => {
    // six flat bands plus a lit 1545 band: pattern A over the six, 6 readable, 0 lit; the vote lifts the call only
    const r = byN(28);
    expect(r.evidence.bands.some((b) => b.id === 'nh1545' && b.lit)).toBe(true);
    expect(r.zooms).toMatchObject({ pattern: 'A', readableCount: 6, litCount: 0, vote1545: true, verdict: 'borderline' });
    expect(r.notes.map((n) => n.key)).toContain('zooms_better_1545_flat');
  });

  it('a consolidant flag never changes the verdict', () => {
    const r = byN(32);
    expect(r.signs.find((s) => s.id === 'ester')?.fired).toBe(true);
    expect(r.verdict).toBe('good');
  });

  it('shows lift, blocked lift, flat bands, contaminants-not-checked and the second opinion', () => {
    expect(byN(31).verdictRule).toBe('lift_good');
    expect(byN(30).notes.map((n) => n.key)).toContain('lift_blocked');
    expect(byN(37).verdictRule).toBe('flat_bands');
    expect(byN(38).notes.map((n) => n.key)).toContain('contaminants_not_checked');
    expect(byN(25).notes.map((n) => n.key)).toContain('second_opinion_differs');
  });
});
