// The UI string table for notes and flags. The core decides WHICH notes apply (keys + params); the
// wording lives here only, so it can change without touching a rule (01_design §9). A Vitest lint
// (strings.test.ts) keeps forbidden words out ("wet", "fail", "reject", exclamation marks).

import { formatModel, formatReading } from './display';
import type { Note, NoteKey, SignId } from './types';

type P = Record<string, number | string | string[]>;
const r = (p: P, k: string) => formatReading(typeof p[k] === 'number' ? (p[k] as number) : null);
// a single model's own output (the components, the second opinion): one decimal, no cap
const rm = (p: P, k: string) => formatModel(typeof p[k] === 'number' ? (p[k] as number) : null);

/** The Ryder 2045 reading in the ZooMS line: one decimal, or two where one decimal would fall below the cut (0.35 shows
 *  "0.35", not "0.3"), so the number never contradicts "above its ZooMS line". */
export function ryderPct(p: P): string {
  const r = typeof p.r === 'number' ? p.r : NaN;
  const cut = typeof p.at_least === 'number' ? p.at_least : 0.34;
  if (!Number.isFinite(r)) return '–';
  const one = r.toFixed(1);
  return Number(one) < cut ? r.toFixed(2) : one;
}

const SIGN_NOUN: Record<SignId, string> = {
  wax: 'wax',
  ester: 'consolidant',
  plaster: 'plaster',
  c1: 'foreign-organic',
  burnt: 'heat',
};

/** Names of the contaminant checks, as a user reads them (for "checked" / "not checked" lists). */
const SIGN_CHECK_NAME: Record<string, string> = {
  wax: 'wax',
  ester: 'consolidant (ester)',
  plaster: 'plaster',
  c1: 'other foreign organic',
  C1: 'other foreign organic',
};

