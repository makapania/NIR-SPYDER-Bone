// The UI string table for notes and flags. The core decides WHICH notes apply (keys + params); the
// wording lives here only, so it can change without touching a rule (01_design §9). A Vitest lint
// (strings.test.ts) keeps forbidden words out ("wet", "fail", "reject", exclamation marks).

import { formatModel, formatReading } from './display';
import { NOT_BUILT_IN, instrumentName } from './instrument';
import type { Note, NoteKey, ScanResult, SignId } from './types';

type P = Record<string, number | string | string[]>;

/** Band ids (bands.ts) to their wavelengths, for notes that name bands; the seven readable bands in all. */
const BAND_NM: Record<string, number> = { ch1689: 1689, ch1728: 1728, nh2044: 2044, amide2175: 2175, ch2262: 2262, ch2284: 2284, nh1545: 1545 };
const ALL_BANDS = 7;
const listNm = (v: number[]) => {
  const s = [...v].sort((a, b) => a - b).map(String);
  return s.length <= 1 ? s.join('') : `${s.slice(0, -1).join(', ')} and ${s[s.length - 1]}`;
};
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
  wax: 'lipid',
  ester: 'consolidant',
  plaster: 'plaster',
  c1: 'foreign-organic',
  burnt: 'heat',
};

/** Names of the contaminant checks, as a user reads them (for "checked" / "not checked" lists). */
const SIGN_CHECK_NAME: Record<string, string> = {
  wax: 'lipid (such as wax)',
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
  ch_above_nh_possible: () =>
    "Possible extra organic material: the C–H band reads higher than the protein (N–H) bands explain (the bone's own fat, wax or consolidant, for example).",
  ch_above_nh_strong: () =>
    "Likely extra organic material: the C–H band reads far higher than the protein (N–H) bands explain (the bone's own fat, wax or consolidant, for example).",
  readers_disagree_strongly: () =>
    'Collagen estimates disagree strongly. Treat the percentage with caution; the cause is unknown.',
  contaminants_not_checked: (p) => {
    const skipped = signList(p.skipped);
    const checked = signList(p.checked);
    if (!skipped) return 'Contaminant signs could not be checked: this scan is too noisy. A rescan with more averages (100–200) would allow it.';
    return `Not checked on this scan: ${skipped}. The scan is too noisy in that part of the spectrum; a rescan with 100–200 averages usually fixes this.${checked ? ` Checked: ${checked}.` : ''}`;
  },
  c1_reduced: () =>
    'The general organic check ran without the region above 2300 nm, which is too noisy on this scan. In this reduced form it can miss some glues and lacquers (for example cellulose-nitrate glues and animal glue).',
  signs_reduced: (p) => {
    const ids = Array.isArray(p.signs) ? p.signs.map(String) : [];
    const plaster = ids.includes('plaster');
    const c1 = ids.includes('C1') || ids.includes('c1');
    const why = 'Above 1800 nm this scan is too noisy for the full checks; more averages (100–200) may let them run.';
    const pl = 'In tests the reduced plaster check found most added plaster.';
    const org = 'The reduced organic check can catch heavy consolidants but misses thin coats, glue and many consolidants.';
    if (plaster && c1) return `The plaster and general organic checks ran in a reduced form, below 1800 nm only. ${why} ${pl} ${org}`;
    if (c1) return `The general organic check ran in a reduced form, below 1800 nm only. ${why} ${org}`;
    return `The plaster check ran on its 1446 nm band only. ${why} ${pl}`;
  },
  models_disagree: (p) =>
    `The three collagen models fall on different sides of a verdict line (${rm(p, 'a')}, ${rm(p, 'b')}, ${rm(p, 'c')}); the verdict uses the middle value.`,
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
    `The 1545 nm model, which reads the scan as measured (no correction), reads ${rm(p, 's')}, more than usual away from the main reading.`,
  instrument_uncalibrated: (p) =>
    `Uncalibrated instrument: no correction for ${typeof p.name === 'string' ? p.name : 'this instrument'} is installed, so the models read the scan as measured and the readings may run high or low. The verdict follows the readings alone: band evidence (flat bands, lifts, the ZooMS band check) needs the instrument built in, and the contaminant and heat signs are provisional. ${NOT_BUILT_IN}`,
  visible_levels_not_fitted: () =>
    'The visible levels are not calibrated for this correction, so the heat sign reads the scan as measured.',
  no_model_reading: () =>
    'No collagen model could read this scan (a model window is unreadable), so the verdict cannot use a reading.',
  band_noise_unknown: (p) => {
    const ids = Array.isArray(p.bands) ? p.bands.map(String) : [];
    const nm = ids.map((id) => BAND_NM[id]).filter((x): x is number => x !== undefined);
    return nm.length && nm.length < ALL_BANDS
      ? `Band noise could not be estimated at ${listNm(nm)} nm on this instrument, so a faint band there is not read; only a strong band counts.`
      : 'Band noise could not be estimated for this correction, so faint bands are not read; only strong bands count.';
  },
  serial_class_mismatch: (p) =>
    `This file's serial (${p.serial}) is a known ${instrumentName(String(p.known)).toLowerCase()} instrument, but the switch says ${instrumentName(String(p.set))}. The switch is yours; flip it if that was a slip.`,
  header_class_mismatch: (p) =>
    `This file's detector settings look like a ${instrumentName(String(p.known)).toLowerCase()} instrument, but the switch says ${instrumentName(String(p.set))}. This is only a hint; the switch is yours.`,
};

