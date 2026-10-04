// The six collagen bands (PLAN Step 7 and the A17 ZooMS rule): centres, weights, faint thresholds.
// In the real app this table comes from the shipped `bands.json`; the UI only uses it for labels and
// the close-up ranges.

import type { BandId } from './types';

export interface BandDef {
  id: BandId;
  nm: number;
  w: number;
  faint: number;
  core: boolean;
  label: string;
}

export const BANDS: BandDef[] = [
  { id: 'ch1689', nm: 1689, w: 0.85, faint: 0, core: true, label: 'C–H' },
  { id: 'ch1728', nm: 1728, w: 1.0, faint: 0.5, core: true, label: 'C–H' },
  { id: 'nh2044', nm: 2044, w: 1.6, faint: 0, core: false, label: 'N–H' },
  { id: 'amide2175', nm: 2175, w: 1.1, faint: 0, core: true, label: 'amide' },
  { id: 'ch2262', nm: 2262, w: 1.6, faint: 1, core: true, label: 'C–H' },
  { id: 'ch2284', nm: 2284, w: 1.14, faint: 0, core: false, label: 'C–H' },
];

/** The OH-corrected 1545 nm band (DECISIONS 75): a 5th evidence core band and a ZooMS vote. It is read on the
 *  OH-corrected 1500–1550 nm window and is a trough there (u = −E / w), so it is listed apart from the six. */
export const BAND_1545: BandDef = { id: 'nh1545', nm: 1545, w: 0.6, faint: 1.5, core: true, label: 'N–H' };

/** A band's definition by id (the six, then 1545). */
export function bandDef(id: BandId): BandDef {
  return id === 'nh1545' ? BAND_1545 : (BANDS.find((b) => b.id === id) ?? BANDS[0]);
}

/** The three evidence close-ups (01_design §5.2): ranges and the bands they show. */
export const CLOSEUPS = [
  { title: 'C–H 1689 · 1728', range: [1655, 1765] as [number, number], bands: [0, 1] },
  { title: 'Protein 2044 · 2175', range: [2010, 2220] as [number, number], bands: [2, 3] },
  { title: 'C–H 2262 · 2284', range: [2236, 2310] as [number, number], bands: [4, 5] },
];
