// The six collagen bands (PLAN Step 7 and the A17 ZooMS rule): centres, weights, faint thresholds.
// In the real app this table comes from the shipped `bands.json`; the UI only uses it for labels and
// the close-up ranges.

import { LEVEL_WORD } from './display';
import type { BandId, BandReading, BandState, EvidenceLevel, ViewKey } from './types';

export interface BandDef {
  id: BandId;
  nm: number;
  w: number;
  faint: number;
  core: boolean;
  label: string;
  /** What the band is, in the close-ups' tooltips (Matt, 2026-10-18: the correct names). */
  name: string;
}

export const BANDS: BandDef[] = [
  { id: 'ch1689', nm: 1689, w: 0.85, faint: 0, core: true, label: 'C–H', name: 'C–H (CH₃)' },
  { id: 'ch1728', nm: 1728, w: 1.0, faint: 0.5, core: true, label: 'C–H', name: 'C–H (CH₂)' },
  { id: 'nh2044', nm: 2044, w: 1.6, faint: 0, core: false, label: 'N–H', name: 'protein N–H' },
  { id: 'amide2175', nm: 2175, w: 1.1, faint: 0, core: true, label: 'amide', name: 'protein amide envelope' },
  { id: 'ch2262', nm: 2262, w: 1.6, faint: 1, core: true, label: 'combination', name: 'combination band (assignment uncertain)' },
  { id: 'ch2284', nm: 2284, w: 1.14, faint: 0, core: false, label: 'amide III + CH₂', name: 'combination band (amide III + CH₂)' },
];

/** The OH-corrected 1545 nm band (DECISIONS 75): a 5th evidence core band and a ZooMS vote. It is read on the
 *  OH-corrected 1500–1550 nm window and is a trough there (u = −E / w), so it is listed apart from the six. */
export const BAND_1545: BandDef = { id: 'nh1545', nm: 1545, w: 0.6, faint: 1.5, core: true, label: 'N–H', name: 'N–H, OH-corrected' };

/** A band's definition by id (the six, then 1545). */
export function bandDef(id: BandId): BandDef {
  return id === 'nh1545' ? BAND_1545 : (BANDS.find((b) => b.id === id) ?? BANDS[0]);
}

/** One plotted stretch of a close-up: an x-range and the bands drawn in it. */
export interface CloseUpWindow {
  range: [number, number];
  bands: BandId[];
}

export interface CloseUp {
  /** The family name over the box (Matt, 2026-10-18). */
  family: string;
  /** The family name where the close-up is as narrow as v0.3's (the full name is its tooltip there). */
  short?: string;
  /** One window, or two side by side with an axis break between them (the stretch between carries neither band). */
  windows: CloseUpWindow[];
  /** The curve the rule reads: the SG 31 evidence kernel, or the OH-corrected model window for 1545. */
  view: Extract<ViewKey, 'D2_31' | 'D2_31_ohc'>;
  /** Collagen shows as a dip here (1545 on the OH-corrected window), not a peak. */
  dip?: boolean;
}

// The close-ups' x-ranges, cut to each band's own shape, its main lobe and shoulders (Matt, 2026-10-18: "we don't need
// all the area between the signal"; the saved width goes to bigger peaks). Measured on the 140 Ryder bones (the SG 31
// kernel, bands up; the collagen pattern = bones >= 3% minus bones < 1%): planning/work/design/band_windows.py.

/** 1545 nm on the OH-corrected window: a side lobe at ~1526 nm, then the dip at ~1546 nm (the rule reads 1543–1547).
 *  1506–1514 nm is the quiet stretch (the flat part near 1500 that looked strange), so it is left out; the lobe stays,
 *  as the other close-ups keep their shoulders. The corrected curve ends at 1550 nm (NaN beyond); 5 nm of padding, so
 *  "1545*" centred on its band stays inside the box even in the narrowest close-up. */
