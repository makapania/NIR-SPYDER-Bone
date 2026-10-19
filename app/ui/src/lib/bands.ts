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

// The close-ups' label line (Matt, 2026-10-19: numbers staggered on two lines read as "a ragged staircase of numbers, not
// as labels"): one line of band numbers per close-up, each centred on its band, or a pair too close for that set as one
// label "2262 · 2284" centred on the pair. Every decision here is made from text widths measured in the font the app is
// actually drawn in (BandCloseUps measures them at runtime): Segoe UI on Windows, SF Pro on a Mac and a Linux fallback
// font are all different widths, and constants tuned on one font put the numbers over the box edge on the others.

/** The clear space (px) two band numbers keep between them on the label line before they are set as one pair label. */
export const NUMBER_GAP = 8;
/** The pair label's separator: a middle dot, with this much space (em) each side of it. */
export const PAIR_SEP = '·';
export const PAIR_SEP_PAD_EM = 0.3;
/** The space (px) every number keeps from its close-up's inner edge (float noise; the widths are measured exactly). */
export const EDGE_CLEAR = 0.1;

/** The label line's type, in the order tried when the numbers do not fit: the design size first, then a little tighter
 *  tracking, then a slightly smaller size (never under 10 px). The whole row uses one of these, so every close-up's
 *  numbers look alike. */
export interface LabelFit {
  /** font size, px */
  fs: number;
  /** letter-spacing, px */
  ls: number;
}
export const LABEL_FITS: LabelFit[] = [
  { fs: 10.5, ls: 0 },
  { fs: 10.5, ls: -0.25 },
  { fs: 10, ls: -0.25 },
  { fs: 10, ls: -0.45 },
];

/** A piece of label text as measured in the rendered font: its width (px) at the design size LABEL_FITS[0] with no
 *  tracking, and its character count (letter-spacing is added per character). */
export interface LabelText {
  w: number;
  n: number;
}
/** A measured text's width at another size and tracking (width scales with the font size; tracking adds per character). */
export const textW = (t: LabelText, f: LabelFit): number => (t.w * f.fs) / LABEL_FITS[0].fs + t.n * f.ls;

/** One band's number on the label line: the band's x (px), the number's text and what hangs to its right (the 1545
 *  asterisk), which does not count for centring. */
export interface NumberSpec {
  x: number;
  num: LabelText;
  tail?: LabelText;
}

/** Where the numbers of one plot window go: each number's shift from its band (px) and, for a pair label, the
 *  separator's centre (px, in the same frame as the bands' x). */
export interface Placement {
  shift: number[];
  sepAt: number | null;
}

const tailW = (n: NumberSpec, f: LabelFit) => (n.tail ? textW(n.tail, f) : 0);

/** The pair form: one label "a · b" centred on the pair's midpoint, with its left and right ends. */
function pairForm(nums: NumberSpec[], sep: LabelText, f: LabelFit): Placement & { l: number; r: number } {
  const [a, b] = nums;
  const wa = textW(a.num, f);
  const wb = textW(b.num, f);
  const s = textW(sep, f);
  const left = (a.x + b.x) / 2 - (wa + s + wb) / 2;
  return { shift: [left + wa / 2 - a.x, left + wa + s + wb / 2 - b.x], sepAt: left + wa + s / 2, l: left, r: left + wa + s + wb + tailW(b, f) };
}

/** Each number centred on its band, with its left and right ends. */
const apartExt = (nums: NumberSpec[], f: LabelFit) =>
  nums.map((n) => {
    const w = textW(n.num, f);
    return { l: n.x - w / 2, r: n.x + w / 2 + tailW(n, f) };
  });

/** Where the band numbers of one plot window go on the label line at type `f`, or null if they do not fit there. `nums`
 *  are in band order; `room` is the stretch (px, in the bands' frame) the numbers may use: to the close-up's inner edges,
 *  or halfway into an axis break. Each number is centred on its band when the numbers keep `gap` between them and stay
 *  inside `room`; else a pair is set as one label centred on the pair's midpoint (2262/2284 always, 1689/1728 when the
 *  close-up is narrow or the font wide), each number still on its own button. One line in every case. */
export function placeNumbers(nums: NumberSpec[], sep: LabelText, room: [number, number], f: LabelFit = LABEL_FITS[0], gap = NUMBER_GAP): Placement | null {
  const eps = 1e-6;
  const inside = (l: number, r: number) => l >= room[0] + EDGE_CLEAR - eps && r <= room[1] - EDGE_CLEAR + eps;
  const ext = apartExt(nums, f);
  if (ext.every((e, i) => inside(e.l, e.r) && (i === 0 || e.l - ext[i - 1].r >= gap - eps))) return { shift: nums.map(() => 0), sepAt: null };
  if (nums.length !== 2) return null;
  const p = pairForm(nums, sep, f);
  return inside(p.l, p.r) ? { shift: p.shift, sepAt: p.sepAt } : null;
}

