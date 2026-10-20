import { describe, expect, it } from 'vitest';
import { LOWRANGE_EXPLAIN, NOTE_TEXT, flatBandsCard, noteParts, ryderPct } from './strings';

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

describe('the reduced contaminant checks (SWIR1 fallbacks) say which ran and what they can miss', () => {
  it('plaster and C1, C1 alone, plaster alone', () => {
    const both = NOTE_TEXT.signs_reduced({ signs: ['plaster', 'C1'] });
    expect(both).toContain('plaster and general organic checks ran in a reduced form, below 1800 nm only');
    expect(both).toContain('may let them run');
    expect(NOTE_TEXT.signs_reduced({ signs: ['C1'] })).toContain('misses thin coats, glue and many consolidants');
    expect(NOTE_TEXT.signs_reduced({ signs: ['C1'] })).not.toContain('plaster');
    expect(NOTE_TEXT.signs_reduced({ signs: ['plaster'] })).toBe(
      'The plaster check ran on its 1446 nm band only. Above 1800 nm this scan is too noisy for the full checks; more averages (100–200) may let them run. In tests the reduced plaster check found most added plaster.',
    );
  });
});

describe('band noise unknown (DECISIONS 137): plain words, faint bands not read', () => {
  it('all bands (a correction without noise propagation) or the bands named, by wavelength', () => {
    const seven = ['ch1689', 'ch1728', 'nh2044', 'amide2175', 'ch2262', 'ch2284', 'nh1545'];
    expect(NOTE_TEXT.band_noise_unknown({ bands: seven })).toBe(
      'Band noise could not be estimated for this correction, so faint bands are not read; only strong bands count.',
    );
    expect(NOTE_TEXT.band_noise_unknown({ bands: ['ch1728'] })).toBe(
      'Band noise could not be estimated at 1728 nm on this instrument, so a faint band there is not read; only a strong band counts.',
    );
    expect(NOTE_TEXT.band_noise_unknown({ bands: ['ch2284', 'ch1689', 'ch1728'] })).toContain('at 1689, 1728 and 2284 nm');
    expect(NOTE_TEXT.band_noise_unknown({})).toContain('for this correction');
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

describe('the panel notes (2026-10-18) and the shortened models-disagree note', () => {
  it('the C–H note has two levels, both naming examples, neither claiming certainty', () => {
    expect(NOTE_TEXT.ch_above_nh_possible({ ch: 3.1, m: 0.4 })).toBe(
      "Possible extra organic material: the C–H band reads higher than the protein (N–H) bands explain (the bone's own fat, wax or consolidant, for example).",
    );
    expect(NOTE_TEXT.ch_above_nh_strong({ ch: 5.2, m: 0.4 })).toBe(
      "Likely extra organic material: the C–H band reads far higher than the protein (N–H) bands explain (the bone's own fat, wax or consolidant, for example).",
    );
    expect(NOTE_TEXT.ch_above_nh_strong({})).not.toMatch(/certain/i);
  });
  it('the strong-disagreement note names no cause', () => {
    const t = NOTE_TEXT.readers_disagree_strongly({ contrasts: ['GL'], m: 3.4 });
    expect(t).toBe('Collagen estimates disagree strongly. Treat the percentage with caution; the cause is unknown.');
    // the verdict card sets the lead in bold; the rest reads on from it
    expect(noteParts({ key: 'readers_disagree_strongly', params: {} }).rest).toBe('. Treat the percentage with caution; the cause is unknown.');
    expect(t).not.toMatch(/glue|coat|contamin/i);
  });
  it('models disagree keeps only the verdict-line sentence', () => {
    const t = NOTE_TEXT.models_disagree({ a: 0.4, b: 0.9, c: 4.1 });
    expect(t).toBe('The three collagen models fall on different sides of a verdict line (0.4%, 0.9%, 4.1%); the verdict uses the middle value.');
    expect(t).not.toContain('coated');
  });
  it('the caution notes split into a bold lead and the rest; other notes have no lead', () => {
    expect(noteParts({ key: 'ch_above_nh_strong', params: {} })).toEqual({
      lead: 'Likely extra organic material',
      rest: ": the C–H band reads far higher than the protein (N–H) bands explain (the bone's own fat, wax or consolidant, for example).",
    });
    expect(noteParts({ key: 'readers_disagree_strongly', params: {} }).lead).toBe('Collagen estimates disagree strongly');
    expect(noteParts({ key: 'ch_above_nh_possible', params: {} }).lead).toBe('Possible extra organic material');
    const m = noteParts({ key: 'models_disagree', params: { a: 0.4, b: 0.9, c: 4.1 } });
    expect(m.lead).toBe('');
    expect(m.rest).toContain('different sides of a verdict line');
  });
  it('the flat-bands card note gives only the reason (the headline and the reading sit above it)', () => {
    expect(flatBandsCard({ m: 1.4 })).toBe('The protein bands are flat, so the verdict follows the bands, not the models.');
    expect(flatBandsCard({ m: 0.2 })).toBe('The protein bands are flat.');
  });
  it('the low-range explanation', () => {
    expect(LOWRANGE_EXPLAIN).toBe('Tuned for low collagen; often reads lower at 3–6%.');
  });
});