export const CLOSEUP_1545_RANGE: [number, number] = [1515, 1555];
/** C–H 1689 and 1728 nm: main lobes 1677–1699 and 1719–1746 share the trough between them; outer shoulders 1666, ~1753. */
export const CLOSEUP_CH_RANGE: [number, number] = [1665, 1755];
/** N–H 2044 nm: main lobe 2031–2060, shoulders 2019 and 2076. */
export const CLOSEUP_2044_RANGE: [number, number] = [2018, 2078];
/** The amide envelope at 2175 nm: a broad main lobe 2154–2195, shoulders ~2133 and ~2205. 2078–2135 nm carries
 *  neither protein band and is cut (an axis break). */
export const CLOSEUP_2175_RANGE: [number, number] = [2135, 2210];
/** 2262 and 2284 nm merge into one broad lobe 2252–2300 (dip ~2274), strong left shoulder at 2238. */
export const CLOSEUP_COMBINATION_RANGE: [number, number] = [2238, 2305];

/** The band close-ups, in wavelength order (01_design §5.2; the 1545 nm band joined 2026-10-18). */
export const CLOSEUPS: CloseUp[] = [
  { family: 'N–H', windows: [{ range: CLOSEUP_1545_RANGE, bands: ['nh1545'] }], view: 'D2_31_ohc', dip: true },
  { family: 'C–H', windows: [{ range: CLOSEUP_CH_RANGE, bands: ['ch1689', 'ch1728'] }], view: 'D2_31' },
  {
    family: 'Protein',
    windows: [
      { range: CLOSEUP_2044_RANGE, bands: ['nh2044'] },
      { range: CLOSEUP_2175_RANGE, bands: ['amide2175'] },
    ],
    view: 'D2_31',
  },
  { family: 'Combination', short: 'Comb.', windows: [{ range: CLOSEUP_COMBINATION_RANGE, bands: ['ch2262', 'ch2284'] }], view: 'D2_31' },
];

/** Every band a close-up shows, in order. */
export const closeUpBands = (cu: CloseUp): BandId[] => cu.windows.flatMap((w) => w.bands);

/** The nm a close-up shows (its windows' spans): its share of the row, so every close-up gets about the same px per nm. */
export const closeUpSpan = (cu: CloseUp): number => cu.windows.reduce((t, w) => t + w.range[1] - w.range[0], 0);

/** A band's position across its window, from 0 (the window's short end) to 1. */
export const bandAt = (w: CloseUpWindow, nm: number): number => (nm - w.range[0]) / (w.range[1] - w.range[0]);

/** The clear space (px) two band numbers keep between them on the label line before they are set as one pair label. */
export const NUMBER_GAP = 8;
/** The pair label's separator (a middle dot between thin spaces) and its width in px at the labels' size. */
export const PAIR_SEP = ' · ';
export const PAIR_SEP_W = 9;

/** Where the band numbers of one plot window go on the close-ups' single label line (Matt, 2026-10-19: numbers staggered
 *  on two lines read as "a ragged staircase of numbers, not as labels"). `xs` are the bands' x (px, ascending), `ws` their
 *  numbers' widths. Each number is centred on its band; when two numbers would come closer than `gap` (2262/2284 always,
 *  1689/1728 when the close-up is narrow), the pair is set as one label "2262 · 2284" centred on the pair's midpoint,
 *  each number still on its own button. Returns each number's shift from its band (px) and, for a pair label, the
 *  separator's centre (px, in the same frame as `xs`). One line in every case. */
export function placeNumbers(xs: number[], ws: number[], gap = NUMBER_GAP, sep = PAIR_SEP_W): { shift: number[]; sepAt: number | null } {
  if (xs.length !== 2) return { shift: xs.map(() => 0), sepAt: null };
  const [xa, xb] = xs;
  const [wa, wb] = ws;
  if (xb - xa >= (wa + wb) / 2 + gap) return { shift: [0, 0], sepAt: null };
  const left = (xa + xb) / 2 - (wa + sep + wb) / 2;
  return { shift: [left + wa / 2 - xa, left + wa + sep + wb / 2 - xb], sepAt: left + wa + sep / 2 };
}