/** The notes that report a contaminant sign (the soft tier): provisional, as the flags are, on an instrument with no
 *  built-in correction (DECISIONS 137; the core marks them provisional there too). */
export const PROVISIONAL_NOTES: readonly NoteKey[] = ['possible_thin_coating'];

/** Whether this note is shown provisional on this scan (an instrument not built in). */
export function isProvisionalNote(key: NoteKey, scan?: Pick<ScanResult, 'uncalibrated'>): boolean {
  return !!scan?.uncalibrated && PROVISIONAL_NOTES.includes(key);
}

/** A note's text. With the scan, "Collagen estimates disagree strongly" names what actually disagreed (disagreeParts),
 *  and a contaminant note on an instrument not built in opens with the provisional qualification. */
export function noteText(n: Note, scan?: ScanResult): string {
  const d = scan && n.key === 'readers_disagree_strongly' ? disagreeParts(scan) : null;
  const t = d ? d.lead + d.rest : NOTE_TEXT[n.key](n.params);
  return isProvisionalNote(n.key, scan) ? `${PROVISIONAL}${t}` : t;
}

/** The readers-disagree-strongly note in words that name the comparison that fired, with both readings (Matt, 2026-10-18:
 *  "it says there are large model differences but none can be seen; if this is actually a different flag it should say that").
 *  The rule is unchanged; this picks the comparison furthest past its line in the scan's panel record. Null without one. */
export function disagreeParts(scan: ScanResult): { lead: string; rest: string } | null {
  const p = scan.panel;
  const m = scan.models.cons3?.value;
  if (!p || m == null || !Number.isFinite(m)) return null;
  const fired = p.contrasts.filter((c) => c.fraction > 1 && Number.isFinite(c.value));
  if (!fired.length) return null;
  const c = fired.reduce((a, b) => (b.fraction > a.fraction ? b : a));
  const f = formatModel;
  const mm = scan.models;
  const tail = ', more than on clean bone. Treat the percentage with caution; the cause is unknown.';
  const w1500 = mm.wc1500?.value;
  // the heading stays "Collagen estimates disagree strongly" (Matt: strong disagreement must read as such); the body names the
  // pair and its readings
  const lead = 'Collagen estimates disagree strongly';
  switch (c.name) {
    case 'SP3':
      return { lead, rest: `: the three consensus models read 2045 nm ${f(mm.wc2045?.value)}, 1545 nm ${f(w1500)} and N–H set ${f(mm.f05?.value)}${tail}` };
    case 'RAW':
      return { lead, rest: `: Ryder 2045 reads ${f(mm.ryder2045?.value)} against the consensus ${f(m)}${tail}` };
    case 'GL':
      return { lead, rest: `: the narrow 1545 nm band reads ${f(m - c.value)} against the consensus ${f(m)}${tail}` };
    case 'SHP':
      return { lead, rest: `: the 1545 nm model reads ${f(w1500)} against ${f(w1500 == null ? null : w1500 - c.value)} for the narrow 1545 nm band${tail}` };
    case 'CHN':
      return { lead, rest: `: C–H 1728 nm reads ${f(mm.ch1728?.value)}, below the consensus ${f(m)}${tail}` };
    case 'CH':
      return { lead, rest: `: C–H 1728 nm reads ${f(mm.ch1728?.value)}, above the consensus ${f(m)}${tail}` };
  }
  return null;
}

