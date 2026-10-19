import { describe, expect, it } from 'vitest';
import { isStrongPanel, noteIcon, pickNotes, strongPanelNote, strongWarnings } from './notes';
import type { Note, NoteKey } from './types';

const n = (key: NoteKey): Note => ({ key, params: {} });

describe('note precedence: at most two under the verdict', () => {
  it('puts the rule note first, then one supporting note, the rest in details', () => {
    const p = pickNotes([n('bands_too_noisy'), n('positive_signs_all_six'), n('lift_good'), n('contaminants_not_checked'), n('second_opinion_differs')]);
    expect(p.headline.map((x) => x.key)).toEqual(['lift_good', 'contaminants_not_checked']);
    expect(p.details.map((x) => x.key)).toEqual(['bands_too_noisy', 'positive_signs_all_six', 'second_opinion_differs']);
  });
  it('orders rule notes +D > flat bands > lift > blocked lift', () => {
    expect(pickNotes([n('lift_blocked'), n('flat_bands'), n('plus_d')]).headline[0].key).toBe('plus_d');
    expect(pickNotes([n('lift_blocked'), n('lift_borderline')]).headline[0].key).toBe('lift_borderline');
  });
  it('orders supporting notes: contaminants not checked > positive signs > bands too noisy', () => {
    const p = pickNotes([n('bands_too_noisy'), n('positive_signs_clear'), n('contaminants_not_checked'), n('models_disagree')]);
    expect(p.headline.map((x) => x.key)).toEqual(['contaminants_not_checked']);
  });
  it('models disagree is details-only: never under the verdict (DECISIONS 136)', () => {
    const p = pickNotes([n('models_disagree')]);
    expect(p.headline).toEqual([]);
    expect(p.details.map((x) => x.key)).toEqual(['models_disagree']);
    expect(pickNotes([n('models_disagree'), n('bands_too_noisy')]).headline.map((x) => x.key)).toEqual(['bands_too_noisy']);
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
    expect(pickNotes([n('zooms_faint_protein'), n('contaminants_not_checked')]).headline.map((x) => x.key)).toEqual([
      'contaminants_not_checked',
      'zooms_faint_protein',
    ]);
    const p = pickNotes([n('zooms_faint_protein'), n('contaminants_not_checked'), n('flat_bands')]);
    expect(p.headline.map((x) => x.key)).toEqual(['flat_bands', 'contaminants_not_checked']);
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
  it('the C–H "possible" note outranks contaminants not checked and models disagree', () => {
    expect(pickNotes([n('models_disagree'), n('contaminants_not_checked'), n('readers_disagree_strongly'), n('ch_above_nh_possible')]).headline.map((x) => x.key)).toEqual(['ch_above_nh_possible']);
  });
  it('a strong warning never takes the supporting slot: the quieter note keeps it, the warning has its own place (DECISIONS 134)', () => {
    const p = pickNotes([n('contaminants_not_checked'), n('readers_disagree_strongly')]);
    expect(p.headline.map((x) => x.key)).toEqual(['contaminants_not_checked']);
    expect(p.warnings.map((x) => x.key)).toEqual(['readers_disagree_strongly']);
    const q = pickNotes([n('ch_above_nh_strong'), n('possible_thin_coating')]);
    expect(q.headline.map((x) => x.key)).toEqual(['possible_thin_coating']);
    expect(q.warnings.map((x) => x.key)).toEqual(['ch_above_nh_strong']);
    const r = pickNotes([n('positive_signs_all_six'), n('ch_above_nh_strong'), n('lift_good')]);
    expect(r.headline.map((x) => x.key)).toEqual(['lift_good', 'positive_signs_all_six']);
    expect(r.warnings.map((x) => x.key)).toEqual(['ch_above_nh_strong']);
  });
  it('the crowded case: the rule note and the ZooMS line fill both slots, and both warnings still show; nothing repeats in the details', () => {
    const notes = [n('lift_borderline'), n('zooms_better_good'), n('possible_thin_coating'), n('readers_disagree_strongly'), n('ch_above_nh_strong')];
    const p = pickNotes(notes);
    expect(p.headline.map((x) => x.key)).toEqual(['lift_borderline', 'zooms_better_good']);
    expect(p.warnings.map((x) => x.key)).toEqual(['ch_above_nh_strong', 'readers_disagree_strongly']);
    expect(p.details.map((x) => x.key)).toEqual(['possible_thin_coating']);
  });
  it("with the core's choice, a fired warning is on the card whether or not the core listed it, and only once", () => {
    const notes = [n('possible_thin_coating'), n('ch_above_nh_possible'), n('readers_disagree_strongly')];
    // an older core: the quieter note held the slot and the warning was left out
    const old = pickNotes(notes, ['possible_thin_coating']);
    expect(old.headline.map((x) => x.key)).toEqual(['possible_thin_coating']);
    expect(old.warnings.map((x) => x.key)).toEqual(['readers_disagree_strongly']);
    expect(old.details.map((x) => x.key)).toEqual(['ch_above_nh_possible']);
    // the core after DECISIONS 134 lists the warning after the other notes: same card
    expect(pickNotes(notes, ['possible_thin_coating', 'readers_disagree_strongly'])).toEqual(old);
    // an older core that put the warning in the supporting slot: the warning still shows once, in its own place
    const slot = pickNotes([n('readers_disagree_strongly'), n('contaminants_not_checked')], ['readers_disagree_strongly']);
    expect(slot.headline).toEqual([]);
    expect(slot.warnings.map((x) => x.key)).toEqual(['readers_disagree_strongly']);
  });
  it('icons: the C–H note is a contaminant-type note, the disagreement note a models note', () => {
    expect(noteIcon('ch_above_nh_possible')).toBe('flask');
    expect(noteIcon('ch_above_nh_strong')).toBe('flask');
    expect(noteIcon('readers_disagree_strongly')).toBe('disagree');
    expect(noteIcon('models_disagree')).toBe('disagree');
  });
});

describe('the scan list tag: strong panel notes only (Matt, 2026-10-18)', () => {
  const k = (key: NoteKey) => ({ key });
  it('the C–H "likely" level first, then readers disagree strongly; nothing for the weaker levels', () => {
    expect(strongPanelNote([k('readers_disagree_strongly'), k('ch_above_nh_strong')])?.key).toBe('ch_above_nh_strong');
    expect(strongPanelNote([k('readers_disagree_strongly')])?.key).toBe('readers_disagree_strongly');
    expect(strongPanelNote([k('ch_above_nh_possible'), k('models_disagree')])).toBeUndefined();
  });
  it('strongWarnings: every fired warning, C–H "likely" first, each once; none for the milder levels', () => {
    expect(strongWarnings([k('readers_disagree_strongly'), k('models_disagree'), k('ch_above_nh_strong')]).map((x) => x.key)).toEqual([
      'ch_above_nh_strong',
      'readers_disagree_strongly',
    ]);
    expect(strongWarnings([k('ch_above_nh_possible'), k('models_disagree')])).toEqual([]);
    expect(isStrongPanel('ch_above_nh_possible')).toBe(false);
  });
});

describe('the strong panel notes are warnings (Matt, 2026-10-18)', () => {
  it('only the C–H "likely" level and readers disagree strongly; the milder levels stay quiet', () => {
    expect(isStrongPanel('ch_above_nh_strong')).toBe(true);
    expect(isStrongPanel('readers_disagree_strongly')).toBe(true);
    expect(isStrongPanel('ch_above_nh_possible')).toBe(false);
    expect(isStrongPanel('models_disagree')).toBe(false);
  });
});
