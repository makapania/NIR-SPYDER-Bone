import { describe, expect, it } from 'vitest';
import {
  CLOSEUPS,
  EDGE_CLEAR,
  LABEL_FITS,
  bandAt,
  bandCount,
  bandSummaryTitle,
  bandTitle,
  bandX,
  bandsByCloseUp,
  closeUpBands,
  closeUpLabelWidth,
  closeUpSpan,
  placeNumbers,
  placeRow,
  stateWord,
  textW,
} from './bands';
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
    // widths as measured in the rendered font (here: 4 digits 22 px wide, a 9 px separator, a 5 px asterisk)
    const n4 = { w: 22, n: 4 };
    const sep = { w: 9, n: 3 };
    const at = (...xs: number[]) => xs.map((x) => ({ x, num: n4 }));
    const room: [number, number] = [-100, 200];
    // one band (1545*, 2044, 2175 each in its own window): centred, no pair label
    expect(placeNumbers(at(30), sep, room)).toEqual({ shift: [0], sepAt: null });
    // two bands with room (C-H in a wide close-up): each number centred on its band
    expect(placeNumbers(at(20, 54), sep, room)).toEqual({ shift: [0, 0], sepAt: null });
    // too close (2262/2284 always): one label "a · b" centred on the pair's midpoint, the separator at the midpoint
    const p = placeNumbers(at(30, 42), sep, room)!;
    expect(p.sepAt).toBeCloseTo(36);
    const ca = 30 + p.shift[0];
    const cb = 42 + p.shift[1];
    expect((ca + cb) / 2).toBeCloseTo(36);
    expect(cb - ca).toBeCloseTo(22 + 9);
    // the boundary: exactly the gap apart stays separate; a hair less is a pair
    expect(placeNumbers(at(0, 30), sep, room)!.sepAt).toBeNull();
    expect(placeNumbers(at(0, 29.9), sep, room)!.sepAt).not.toBeNull();
    // a wider font (the same numbers measured 26 px wide) needs the pair where the narrower one sat apart
    expect(placeNumbers([0, 30].map((x) => ({ x, num: { w: 26, n: 4 } })), sep, room)!.sepAt).not.toBeNull();
    // unequal widths: the pair label as a whole is centred on the midpoint
    const q = placeNumbers([{ x: 0, num: { w: 20, n: 4 } }, { x: 10, num: { w: 30, n: 4 } }], { w: 10, n: 3 }, room)!;
    const left = 0 + q.shift[0] - 10;
    const right = 10 + q.shift[1] + 15;
    expect((left + right) / 2).toBeCloseTo(5);
  });

  it('keep every number inside its close-up, whatever the font: a pair label, then tighter or smaller type, never over the edge', () => {
    const sep = { w: 9, n: 3 };
    // a number centred near the box edge does not fit: no placement at that type
    expect(placeNumbers([{ x: 40, num: { w: 26, n: 4 }, tail: { w: 6, n: 1 } }], sep, [0, 54])).toBeNull();
    // the asterisk counts on the right but not for centring
    expect(placeNumbers([{ x: 40, num: { w: 26, n: 4 }, tail: { w: 6, n: 1 } }], sep, [0, 60])).toEqual({ shift: [0], sepAt: null });
    // side by side over the edge: the pair label, centred on the pair, inside the room
    const pr = placeNumbers([{ x: 6, num: { w: 22, n: 4 } }, { x: 46, num: { w: 22, n: 4 } }], sep, [-4, 56])!;
    expect(pr.sepAt).toBeCloseTo(26);
    // the row tightens or shrinks the type only when a window does not fit at the design size
    const wide = { w: 28, n: 4 };
    const roomy = placeRow([{ nums: [{ x: 30, num: wide }], room: [0, 60] }], sep);
    expect(roomy).toMatchObject({ fit: 0, squeezed: false });
    const tight = placeRow([{ nums: [{ x: 30, num: wide }], room: [16, 44] }], sep);
    expect(tight.fit).toBeGreaterThan(0);
    expect(tight.squeezed).toBe(false);
    expect(LABEL_FITS[tight.fit].fs).toBeGreaterThanOrEqual(10);
    const w = textW(wide, LABEL_FITS[tight.fit]);
    expect(30 - w / 2).toBeGreaterThanOrEqual(16 + EDGE_CLEAR - 1e-6);
    expect(30 + w / 2).toBeLessThanOrEqual(44 - EDGE_CLEAR + 1e-6);
    // nothing fits even at the smallest type: moved inside the room as a whole (off-centre, never over the edge)
    const sq = placeRow([{ nums: [{ x: 4, num: wide }], room: [0, 60] }], sep);
    expect(sq.squeezed).toBe(true);
    const l = 4 + sq.placed[0].shift[0] - textW(wide, LABEL_FITS[sq.fit]) / 2;
    expect(l).toBeCloseTo(EDGE_CLEAR);
    // a close-up's minimum width grows with the measured text: the 1545 close-up for "1545*" in a wide font
    const t = (w4: number) => ({ nums: { nh1545: { w: w4, n: 4 } }, tail: { w: w4 / 4, n: 1 }, sep });
    const frame = { pad: 4, border: 1, brk: 12 };
    const narrowFont = closeUpLabelWidth(CLOSEUPS[0], t(22), frame);
    const wideFont = closeUpLabelWidth(CLOSEUPS[0], t(28), frame);
    expect(wideFont).toBeGreaterThan(narrowFont);
    // and the 1545 number fits in a close-up that wide: x at 3/4 of the window, the asterisk inside the right edge
    const ww = wideFont - 2 * (frame.pad + frame.border);
    expect(placeNumbers([{ x: bandX(CLOSEUPS[0].windows[0], 1545, ww), num: { w: 28, n: 4 }, tail: { w: 7, n: 1 } }], sep, [-4, ww + 4])).not.toBeNull();
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