/** Why the 1545 nm band is marked with an asterisk (Matt, 2026-10-18: one star and a tooltip, not "OH-corrected" in the
 *  title). Only NH1545c has a projection (plugins/bands.json); the six others, 2044 included, are read on the plain curve. */
export const OHC_NOTE =
  'Read on the OH-corrected curve: the altered OH / bound-water direction is taken out first, so collagen shows here as a dip, not a peak. The other bands are read on the plain curve.';

const STATE_WORD: Record<BandState, string> = { strong: 'Strong', clear: 'Clear', trace: 'Trace', flat: 'Flat', cant_tell: 'Too noisy' };

/** A band's state in words. A band lit for ZooMS but under its faint threshold says so (the organic-evidence level
 *  counts it as flat). */
export function stateWord(b: BandReading): string {
  return b.evidenceState === 'faint' ? 'Faint' : STATE_WORD[b.state];
}

/** A band's tooltip: wavelength, what it is, its state and its standing caveat. */
export function bandTitle(b: BandReading): string {
  const d = bandDef(b.id);
  const caveat =
    b.id === 'nh2044'
      ? ' It sits on the OH/water band shoulder, so it can only add evidence.'
      : b.id === 'ch2284'
        ? ' Read for the ZooMS band pattern only, not for the evidence level (glue side lobes and Mg–OH clay can light it).'
        : b.id === 'nh1545'
          ? ` ${OHC_NOTE}`
          : '';
  const faint = b.evidenceState === 'faint' ? ' Lit, but below this band’s faint threshold: the evidence level counts it as flat.' : '';
  return `${b.nm} nm, ${d.name}: ${stateWord(b).toLowerCase()}.${faint}${caveat}`;
}

/** What the scans list's band summary counts: the bands drawn lit (trace, clear or strong) out of every band shown in
 *  the close-ups (the six ZooMS-pattern bands and the OH-corrected 1545 nm band), and how many were too noisy. */
export interface BandCount {
  lit: number;
  total: number;
  noisy: number;
}

const isLit = (s: BandState) => s === 'trace' || s === 'clear' || s === 'strong';

export function bandCount(bands: BandReading[]): BandCount {
  return {
    lit: bands.filter((b) => isLit(b.state)).length,
    total: bands.length,
    noisy: bands.filter((b) => b.state === 'cant_tell').length,
  };
}

/** The list summary's tooltip: the level, the count and every band in wavelength order. The count is not a vote count
 *  (Astra, 2026-10-18): the level comes from the band rules, which weigh the bands differently. */
export function bandSummaryTitle(level: EvidenceLevel, bands: BandReading[]): string {
  if (!bands.length) return 'No band reading';
  const c = bandCount(bands);
  const byNm = [...bands].sort((a, b) => a.nm - b.nm);
  const list = byNm.map((b) => `${b.nm} ${stateWord(b).toLowerCase()}`).join(', ');
  const noisy = c.noisy ? `, ${c.noisy} too noisy` : '';
  return (
    `Protein bands: ${LEVEL_WORD[level]}. ${c.lit} of the ${c.total} bands in the close-ups are lit${noisy}: ${list}. ` +
    'The count is a quick look, not a vote: the level comes from the band rules, which weigh the bands differently.'
  );
}

/** Bands in wavelength order, grouped as in the close-ups (the 1545 band first). */
export function bandsByCloseUp(bands: BandReading[]): BandReading[][] {
  return CLOSEUPS.map((cu) => closeUpBands(cu).map((id) => bands.find((b) => b.id === id)).filter((b): b is BandReading => !!b)).filter(
    (g) => g.length > 0,
  );
}
