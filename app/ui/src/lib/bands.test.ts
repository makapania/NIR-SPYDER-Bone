import { describe, expect, it } from 'vitest';
import { CLOSEUPS, bandAt, bandCount, bandSummaryTitle, bandTitle, bandsByCloseUp, closeUpBands, closeUpSpan, placeNumbers, stateWord } from './bands';
import { splitName } from './display';
import type { BandId, BandReading, BandState } from './types';

const NM: Record<BandId, number> = { ch1689: 1689, ch1728: 1728, nh2044: 2044, amide2175: 2175, ch2262: 2262, ch2284: 2284, nh1545: 1545 };
const band = (id: BandId, state: BandState, extra: Partial<BandReading> = {}): BandReading => ({
  id,
  nm: NM[id],
  e: 1,
  u: 1,
  readable: state !== 'cant_tell',
  lit: state === 'trace' || state === 'clear' || state === 'strong',
  state,
  ...extra,
});
// the core's order: the six, then 1545
const seven = (states: BandState[]): BandReading[] =>
  (['ch1689', 'ch1728', 'nh2044', 'amide2175', 'ch2262', 'ch2284', 'nh1545'] as BandId[]).map((id, i) => band(id, states[i]));

describe('the band close-ups', () => {
  it('show all seven bands the rules read, in wavelength order, 1545 on the OH-corrected window as a dip', () => {
    expect(CLOSEUPS.flatMap(closeUpBands)).toEqual(['nh1545', 'ch1689', 'ch1728', 'nh2044', 'amide2175', 'ch2262', 'ch2284']);
    const c1545 = CLOSEUPS[0];
    expect(c1545).toMatchObject({ view: 'D2_31_ohc', dip: true });
    // the OH-corrected curve exists only from 1500 to 1550 nm; the quiet stretch near 1500 is left out
    const [lo, hi] = c1545.windows[0].range;
    expect(lo).toBeGreaterThanOrEqual(1510);
    expect(hi).toBeGreaterThanOrEqual(1550);
    expect(hi).toBeLessThanOrEqual(1555);
    expect(CLOSEUPS.slice(1).every((c) => c.view === 'D2_31' && !c.dip)).toBe(true);
  });

  it('cut each window to its bands, with room for the band and its read window (±2 nm)', () => {
    for (const c of CLOSEUPS)
      for (const w of c.windows)
        for (const id of w.bands) {
          expect(NM[id] - 2).toBeGreaterThan(w.range[0]);
          expect(NM[id] + 2).toBeLessThan(w.range[1]);
        }
    // the protein close-up skips the stretch that carries neither band (an axis break)
    const protein = CLOSEUPS.find((c) => c.family === 'Protein')!;
    expect(protein.windows).toHaveLength(2);
    expect(protein.windows[0].range[1]).toBeLessThan(protein.windows[1].range[0]);
    expect(closeUpSpan(protein)).toBe(60 + 75);
  });

  it('group a scan’s bands as the close-ups do, skipping a close-up whose band is absent', () => {
    const b = seven(['clear', 'clear', 'flat', 'strong', 'trace', 'flat', 'flat']);
    expect(bandsByCloseUp(b).map((g) => g.map((x) => x.nm))).toEqual([[1545], [1689, 1728], [2044, 2175], [2262, 2284]]);
    expect(bandsByCloseUp(b.slice(0, 6)).map((g) => g.length)).toEqual([2, 2, 2]);
  });

  it('label every band on one line: its number centred on its band, a close pair as one label centred on the pair', () => {
    const ch = CLOSEUPS[1];
    expect(bandAt(ch.windows[0], 1665)).toBe(0);
    expect(bandAt(ch.windows[0], 1755)).toBe(1);
    expect(bandAt(ch.windows[0], 1710)).toBeCloseTo(0.5);
    // one band (1545*, 2044, 2175 each in its own window): centred, no pair label
    expect(placeNumbers([30], [22])).toEqual({ shift: [0], sepAt: null });
    // two bands with room (C-H in a wide close-up): each number centred on its band
    expect(placeNumbers([20, 54], [22, 22], 8)).toEqual({ shift: [0, 0], sepAt: null });
    // too close (2262/2284 always): one label "a · b" centred on the pair's midpoint, the separator at the midpoint
    const p = placeNumbers([30, 42], [22, 22], 8, 9);
    expect(p.sepAt).toBeCloseTo(36);
    const ca = 30 + p.shift[0];
    const cb = 42 + p.shift[1];
    expect((ca + cb) / 2).toBeCloseTo(36);
    expect(cb - ca).toBeCloseTo(22 + 9);
    // the boundary: exactly the gap apart stays separate; a hair less is a pair
    expect(placeNumbers([0, 30], [22, 22], 8).sepAt).toBeNull();
    expect(placeNumbers([0, 29.9], [22, 22], 8).sepAt).not.toBeNull();
    // unequal widths: the pair label as a whole is centred on the midpoint
    const q = placeNumbers([0, 10], [20, 30], 8, 10);
    const left = 0 + q.shift[0] - 10;
    const right = 10 + q.shift[1] + 15;
    expect((left + right) / 2).toBeCloseTo(5);
  });

  it('name each band honestly in its tooltip', () => {
    expect(bandTitle(band('ch2284', 'clear'))).toContain('amide III + CH₂');
    expect(bandTitle(band('ch2284', 'clear'))).toContain('ZooMS band pattern only');
    expect(bandTitle(band('ch2262', 'flat'))).toContain('assignment uncertain');
    expect(bandTitle(band('nh2044', 'trace'))).toContain('can only add evidence');
    expect(bandTitle(band('nh1545', 'clear'))).toContain('collagen shows here as a dip, not a peak');
    expect(bandTitle(band('nh1545', 'clear'))).toContain('altered OH / bound-water');
    expect(bandTitle(band('ch1689', 'strong'))).toBe('1689 nm, C–H (CH₃): strong.');
  });

  it('say "faint" for a band lit for ZooMS but under its evidence threshold', () => {
    expect(stateWord(band('nh1545', 'trace', { evidenceState: 'faint' }))).toBe('Faint');
    expect(bandTitle(band('nh1545', 'trace', { evidenceState: 'faint' }))).toContain('counts it as flat');
    expect(stateWord(band('ch1728', 'cant_tell'))).toBe('Too noisy');
  });
});

