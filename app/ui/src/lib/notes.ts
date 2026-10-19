// Note precedence (PLAN v3.2 Step 9; DECISIONS 53, 80 amended): at most TWO notes under the verdict.
// First the note of the rule that set the verdict: +D > flat bands > lift > blocked lift.
// Then the ZooMS line, when the ZooMS band check calls the scan better than the verdict.
// Then one supporting note: above bone range > possible thin coating > C–H above N–H > readers disagree strongly >
// contaminants not checked > models disagree > positive signs > bands too noisy. Last, the faint protein sign for ZooMS, only if a slot is
// still free. Everything else goes in the details. Flags (contaminant / heat signs) are not notes: they
// have their own prominent chip and never count toward the two. Nor do the strong warnings (C–H "likely", readers disagree
// strongly; DECISIONS 130, 134): a fired one is always on the card in its own place, never takes the supporting slot, and
// the quieter note keeps that slot.

import type { Note, NoteKey } from './types';

export const RULE_NOTES: NoteKey[] = ['plus_d', 'flat_bands', 'lift_good', 'lift_borderline', 'lift_blocked'];
/** The ZooMS line (one of the three; DECISIONS 80 amended). It never changes the verdict. */
export const ZOOMS_NOTES: NoteKey[] = [
  'zooms_better_good',
  'zooms_better_protein',
  'zooms_better_1545',
  'zooms_better_1545_flat',
  'zooms_better_ryder',
];
/** The quietest note (weak evidence): under the verdict only when a slot is left. */
export const LAST_NOTES: NoteKey[] = ['zooms_faint_protein'];
export const SUPPORTING_NOTES: NoteKey[] = [
  'above_bone_range',
  'possible_thin_coating',
  'ch_above_nh_strong',
  'ch_above_nh_possible',
  'readers_disagree_strongly',
  'contaminants_not_checked',
  // models_disagree is details-only (DECISIONS 136): it never takes a slot under the verdict
  'positive_signs_all_six',
  'positive_signs_clear',
  'bands_too_noisy',
];

export interface PickedNotes {
  /** Up to two, shown under the verdict. Never a strong warning. */
  headline: Note[];
  /** Every strong warning that fired (C–H "likely" first, then readers disagree strongly): always on the verdict card. */
  warnings: Note[];
  /** The rest, shown in the details column (never a note the card shows). */
  details: Note[];
}

/** Picks the (at most two) headline notes and the strong warnings. With `shown` (the core's choice, desktop app) those keys
 *  are used, in that order; without it (browser mock) the same precedence is applied here. The warnings come from `notes`
 *  either way, so a fired warning reaches the card whether or not the core listed it in `shown` (older records, and cores
 *  before DECISIONS 134, let a quieter note take its place). */
export function pickNotes(notes: Note[], shown?: NoteKey[]): PickedNotes {
  const warnings = strongWarnings(notes);
  const rest = (headline: Note[]) => notes.filter((n) => !headline.includes(n) && !warnings.includes(n));
  if (shown) {
    const headline = shown
      .filter((k) => !isStrongPanel(k))
      .map((k) => notes.find((n) => n.key === k))
      .filter((n): n is Note => n !== undefined);
    return { headline, warnings, details: rest(headline) };
  }
  const first = (order: NoteKey[]) => {
    for (const k of order) {
      const n = notes.find((x) => x.key === k);
      if (n) return n;
    }
    return null;
  };
  // the supporting slot skips the warnings (they have their own place), as the core does
  const headline = [first(RULE_NOTES), first(ZOOMS_NOTES), first(SUPPORTING_NOTES.filter((k) => !isStrongPanel(k)))]
    .filter((n): n is Note => n !== null)
    .slice(0, 2);
  // the faint sign only when a slot is free and no ZooMS line shows (a Ryder line puts it in the details)
  const last = first(LAST_NOTES);
  if (last && headline.length < 2 && !headline.some((n) => ZOOMS_NOTES.includes(n.key))) headline.push(last);
  return { headline, warnings, details: rest(headline) };
}

export type NoteIcon = 'up' | 'down' | 'info' | 'flask' | 'disagree' | 'noise' | 'check' | 'zooms';

export function noteIcon(k: NoteKey): NoteIcon {
  switch (k) {
    case 'lift_good':
    case 'lift_borderline':
      return 'up';
    case 'plus_d':
      return 'down';
    case 'lift_blocked':
    case 'possible_thin_coating':
    case 'above_bone_range':
    case 'ch_above_nh_possible':
    case 'ch_above_nh_strong':
      return 'flask';
    case 'models_disagree':
    case 'readers_disagree_strongly':
      return 'disagree';
    case 'bands_too_noisy':
    case 'contaminants_not_checked':
    case 'c1_reduced':
    case 'signs_reduced':
    case 'no_model_reading':
      return 'noise';
    case 'positive_signs_all_six':
    case 'positive_signs_clear':
      return 'check';
    case 'zooms_better_good':
    case 'zooms_better_protein':
    case 'zooms_better_1545':
    case 'zooms_better_1545_flat':
    case 'zooms_better_ryder':
    case 'zooms_faint_protein':
      return 'zooms';
    default:
      return 'info';
  }
}

export function isPositive(k: NoteKey): boolean {
  return k === 'positive_signs_all_six' || k === 'positive_signs_clear';
}

/** The ZooMS line (DECISIONS 80 amended): the ZooMS band check calls this scan better than the verdict. */
export function isZoomsLine(k: NoteKey): boolean {
  return ZOOMS_NOTES.includes(k);
}

/** The faint protein sign for ZooMS: muted, never a list mark. */
export function isFaint(k: NoteKey): boolean {
  return LAST_NOTES.includes(k);
}

/** A scan's ZooMS line (the stronger lines only; the faint sign is not one), if it has one. */
export function zoomsLine<T extends { key: NoteKey }>(notes: T[]): T | undefined {
  return notes.find((n) => isZoomsLine(n.key));
}

/** The two strong panel notes (C–H "likely", readers disagree strongly): warnings, in their own warning colour, on the verdict
 *  card and in the scans list (Matt, 2026-10-18: "usually a time when there is a real problem"; reverses the quiet treatment
 *  of DECISIONS 121). The milder levels stay quiet. */
export const STRONG_PANEL_NOTES: NoteKey[] = ['ch_above_nh_strong', 'readers_disagree_strongly'];
export const isStrongPanel = (k: NoteKey): boolean => STRONG_PANEL_NOTES.includes(k);

/** The strong panel note for the scan list's mark (Matt, 2026-10-18: strong ones only): the C–H note's "likely" level
 *  first, else readers_disagree_strongly; undefined when neither fired. */
export function strongPanelNote<T extends { key: NoteKey }>(notes: T[]): T | undefined {
  return notes.find((n) => n.key === 'ch_above_nh_strong') ?? notes.find((n) => n.key === 'readers_disagree_strongly');
}

/** Every strong warning that fired, in STRONG_PANEL_NOTES order (C–H "likely", then readers disagree strongly), each once.
 *  Both can fire together; the card shows both (never one dropped silently). */
export function strongWarnings<T extends { key: NoteKey }>(notes: T[]): T[] {
  return STRONG_PANEL_NOTES.map((k) => notes.find((n) => n.key === k)).filter((n): n is T => n !== undefined);
}
