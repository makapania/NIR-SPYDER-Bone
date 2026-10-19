import { describe, expect, it } from 'vitest';
import { cardItems } from './card';
import { MOCK_SPECS, buildResult } from './mock/scans';
import { NOTE_SHORT, NOTE_LEAD } from './strings';
import type { NoteKey, ScanResult } from './types';

const byN = (n: number) => buildResult(MOCK_SPECS.find((s) => s.n === n)!, 'radiocarbon', 'hires');
const ids = (xs: { id: string }[]) => xs.map((x) => x.id);

describe('the compact card items', () => {
  it('the crowded card: flags, then both warnings (C–H first), then the notes; nothing dropped', () => {
    const { alerts, notes } = cardItems(byN(42));
    expect(ids(alerts)).toEqual(['flag:burnt', 'ch_above_nh_strong', 'readers_disagree_strongly']);
    expect(alerts.map((a) => a.tone)).toEqual(['flag', 'warn', 'warn']);
    expect(ids(notes)).toEqual(['lift_borderline', 'zooms_better_good']);
    expect(notes.map((n) => n.tone)).toEqual(['note', 'zooms']);
  });

  it('a warning keeps its heading as the label; the named pair and readings are its full text', () => {
    const w = cardItems(byN(41)).alerts.find((a) => a.id === 'readers_disagree_strongly')!;
    expect(w.label).toBe('Collagen estimates disagree strongly');
    expect(w.body).toMatch(/^The narrow 1545 nm band reads 6\.6% against the consensus 13\.6%/);
    expect(w.body).toContain('Treat the percentage with caution');
  });

  it("a full text that opens with its label reads on from it; others are kept whole", () => {
    const { notes } = cardItems(byN(41));
    const coat = notes.find((n) => n.id === 'possible_thin_coating')!;
    expect(coat.label).toBe('Possible thin coating');
    expect(coat.body).toBe('A weak sign of glue or consolidant at this spot. It may be nothing, but check the spot before sampling.');
    const lift = cardItems(byN(42)).notes[0];
    expect(lift.label).toBe('Raised to Borderline');
    expect(lift.body).toMatch(/^The collagen bands are resolved/);
    const zooms = cardItems(byN(21)).notes.find((n) => n.id === 'zooms_better_1545')!;
    expect(zooms.label).toBe('Some chance with ZooMS');
    expect(zooms.body).toBe('The 1545 nm collagen band shows at this spot.');
  });

  it('the flat-bands verdict note gives the reason only, as the card did', () => {
    const r = byN(37);
    expect(r.verdictRule).toBe('flat_bands');
    const flat = cardItems(r).notes.find((n) => n.id === 'flat_bands')!;
    expect(flat.label).toBe('Protein bands flat');
    expect(flat.body).toMatch(/^The protein bands are flat/);
  });

  it('a fired warning the core left out of notesShown is still an item; a quiet note keeps its slot', () => {
    const r: ScanResult = { ...byN(41), notesShown: ['possible_thin_coating'] };
    const { alerts, notes } = cardItems(r);
    expect(ids(alerts)).toEqual(['readers_disagree_strongly']);
    expect(ids(notes)).toEqual(['possible_thin_coating']);
  });

  it('every note key has a short label, no longer than its lead and never more than 36 characters', () => {
    for (const [k, v] of Object.entries(NOTE_SHORT) as [NoteKey, string][]) {
      expect(v.length, k).toBeGreaterThan(0);
      expect(v.length, k).toBeLessThanOrEqual(36);
      const lead = NOTE_LEAD[k];
      if (lead) expect(v, k).toBe(lead);
    }
  });
});
