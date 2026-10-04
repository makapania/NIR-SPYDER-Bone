// Display policy (PLAN v3.2 Step 10; A20; DECISIONS 36). Formatting only: sorting and export always
// use the unrounded values.
//   m < 0.5  -> "< 0.5%"             (no number, never negative)
//   0.5–3    -> "≈ 1.7% (± 1)"       (one decimal; short form drops "(± 1)")
//   3–6      -> "≈ 4%"
//   >= 6     -> "> 6%"
// The consensus (headline, list, verdict input) uses this; each single model shows its own output (formatModel).
// A displayed number never crosses a screening line: 2.96 shows "≈ 2.9%", not "≈ 3.0%", so the text
// cannot contradict a Borderline verdict.

import type { BandReading, EvidenceLevel, ModelKey, Zooms } from './types';

export const LO = 0.5;
export const HI = 3;
export const TOP = 6;

export function formatReading(v: number | null | undefined, short = false): string {
  if (v == null || !Number.isFinite(v)) return '–';
  if (v < LO) return '< 0.5%';
  if (v < HI) {
    const shown = Math.min(Math.floor(v * 10 + 0.5) / 10, 2.9);
    return short ? `≈ ${shown.toFixed(1)}%` : `≈ ${shown.toFixed(1)}% (± 1)`;
  }
  if (v < TOP) return `≈ ${Math.max(3, Math.round(v))}%`;
  return '> 6%';
}

/** A single model's own output (the consensus keeps formatReading). One decimal at every level and no cap, so
 *  the individual models show the data (Matt, 2026-10-03); a reading below zero shows "< 0%". */
export function formatModel(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return '–';
  if (v < 0) return '< 0%';
  return `${v.toFixed(1)}%`;
}

/** Model display names (DECISIONS 77: named by where each model's weight sits). One list for the details panel and
 *  the readable export. */
export const MODEL_NAME: Record<ModelKey, string> = {
  cons3: 'Consensus of three',
  wc2045: '2045 nm, OH-corrected',
  wc1500: '1545 nm, OH-corrected',
  f05: 'N–H set, 2175 + 2045 nm, OH-corrected',
  ryder2045: 'Ryder 2045',
  s1r2: '1545 nm, no transfer needed',
};

export const LEVEL_WORD: Record<EvidenceLevel, string> = {
  none: 'None',
  trace: 'Trace',
  clear: 'Clear',
  strong: 'Strong',
  cant_tell: "Can't tell",
};

/** The ZooMS band check in words (no pattern letters; PLAN v3.2 Changes 12): the close-ups' caption and the details
 *  row. With the core's call (desktop app) it never contradicts the check: Can't tell says which bands are too
 *  noisy. */
export function zoomsHeadline(z: Zooms, bands: BandReading[] = []): string {
  if (z.verdict === 'cant_tell') {
    const protein = bands.filter((b) => b.id === 'nh2044' || b.id === 'amide2175');
    return protein.some((b) => !b.readable) || protein.length === 0
      ? 'Too noisy to read at 2044/2175 nm'
      : 'Too noisy to read every collagen band';
  }
  // the OH-corrected 1545 nm band lifted an Unlikely pattern to Borderline (DECISIONS 75)
  if (z.vote1545)
    return z.pattern === 'A'
      ? 'Flat at the six bands, but the 1545 nm N–H band shows protein'
      : 'Protein bands flat at 2044/2175, but the 1545 nm N–H band shows protein';
  switch (z.pattern) {
    case 'A':
      return 'Flat at all six collagen bands';
    case 'B':
      return 'Protein bands flat, C–H bands only';
    case 'C':
      return 'One protein band resolved';
    case 'D':
      return 'Protein bands resolved, C–H bands incomplete';
    case 'E':
      return 'All six collagen bands resolved';
    default:
      return 'Too noisy to read at 2044/2175 nm';
  }
}

/** Square-root track position (0–100) over 0–7%: the low end gets the room (01_design §4.5). */
export function trackPos(v: number): number {
  return Math.sqrt(Math.min(Math.max(v, 0), 7) / 7) * 100;
}

export function shortName(file: string): string {
  return file.replace(/\.asd$/i, '');
}

export function timeOf(iso: string): string {
  // "2026-10-03T14:27:46+02:00" -> "14:27"
  const m = /T(\d\d:\d\d)(:\d\d)?/.exec(iso);
  return m ? m[1] : iso;
}

export function timeLong(iso: string): string {
  const m = /T(\d\d:\d\d:\d\d)/.exec(iso);
  return m ? m[1] : iso;
}

export function ordinal(n: number): string {
  const s = ['th', 'st', 'nd', 'rd'];
  const v = n % 100;
  return n + (s[(v - 20) % 10] || s[v] || s[0]);
}
