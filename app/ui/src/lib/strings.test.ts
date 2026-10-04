import { describe, expect, it } from 'vitest';
import { NOTE_TEXT, ryderPct } from './strings';

describe('the Ryder 2045 ZooMS line', () => {
  it('one decimal, two where one decimal would fall below the cut; the cut comes from the core', () => {
    expect(ryderPct({ r: 0.44, at_least: 0.34 })).toBe('0.4');
    expect(ryderPct({ r: 0.35, at_least: 0.34 })).toBe('0.35');
    expect(ryderPct({ r: 0.34, at_least: 0.34 })).toBe('0.34');
    expect(ryderPct({ r: 1.26, at_least: 0.34 })).toBe('1.3');
    expect(NOTE_TEXT.zooms_better_ryder({ r: 0.35, at_least: 0.34 })).toBe(
      'Some chance with ZooMS: the published Ryder 2045 model reads 0.35%, above its ZooMS line (0.34%).',
    );
  });
});

describe('the faint protein sign names what shows', () => {
  it('2175 alone, one other band, or two bands at a very low reading', () => {
    expect(NOTE_TEXT.zooms_faint_protein({ lit: ['amide2175'] })).toContain('only the 2175 nm protein band');
    expect(NOTE_TEXT.zooms_faint_protein({ lit: ['nh2044'] })).toContain('only one protein band');
    expect(NOTE_TEXT.zooms_faint_protein({ lit: ['amide2175', 'nh1545'], m: 0.2 })).toContain('collagen models read very low');
  });
});
import { allStrings, flagText, noteText } from './strings';

describe('UI string table lint', () => {
  const all = allStrings();
  it('never calls bone "wet" (DECISIONS 33)', () => {
    for (const s of all) expect(s.toLowerCase()).not.toMatch(/\bwet\b|water-rich/);
  });
  it('has no alarming words or exclamation marks', () => {
    for (const s of all) {
      expect(s).not.toMatch(/!/);
      expect(s.toLowerCase()).not.toMatch(/\bfail|\breject/);
    }
  });
  it('uses the agreed note and flag texts', () => {
    expect(noteText({ key: 'positive_signs_all_six', params: {} })).toBe(
      'Positive signs: all six collagen bands are resolved and no contaminant sign fired.',
    );
    expect(noteText({ key: 'contaminants_not_checked', params: {} })).toBe(
      'Contaminant signs could not be checked: this scan is too noisy. A rescan with more averages (100–200) would allow it.',
    );
    expect(flagText('ester')).toBe(
      'Consolidant sign at this spot. Many collagen models still read well on treated bone, but check the spot and clean it if you can.',
    );
    expect(flagText('burnt', 'charred')).toBe(
      'Heat sign at this spot (charred: a dark visible edge). Heating lowers collagen; the readings already reflect that.',
    );
    expect(flagText('burnt', 'calcined')).toContain('(calcined: sharp OH peaks at 979 and 1433 nm)');
    expect(flagText('burnt')).toBe('Heat sign at this spot. Heating lowers collagen; the readings already reflect that.');
    expect(noteText({ key: 'flat_bands', params: { m: 1.4 } })).toContain('≈ 1.4% (± 1)');
  });
});