/** "a, b and c" from a core list param (an array, or a comma-separated string). */
function signList(v: unknown): string {
  const ids = Array.isArray(v) ? v.map(String) : typeof v === 'string' && v ? v.split(',').map((s) => s.trim()) : [];
  const names = ids.filter(Boolean).map((id) => SIGN_CHECK_NAME[id] ?? id);
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

export function signNoun(id: SignId): string {
  return SIGN_NOUN[id];
}

export const NOTE_TEXT: Record<NoteKey, (p: P) => string> = {
  plus_d: (p) =>
    `Lowered to Borderline: the collagen models read ${r(p, 'm')}, but the protein bands at 2044 and 2175 nm are flat. Rescan this spot or check for a coating.`,
  flat_bands: (p) =>
    typeof p.m === 'number' && p.m >= 0.5
      ? `No protein signal at this spot. The collagen models read ${r(p, 'm')}, but the protein bands are flat.`
      : 'No protein signal at this spot: the protein bands are flat.',
  lift_good: (p) =>
    `Raised to Good: all six collagen bands are clearly resolved and no contaminant sign fired, although the collagen models read ${r(p, 'm')}. The models can under-read unusual bone.`,
  lift_borderline: (p) =>
    `Raised to Borderline: the collagen bands are resolved and no contaminant sign fired, although the collagen models read ${r(p, 'm')}.`,
  lift_blocked: (p) =>
    p.reason === 'noise'
      ? `The collagen bands look ${p.strength}, but this scan is too noisy to check for contaminants, so the bands cannot raise the verdict.`
      : `The collagen bands look ${p.strength}, but a ${SIGN_NOUN[p.sign as SignId] ?? 'contaminant'} sign fired. A coating or contaminant can produce these bands, so the bands cannot raise the verdict.`,
  above_bone_range: () =>
    'Reads higher than archaeological bone normally does (as high as fresh modern bone). Modern bone, glue or another protein may be present.',
  possible_thin_coating: () =>
    'Possible thin coating: a weak sign of glue or consolidant at this spot. It may be nothing, but check the spot before sampling.',
  contaminants_not_checked: (p) => {
    const skipped = signList(p.skipped);
    const checked = signList(p.checked);
    if (!skipped) return 'Contaminant signs could not be checked: this scan is too noisy. A rescan with more averages (100–200) would allow it.';
    return `Not checked on this scan: ${skipped}. The scan is too noisy in that part of the spectrum; a rescan with 100–200 averages usually fixes this.${checked ? ` Checked: ${checked}.` : ''}`;
  },
  c1_reduced: () =>
    'The general organic check ran without the region above 2300 nm, which is too noisy on this scan. In this reduced form it can miss some glues and lacquers (for example cellulose-nitrate glues and animal glue).',
  models_disagree: (p) =>
    `The three collagen models fall on different sides of a verdict line (${rm(p, 'a')}, ${rm(p, 'b')}, ${rm(p, 'c')}); the verdict uses the middle value. Large disagreement is common on coated or treated bone, or bone with an altered OH/water band.`,
  positive_signs_all_six: () =>
    'Positive signs: all six collagen bands are resolved and no contaminant sign fired.',
  positive_signs_clear: () =>
    'Positive signs: the collagen bands are clearly resolved and no contaminant sign fired.',
  zooms_better_good: () => 'Better chance with ZooMS: all six collagen bands are resolved at this spot.',
  zooms_better_protein: () => 'Some chance with ZooMS: protein bands show at this spot.',
  zooms_better_1545: () => 'Some chance with ZooMS: the 1545 nm collagen band shows at this spot.',
  zooms_better_1545_flat: () =>
    'Some chance with ZooMS: the 1545 nm collagen band shows, although the other protein bands are flat.',
  zooms_better_ryder: (p) =>
    `Some chance with ZooMS: the published Ryder 2045 model reads ${ryderPct(p)}%, above its ZooMS line (${p.at_least ?? 0.34}%).`,
  zooms_faint_protein: (p) => {
    const lit = Array.isArray(p.lit) ? p.lit.map(String) : [];
    if (lit.length === 1 && lit[0] === 'amide2175')
      return 'Faint sign for ZooMS: only the 2175 nm protein band shows here, which is weak evidence on its own.';
    if (lit.length <= 1) return 'Faint sign for ZooMS: only one protein band shows here, which is weak evidence on its own.';
    return 'Faint sign for ZooMS: protein bands show here, but the collagen models read very low, so this is weak evidence.';
  },
  bands_too_noisy: () =>
    'The bands are too noisy to judge at this spot; the verdict follows the collagen models. A rescan with 100–200 averages usually helps.',
  second_opinion_differs: (p) =>
    `The 1545 nm model with no transfer needed reads ${rm(p, 's')}, which differs from the main reading; the high-res transfer is provisional.`,
  no_transfer_available: () =>
    'No high-res transfer is available for this instrument, so the models read the scan as measured. Readings may run high.',
  no_model_reading: () =>
    'No collagen model could read this scan (a model window is unreadable), so the verdict cannot use a reading.',
  serial_class_mismatch: (p) =>
    `This file's serial (${p.serial}) is a known ${p.known === 'hires' ? 'high-res' : 'standard'} instrument, but the switch says ${p.set === 'hires' ? 'High-res' : 'Standard'}. The switch is yours; flip it if that was a slip.`,
  header_class_mismatch: (p) =>
    `This file's detector settings look like a ${p.known === 'hires' ? 'high-res' : 'standard'} instrument, but the switch says ${p.set === 'hires' ? 'High-res' : 'Standard'}. This is only a hint; the switch is yours.`,
};

export function noteText(n: Note): string {
  return NOTE_TEXT[n.key](n.params);
}

/** Flag texts for fired signs (DECISIONS 53). Calm and informative; they never change the verdict. */
export function flagText(id: SignId, heatKind?: 'charred' | 'calcined'): string {
  if (id === 'burnt') {
    const what = heatKind === 'calcined' ? ' (calcined: sharp OH peaks at 979 and 1433 nm)' : heatKind === 'charred' ? ' (charred: a dark visible edge)' : '';
    return `Heat sign at this spot${what}. Heating lowers collagen; the readings already reflect that.`;
  }
  const noun = SIGN_NOUN[id];
  return `${noun[0].toUpperCase()}${noun.slice(1)} sign at this spot. Many collagen models still read well on treated bone, but check the spot and clean it if you can.`;
}

/** Short badge label for a fired sign. */
export function flagLabel(id: SignId): string {
  return { wax: 'Wax sign', ester: 'Consolidant sign', plaster: 'Plaster sign', c1: 'Foreign-organic sign', burnt: 'Heat sign' }[id];
}

export const UNUSABLE_TEXT: Record<string, string> = {
  low_signal: 'Low signal across the whole range. The probe may have lifted. Rescan this spot.',
  saturated: 'The detector saturated. Rescan this spot.',
  panel: 'This looks like the white panel or an empty probe. Rescan the bone.',
  unsupported: 'This file type or format is not supported.',
  unreadable: 'The file could not be read completely. Rescan, or save it again.',
};

export const NOT_BONE_TEXT = 'Check the target: the probe may be on plaster, a label, a coating or the holder.';

/** Every user-facing string, for the lint test. */
export function allStrings(): string[] {
  const p = { m: 1.4, a: 0.4, b: 0.9, c: 4.1, s: 1.2, r: 0.44, at_least: 0.34, strength: 'strong', sign: 'ester', reason: 'sign', serial: 28313, known: 'hires', set: 'standard' };
  const ids: SignId[] = ['wax', 'ester', 'plaster', 'c1', 'burnt'];
  return [
    ...Object.values(NOTE_TEXT).map((f) => f(p)),
    NOTE_TEXT.lift_blocked({ ...p, reason: 'noise' }),
    ...ids.map((id) => flagText(id)),
    flagText('burnt', 'charred'),
    flagText('burnt', 'calcined'),
    ...ids.map(flagLabel),
    ...Object.values(UNUSABLE_TEXT),
    NOT_BONE_TEXT,
  ];
}
