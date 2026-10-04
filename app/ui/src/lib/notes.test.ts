import { describe, expect, it } from 'vitest';
import { pickNotes } from './notes';
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
