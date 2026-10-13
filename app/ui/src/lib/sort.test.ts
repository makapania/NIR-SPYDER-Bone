import { describe, expect, it } from 'vitest';
import { MOCK_SPECS, buildResult } from './mock/scans';
import { orderScans } from './sort';

describe('scan ordering', () => {
  const scans = MOCK_SPECS.map((s) => buildResult(s, 'radiocarbon', 'hires'));
  it('newest first by acquisition time', () => {
    const o = orderScans(scans, 'newest');
    expect(o[0].file).toBe('Spectrum00040.asd');
    expect(o[o.length - 1].file).toBe('Spectrum00021.asd');
  });
  it('most promising first: verdict groups, CONS3 within Good/Borderline, S within Unlikely, Rescan/not bone last', () => {
    const o = orderScans(scans, 'promising');
    const g = { good: 0, borderline: 1, cant_tell: 2, unlikely: 3, rescan: 4, not_bone: 4 } as const;
    for (let i = 1; i < o.length; i++) expect(g[o[i].verdict]).toBeGreaterThanOrEqual(g[o[i - 1].verdict]);
    // DECISIONS 99: within a group, scans with a contaminant sign (not heat) come after the clean ones
    const flagged = (s: (typeof o)[number]) => s.signs.some((x) => x.fired && x.id !== 'burnt');
    for (const [v, key] of [
      ['good', (s: (typeof o)[number]) => s.models.cons3!.value!],
      ['unlikely', (s: (typeof o)[number]) => s.evidence.s ?? -Infinity],
    ] as const) {
      const grp = o.filter((s) => s.verdict === v);
      const f = grp.map(flagged);
      expect(f).toEqual([...f].sort((a, b) => Number(a) - Number(b)));
      for (const part of [grp.filter((s) => !flagged(s)), grp.filter(flagged)]) {
        const k = part.map(key);
        expect(k).toEqual([...k].sort((a, b) => b - a));
      }
    }
    expect(o.some(flagged)).toBe(true);
  });
  it('the organic-signal filter keeps trace and above', () => {
    const o = orderScans(scans, 'newest', true);
    expect(o.every((s) => ['trace', 'clear', 'strong'].includes(s.evidence.level))).toBe(true);
    expect(o.length).toBeLessThan(scans.length);
  });
  it("follows the core's sort key when present: group ascending, value descending, a missing value last", () => {
    const base = scans[0];
    const mk = (file: string, g: number, v: number | null) => ({ ...base, file, scanId: file, sortGroup: g, sortValue: v });
    const o = orderScans([mk('a', 1, 0.7), mk('b', 0, 2), mk('c', 1, null), mk('d', 0, 5), mk('e', 1, 1.9)], 'promising');
    expect(o.map((s) => s.file)).toEqual(['d', 'b', 'e', 'a', 'c']);
  });
  it("puts the core's penalised scans after the clean ones of their group, never across groups (DECISIONS 99)", () => {
    const base = scans[0];
    const mk = (file: string, g: number, v: number, p: number) =>
      ({ ...base, file, scanId: file, sortGroup: g, sortValue: v, sortPenalty: p });
    // a flagged Good with a high reading: after every clean Good, still before any Borderline
    const o = orderScans([mk('bord', 1, 2.9, 0), mk('goodFlag', 0, 12, 1), mk('goodLow', 0, 3.1, 0), mk('goodHigh', 0, 8, 0),
      mk('bordFlag', 1, 2.5, 1)], 'promising');
    expect(o.map((s) => s.file)).toEqual(['goodHigh', 'goodLow', 'goodFlag', 'bord', 'bordFlag']);
  });
});
