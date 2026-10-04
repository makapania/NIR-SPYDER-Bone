// Verdict words, glyph shapes and colour tokens. The SHAPE carries the meaning; colour is redundant
// (01_design §4.1). Colours are fixed across skins and validated for colour-vision deficiency.

import type { Verdict } from './types';

/** Glyph shapes:
 *  full     – filled disc (Good)
 *  half     – half-filled disc (Borderline)
 *  third    – about a third filled (Unlikely)
 *  question – neutral ring with "?" (Can't tell)
 *  dashed   – dashed empty ring (Rescan)
 *  diamond  – dashed neutral diamond (Doesn't look like bone: a different shape, not an alarm) */
export type GlyphShape = 'full' | 'half' | 'third' | 'question' | 'dashed' | 'diamond';
/** Colour tone, mapped to the fixed --v-<tone>, --v-<tone>-ink and --v-<tone>-tint tokens. */
export type Tone = 'good' | 'mid' | 'low' | 'none';

export interface VerdictMeta {
  word: string;
  short: string;
  shape: GlyphShape;
  tone: Tone;
  /** Fill fraction for the disc glyphs. */
  fill: number;
}

export const VERDICT_META: Record<Verdict, VerdictMeta> = {
  good: { word: 'Good candidate', short: 'Good', shape: 'full', tone: 'good', fill: 1 },
  borderline: { word: 'Borderline', short: 'Borderline', shape: 'half', tone: 'mid', fill: 0.5 },
  unlikely: { word: 'Unlikely', short: 'Unlikely', shape: 'third', tone: 'low', fill: 0.3 },
  cant_tell: { word: "Can't tell", short: "Can't tell", shape: 'question', tone: 'none', fill: 0 },
  rescan: { word: 'Rescan', short: 'Rescan', shape: 'dashed', tone: 'none', fill: 0 },
  not_bone: { word: "Doesn't look like bone", short: 'Not bone?', shape: 'diamond', tone: 'none', fill: 0 },
};

export function verdictMeta(v: Verdict): VerdictMeta {
  return VERDICT_META[v];
}

export function toneVar(tone: Tone, part: '' | 'ink' | 'tint' = ''): string {
  return `var(--v-${tone}${part ? '-' + part : ''})`;
}

/** Tone for a bare reading on the % track (no verdict rule applied). */
export function toneForReading(m: number | null): Tone {
  if (m == null) return 'none';
  if (m < 0.5) return 'low';
  if (m < 3) return 'mid';
  return 'good';
}

// One verdict for every analysis (DECISIONS 80 amended): the radiocarbon / isotopes verdict. ZooMS has no picker
// button and no verdict of its own; where the ZooMS band check calls a scan better, a ZooMS line says so.
const NOUN = 'radiocarbon or isotope analysis';

/** The sentence under a model-set verdict. Never quotes a screening line. Below Good it names radiocarbon and
 *  isotopes only, so it never contradicts a ZooMS line ("Better chance with ZooMS: ..."). */
export function verdictSentence(v: Verdict): string {
  const n = NOUN;
  switch (v) {
    case 'good':
      return 'Collagen looks ample for radiocarbon, isotopes or ZooMS.';
    case 'borderline':
      return `Collagen may be enough for ${n}. Worth sampling if this bone matters.`;
    case 'unlikely':
      return `Collagen is probably too low for ${n}. Another area of the bone may do better.`;
    case 'cant_tell':
      return 'The collagen bands are too noisy to read at this spot. A lighter or cleaner area, or more averages, may help.';
    case 'rescan':
      return 'This scan cannot be read. Rescan this spot.';
    case 'not_bone':
      return 'Check the target: the spectrum does not resemble bone.';
  }
}

/** "Most promising first" verdict order (PLAN Step 9 Sorting). */
export const PROMISING_ORDER: Record<Verdict, number> = {
  good: 0,
  borderline: 1,
  cant_tell: 2,
  unlikely: 3,
  rescan: 4,
  not_bone: 4,
};
