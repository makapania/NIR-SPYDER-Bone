import { describe, expect, it } from 'vitest';
import { noteIcon, pickNotes, strongPanelNote } from './notes';
import type { Note, NoteKey } from './types';

const n = (key: NoteKey): Note => ({ key, params: {} });

describe('note precedence: at most two under the verdict', () => {
  it('puts the rule note first, then one supporting note, the rest in details', () => {
    const p = pickNotes([n('bands_too_noisy'), n('positive_signs_all_six'), n('lift_good'), n('models_disagree'), n('second_opinion_differs')]);
    expect(p.headline.map((x) => x.key)).toEqual(['lift_good', 'models_disagree']);
    expect(p.details.map((x) => x.key)).toEqual(['bands_too_noisy', 'positive_signs_all_six', 'second_opinion_differs']);
  });
  it('orders rule notes +D > flat bands > lift > blocked lift', () => {
    expect(pickNotes([n('lift_blocked'), n('flat_bands'), n('plus_d')]).headline[0].key).toBe('plus_d');
    expect(pickNotes([n('lift_blocked'), n('lift_borderline')]).headline[0].key).toBe('lift_borderline');
  });
  it('orders supporting notes: contaminants not checked > models disagree > positive signs > bands too noisy', () => {
    const p = pickNotes([n('bands_too_noisy'), n('positive_signs_clear'), n('contaminants_not_checked'), n('models_disagree')]);
    expect(p.headline.map((x) => x.key)).toEqual(['contaminants_not_checked']);
  });
  it('the ZooMS line comes after the rule note and before the supporting note, two at most (DECISIONS 80 amended)', () => {
    expect(pickNotes([n('positive_signs_all_six'), n('zooms_better_good')]).headline.map((x) => x.key)).toEqual([
      'zooms_better_good',
      'positive_signs_all_six',
    ]);
    const p = pickNotes([n('models_disagree'), n('zooms_better_1545'), n('flat_bands')]);
    expect(p.headline.map((x) => x.key)).toEqual(['flat_bands', 'zooms_better_1545']);
    expect(p.details.map((x) => x.key)).toEqual(['models_disagree']);
  });
  it('the faint protein sign shows only when a slot is left after the rule and supporting notes', () => {
    expect(pickNotes([n('zooms_faint_protein'), n('models_disagree')]).headline.map((x) => x.key)).toEqual([
      'models_disagree',
      'zooms_faint_protein',
    ]);
    const p = pickNotes([n('zooms_faint_protein'), n('models_disagree'), n('flat_bands')]);
    expect(p.headline.map((x) => x.key)).toEqual(['flat_bands', 'models_disagree']);
    expect(p.details.map((x) => x.key)).toEqual(['zooms_faint_protein']);
  });
  it('a Ryder 2045 line shows instead of the faint sign, which goes to the details', () => {
    const p = pickNotes([n('zooms_better_ryder'), n('zooms_faint_protein')]);
    expect(p.headline.map((x) => x.key)).toEqual(['zooms_better_ryder']);
    expect(p.details.map((x) => x.key)).toEqual(['zooms_faint_protein']);
  });
  it('never puts details-only notes under the verdict', () => {
    const p = pickNotes([n('second_opinion_differs'), n('serial_class_mismatch')]);
    expect(p.headline).toEqual([]);
    expect(p.details).toHaveLength(2);
  });
  it("uses the core's choice when it is given (desktop app), in its order", () => {
    const notes = [n('bands_too_noisy'), n('lift_good'), n('contaminants_not_checked')];
    const p = pickNotes(notes, ['lift_good', 'contaminants_not_checked']);
    expect(p.headline.map((x) => x.key)).toEqual(['lift_good', 'contaminants_not_checked']);
    expect(p.details.map((x) => x.key)).toEqual(['bands_too_noisy']);
    expect(pickNotes(notes, []).headline).toEqual([]);
  });
});

describe('the panel notes (2026-10-18): supporting slot, after a thin coating and before the noise and models notes', () => {
  it('C–H above N–H outranks the strong disagreement, which outranks contaminants not checked and models disagree', () => {
    expect(pickNotes([n('models_disagree'), n('contaminants_not_checked'), n('readers_disagree_strongly'), n('ch_above_nh_possible')]).headline.map((x) => x.key)).toEqual(['ch_above_nh_possible']);
    expect(pickNotes([n('models_disagree'), n('readers_disagree_strongly')]).headline.map((x) => x.key)).toEqual(['readers_disagree_strongly']);
    expect(pickNotes([n('ch_above_nh_strong'), n('possible_thin_coating')]).headline.map((x) => x.key)).toEqual(['possible_thin_coating']);
    expect(pickNotes([n('positive_signs_all_six'), n('ch_above_nh_strong'), n('lift_good')]).headline.map((x) => x.key)).toEqual(['lift_good', 'ch_above_nh_strong']);
  });
  it('icons: the C–H note is a contaminant-type note, the disagreement note a models note', () => {
    expect(noteIcon('ch_above_nh_possible')).toBe('flask');
    expect(noteIcon('ch_above_nh_strong')).toBe('flask');
    expect(noteIcon('readers_disagree_strongly')).toBe('layers');
  });
});

describe('the scan list tag: strong panel notes only (Matt, 2026-10-18)', () => {
  const k = (key: NoteKey) => ({ key });
  it('the C–H "likely" level first, then readers disagree strongly; nothing for the weaker levels', () => {
    expect(strongPanelNote([k('readers_disagree_strongly'), k('ch_above_nh_strong')])?.key).toBe('ch_above_nh_strong');
    expect(strongPanelNote([k('readers_disagree_strongly')])?.key).toBe('readers_disagree_strongly');
    expect(strongPanelNote([k('ch_above_nh_possible'), k('models_disagree')])).toBeUndefined();
  });
});