/** Opens a flag's (or a contaminant note's) text on an instrument with no built-in correction (DECISIONS 137). */
export const PROVISIONAL = 'Provisional on this instrument: it is not built in, so these checks are not calibrated for it. ';

/** Flag texts for fired signs (DECISIONS 53). Calm and informative; they never change the verdict. */
export function flagText(id: SignId, heatKind?: 'charred' | 'calcined'): string {
  if (id === 'burnt') {
    const what = heatKind === 'calcined' ? ' (calcined: sharp OH peaks at 979 and 1433 nm)' : heatKind === 'charred' ? ' (charred: a dark visible edge)' : '';
    return `Heat sign at this spot${what}. Heating lowers collagen; the readings already reflect that.`;
  }
  // the wax sign (id 'wax') reads long-chain C–H: lipids from any source, wax being one example (it fires on some recent bone's own fat)
  if (id === 'wax')
    return 'Lipid sign at this spot (wax, for example, or fat), from any source. Many collagen models still read well, but check the spot.';
  const noun = SIGN_NOUN[id];
  return `${noun[0].toUpperCase()}${noun.slice(1)} sign at this spot. Many collagen models still read well on treated bone, but check the spot and clean it if you can.`;
}

/** Short badge label for a fired sign. */
export function flagLabel(id: SignId): string {
  return { wax: 'Lipid sign', ester: 'Consolidant sign', plaster: 'Plaster sign', c1: 'Foreign-organic sign', burnt: 'Heat sign' }[id];
}

/** A fired sign's label and text as every view shows them (card, details, scan list, readable export): marked
 *  provisional on an instrument with no built-in correction (DECISIONS 137). */
export function shownFlag(
  scan: Pick<ScanResult, 'uncalibrated'>,
  id: SignId,
  heatKind?: 'charred' | 'calcined',
): { label: string; text: string } {
  const p = !!scan.uncalibrated;
  return { label: `${flagLabel(id)}${p ? ' (provisional)' : ''}`, text: `${p ? PROVISIONAL : ''}${flagText(id, heatKind)}` };
}

export const UNUSABLE_TEXT: Record<string, string> = {
  low_signal: 'Low signal across the whole range. The probe may have lifted. Rescan this spot.',
  saturated: 'The detector saturated. Rescan this spot.',
  panel: 'This looks like the white panel or an empty probe. Rescan the bone.',
  unsupported: 'This file type or format is not supported.',
  unreadable: 'The file could not be read completely. Rescan, or save it again.',
};

/** The low-range estimate's standing explanation (the verdict card's tooltip and the details row). */
export const LOWRANGE_EXPLAIN = 'Tuned for low collagen; often reads lower at 3–6%.';

/** The caution notes whose opening words the verdict card sets in bold (DECISIONS 123). Each note's text starts with its lead. */
export const NOTE_LEAD: Partial<Record<NoteKey, string>> = {
  ch_above_nh_possible: 'Possible extra organic material',
  ch_above_nh_strong: 'Likely extra organic material',
  readers_disagree_strongly: 'Collagen estimates disagree strongly',
};

/** Short chip labels for the compact verdict card (design proposal, 2026-10-18; Matt rules on wording). The full text is
 *  one interaction away (tooltip on hover and keyboard focus, click or tap to expand); a lead that is already short is kept. */