describe('the scans list band summary', () => {
  it('counts the bands drawn lit (trace, clear, strong) out of all seven, and the noisy ones', () => {
    expect(bandCount(seven(['clear', 'strong', 'trace', 'flat', 'cant_tell', 'flat', 'clear']))).toEqual({ lit: 4, total: 7, noisy: 1 });
    expect(bandCount(seven(['flat', 'flat', 'flat', 'flat', 'flat', 'flat', 'flat']))).toEqual({ lit: 0, total: 7, noisy: 0 });
    expect(bandCount([])).toEqual({ lit: 0, total: 0, noisy: 0 });
  });

  it('says what the count covers and that it is not a vote', () => {
    const t = bandSummaryTitle('clear', seven(['clear', 'strong', 'trace', 'flat', 'cant_tell', 'flat', 'clear']));
    expect(t).toContain('Protein bands: Clear.');
    expect(t).toContain('4 of the 7 bands in the close-ups are lit, 1 too noisy');
    expect(t).toContain('not a vote');
    // every band, in wavelength order
    expect(t).toContain('1545 clear, 1689 clear, 1728 strong, 2044 trace, 2175 flat, 2262 too noisy, 2284 flat');
    expect(bandSummaryTitle('cant_tell', [])).toBe('No band reading');
  });
});

describe('middle truncation of scan names', () => {
  it('keeps the end that tells scans apart', () => {
    expect(splitName('Spectrum00039')).toEqual({ head: 'Spectr', tail: 'um00039' });
    expect(splitName('Femur_A_00012')).toEqual({ head: 'Femur', tail: '_A_00012' });
    expect(splitName('SiteA_Trench2_Layer4b_long-bone-fragment_0117_spot3_avg200').tail).toBe('_avg200');
    expect(splitName('Session2_femur-shaft_lateral_after-brushing_rep1').tail).toBe('_rep1');
  });

  it('leaves short names whole and never loses a character', () => {
    expect(splitName('Rib_rep2')).toEqual({ head: 'Rib_rep2', tail: '' });
    for (const n of ['Spectrum00039', 'a_b_c_d_e_f_g_h', 'x'.repeat(40), 'name with spaces 0001']) {
      const s = splitName(n);
      expect(s.head + s.tail).toBe(n);
      expect(s.head.length).toBeGreaterThan(0);
    }
  });
});