/** The last resort when even the smallest type does not fit (a very wide font in a very narrow window): the most compact
 *  form at type `f`, moved as a whole inside `room`. Off-centre, but never over the box edge while it is narrower than
 *  the room. */
export function squeezeNumbers(nums: NumberSpec[], sep: LabelText, room: [number, number], f: LabelFit = LABEL_FITS[LABEL_FITS.length - 1]): Placement {
  let p: Placement & { l: number; r: number };
  if (nums.length === 2) p = pairForm(nums, sep, f);
  else {
    const ext = apartExt(nums, f);
    p = { shift: nums.map(() => 0), sepAt: null, l: Math.min(...ext.map((e) => e.l)), r: Math.max(...ext.map((e) => e.r)) };
  }
  const lo = room[0] + EDGE_CLEAR;
  const hi = room[1] - EDGE_CLEAR;
  const d = p.l < lo ? lo - p.l : p.r > hi ? Math.max(hi - p.r, lo - p.l) : 0;
  return { shift: p.shift.map((s) => s + d), sepAt: p.sepAt === null ? null : p.sepAt + d };
}

/** One window's numbers and room, for placing the whole row. */
export interface LabelWindow {
  nums: NumberSpec[];
  room: [number, number];
}

/** The type for the whole row and every window's placement: the first type in LABEL_FITS at which every window's numbers
 *  fit (each window apart where it can, else as a pair label); if none does, the smallest type, squeezed inside. */
export function placeRow(wins: LabelWindow[], sep: LabelText, gap = NUMBER_GAP): { fit: number; placed: Placement[]; squeezed: boolean } {
  for (let k = 0; k < LABEL_FITS.length; k++) {
    const placed = wins.map((w) => placeNumbers(w.nums, sep, w.room, LABEL_FITS[k], gap));
    if (placed.every((p) => p !== null)) return { fit: k, placed: placed as Placement[], squeezed: false };
  }
  const k = LABEL_FITS.length - 1;
  return {
    fit: k,
    placed: wins.map((w) => placeNumbers(w.nums, sep, w.room, LABEL_FITS[k], gap) ?? squeezeNumbers(w.nums, sep, w.room, LABEL_FITS[k])),
    squeezed: true,
  };
}

/** The plot area is inset this much (px) each side of its window (small plot, no axes); a band's x is measured from it. */
export const PLOT_INSET = 4;
/** A band's x (px) across a window `ww` px wide. */
export const bandX = (w: CloseUpWindow, nm: number, ww: number): number => PLOT_INSET + bandAt(w, nm) * (ww - 2 * PLOT_INSET);

/** A close-up's frame: its padding and border each side (px) and the width of an axis break between windows. */
export interface CloseUpFrame {
  pad: number;
  border: number;
  brk: number;
}

/** The measured label texts: each band's number by id, the 1545 asterisk and the pair separator. */
export interface LabelTexts {
  nums: Partial<Record<BandId, LabelText>>;
  tail: LabelText;
  sep: LabelText;
}

/** The narrowest a close-up can be (px, border box) for its numbers to fit on the label line at type `f`, from the
 *  measured widths. The windows share the close-up's inner width by the nm they show, as the layout does. Bands without
 *  a measured text (the scan lacks them) are left out. */
export function closeUpLabelWidth(cu: CloseUp, t: LabelTexts, frame: CloseUpFrame, f: LabelFit = LABEL_FITS[0]): number {
  const wins = cu.windows.map((w) => ({ w, bands: w.bands.filter((id) => t.nums[id]) })).filter((x) => x.bands.length > 0);
  if (!wins.length) return 0;
  const span = wins.reduce((s, x) => s + x.w.range[1] - x.w.range[0], 0);
  let inner = 0;
  wins.forEach(({ w, bands }, i) => {
    const lo = i === 0 ? -frame.pad : -frame.brk / 2;
    const hiPad = i === wins.length - 1 ? frame.pad : frame.brk / 2;
    const fits = (ww: number) =>
      placeNumbers(
        bands.map((id) => ({ x: bandX(w, bandDef(id).nm, ww), num: t.nums[id]!, tail: id === 'nh1545' ? t.tail : undefined })),
        t.sep,
        [lo, ww + hiPad],
        f,
      ) !== null;
    let ww = 4;
    while (ww < 600 && !fits(ww)) ww += 0.5;
    inner = Math.max(inner, (ww * span) / (w.range[1] - w.range[0]));
  });
  return Math.ceil(inner + (wins.length - 1) * frame.brk + 2 * (frame.pad + frame.border));
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