export const NOTE_SHORT: Record<NoteKey, string> = {
  plus_d: 'Lowered to Borderline',
  flat_bands: 'Protein bands flat',
  lift_good: 'Raised to Good',
  lift_borderline: 'Raised to Borderline',
  lift_blocked: 'Bands cannot raise the verdict',
  zooms_better_good: 'Better chance with ZooMS',
  zooms_better_protein: 'Some chance with ZooMS',
  zooms_better_1545: 'Some chance with ZooMS',
  zooms_better_1545_flat: 'Some chance with ZooMS',
  zooms_better_ryder: 'Some chance with ZooMS',
  zooms_faint_protein: 'Faint sign for ZooMS',
  above_bone_range: 'Above bone range',
  possible_thin_coating: 'Possible thin coating',
  ch_above_nh_possible: 'Possible extra organic material',
  ch_above_nh_strong: 'Likely extra organic material',
  readers_disagree_strongly: 'Collagen estimates disagree strongly',
  contaminants_not_checked: 'Contaminants not checked',
  c1_reduced: 'Organic check reduced',
  signs_reduced: 'Checks reduced',
  models_disagree: 'Models disagree',
  positive_signs_all_six: 'Positive signs',
  positive_signs_clear: 'Positive signs',
  bands_too_noisy: 'Bands too noisy',
  second_opinion_differs: 'Second opinion differs',
  instrument_uncalibrated: 'Uncalibrated instrument',
  visible_levels_not_fitted: 'Heat sign read as measured',
  no_model_reading: 'No model reading',
  band_noise_unknown: 'Band noise unknown',
  serial_class_mismatch: 'Serial and switch differ',
  header_class_mismatch: 'Detector and switch differ',
};

/** A note split into its bold lead and the rest (lead '' when the note has none). */
export function noteParts(n: Note, scan?: ScanResult): { lead: string; rest: string } {
  const d = scan && n.key === 'readers_disagree_strongly' ? disagreeParts(scan) : null;
  if (d) return d;
  const text = noteText(n);
  const lead = NOTE_LEAD[n.key];
  return lead && text.startsWith(lead) ? { lead, rest: text.slice(lead.length) } : { lead: '', rest: text };
}

/** The verdict card's flat-bands note: the headline above it already says "No protein signal at this spot" and the line
 *  under it gives the models' reading, so the note gives only the reason (DECISIONS 123). */
export function flatBandsCard(p: P): string {
  return typeof p.m === 'number' && p.m >= 0.5
    ? 'The protein bands are flat, so the verdict follows the bands, not the models.'
    : 'The protein bands are flat.';
}

export const NOT_BONE_TEXT = 'Check the target: the probe may be on plaster, a label, a coating or the holder.';

/** Every user-facing string, for the lint test. */
export function allStrings(): string[] {
  const p = { m: 1.4, a: 0.4, b: 0.9, c: 4.1, s: 1.2, r: 0.44, at_least: 0.34, strength: 'strong', sign: 'ester', reason: 'sign', serial: 28313, known: 'hires', set: 'standard', name: 'FieldSpec 3', class: 'asd.fieldspec3' };
  const ids: SignId[] = ['wax', 'ester', 'plaster', 'c1', 'burnt'];
  return [
    ...Object.values(NOTE_TEXT).map((f) => f(p)),
    NOTE_TEXT.lift_blocked({ ...p, reason: 'noise' }),
    NOTE_TEXT.signs_reduced({ signs: ['plaster', 'C1'] }),
    NOTE_TEXT.signs_reduced({ signs: ['C1'] }),
    NOTE_TEXT.signs_reduced({ signs: ['plaster'] }),
    ...ids.map((id) => flagText(id)),
    flagText('burnt', 'charred'),
    flagText('burnt', 'calcined'),
    ...ids.map(flagLabel),
    ...Object.values(UNUSABLE_TEXT),
    NOT_BONE_TEXT,
    LOWRANGE_EXPLAIN,
    flatBandsCard({ m: 1.4 }),
    ...Object.values(NOTE_SHORT),
  ];
}
