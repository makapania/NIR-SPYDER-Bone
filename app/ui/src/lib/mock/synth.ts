// MOCK ONLY. Synthetic example spectra for the browser mock.
//
// The reference spectra are the public draft references (mean absorbance of the Ryder et al. 2026
// mmc2 reference bones in five yield bands; planning/work/bundles/public/draft_references.json).
// A mock scan is built from them so its collagen bands sit on the same scale as the overlays:
//   A = blend of reference absorbances at a chosen collagen level
//     + baseline offset and tilt
//     + an altered OH/water component (Gaussians at 1450/1932 nm and a shoulder near 1990 nm)
//     + optional coating bands (ester consolidant or paraffin wax, sharp Gaussians)
//     + noise (white, which drives N2, plus band-scale correlated noise, rising above 2300 nm on
//       high-res as the white-reference signal falls).
// Non-bone targets (plaster) and an unusable dark scan are pure Gaussian/baseline constructions.
// None of these numbers mean anything scientifically.

import refsJson from './draft_references.json';
import { savgol, sgWeights } from './sg';

export const GRID = { startNm: 350, stepNm: 1, n: 2151 } as const;
export const N = GRID.n;
export const idx = (nm: number) => Math.round(nm - GRID.startNm);
export const LAMBDA = Float64Array.from({ length: N }, (_, i) => GRID.startNm + i);

interface RefJson {
  label: string;
  n: number;
  mean_yield_pct: number;
  absorbance: number[];
}
const REFS = (refsJson as unknown as { references: RefJson[] }).references;
export const REF_META = REFS.map((r) => ({ label: r.label, n: r.n, meanYield: r.mean_yield_pct }));
export const REF_A = REFS.map((r) => Float64Array.from(r.absorbance));

export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
export function gauss(r: () => number): number {
  let u = 0;
  while (!u) u = r();
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * r());
}

const G = (l: number, mu: number, s: number) => Math.exp(-((l - mu) ** 2) / (2 * s * s));

/** Reference absorbance blended to collagen level c (piecewise linear in mean yield). */
export function blendReferences(c: number): Float64Array {
  const ys = REF_META.map((r) => r.meanYield);
  // below 0 extrapolates past the 0% reference: "flatter than collagen-free bone"
  const cc = Math.max(-0.6, Math.min(c, 13));
  let k = ys.length - 2;
  for (let i = 0; i < ys.length - 1; i++) {
    if (cc <= ys[i + 1]) {
      k = i;
      break;
    }
  }
  const t = (cc - ys[k]) / (ys[k + 1] - ys[k]);
  const a = REF_A[k];
  const b = REF_A[k + 1];
  const out = new Float64Array(N);
  for (let i = 0; i < N; i++) out[i] = a[i] + (b[i] - a[i]) * t;
  return out;
}

export type Coat = 'ester' | 'wax';
const COATS: Record<Coat, [number, number, number][]> = {
  ester: [
    [1690, 9, 0.004],
    [1728, 9, 0.011],
    [1765, 7, 0.006],
    [2140, 10, 0.006],
    [2254, 6, 0.009],
    [2300, 9, 0.006],
  ],
  wax: [
    [1210, 6, 0.004],
    [1728, 7, 0.01],
    [1766, 6, 0.01],
    [2310, 7, 0.014],
    [2350, 8, 0.012],
  ],
};
const GYPSUM: [number, number, number][] = [
  [1445, 14, 0.06],
  [1490, 14, 0.07],
  [1535, 12, 0.04],
  [1750, 12, 0.025],
  [1945, 22, 0.22],
  [2215, 14, 0.03],
  [2265, 14, 0.035],
];

export interface SynthSpec {
  kind?: 'bone' | 'plaster' | 'dark';
  /** Collagen level the bands are drawn at (not necessarily what the models read). */
  c: number;
  off?: number;
  tilt?: number;
  /** Altered OH/water component, 0 = like the reference bones. */
  oh?: number;
  coat?: Coat;
  /** White noise level as an N2 target (×1e-5 absorbance). */
  n2?: number;
  /** Band-scale noise: target SD of E = −1e5·SG31 d²A (×1e-5). */
  nz?: number;
  hires?: boolean;
  seed: number;
}

export interface Synth {
  A: Float64Array;
  /** The OH/water component that was added (the mock's "OH direction"). */
  ohPart: Float64Array;
  /** SD of E at each wavelength from the added noise (for band readability). */
  sdE: (nm: number) => number;
}

let unitCorr: { sd: number } | null = null;

function correlatedNoise(r: () => number): Float64Array {
  const raw = Float64Array.from({ length: N + 12 }, () => gauss(r));
  const out = new Float64Array(N);
  for (let i = 0; i < N; i++) {
    let s = 0;
    for (let j = 0; j < 13; j++) s += raw[i + j] * Math.sin(((j + 0.5) * Math.PI) / 13);
    out[i] = s;
  }
  return out;
}

/** SD of E for unit correlated noise, measured once on a long realisation. */
function corrUnitSd(): number {
  if (unitCorr) return unitCorr.sd;
  const r = mulberry32(99);
  const x = correlatedNoise(r);
  const d = savgol(x, 31);
  let s = 0;
  let k = 0;
  for (let i = idx(1100); i <= idx(2450); i++) {
    s += (d[i] * 1e5) ** 2;
    k++;
  }
  unitCorr = { sd: Math.sqrt(s / k) };
  return unitCorr.sd;
}

const tailFactor = (nm: number, hires: boolean) => (hires ? 1 + 1.2 / (1 + Math.exp(-(nm - 2300) / 25)) : 1);

export function synthesise(sp: SynthSpec): Synth {
  const r = mulberry32(sp.seed);
  const kind = sp.kind ?? 'bone';
  const hires = !!sp.hires;
  let A: Float64Array;
  const ohPart = new Float64Array(N);
  if (kind === 'plaster') {
    A = new Float64Array(N);
    for (let i = 0; i < N; i++) {
      const l = LAMBDA[i];
      let a = 0.11 + 0.03 * ((l - 350) / 2150) + 0.25 * Math.exp(-(l - 350) / 120);
      for (const [mu, s, h] of GYPSUM) a += h * G(l, mu, s);
      A[i] = a;
    }
  } else if (kind === 'dark') {
    A = new Float64Array(N);
    for (let i = 0; i < N; i++) A[i] = 1.55 + 0.1 * Math.sin(LAMBDA[i] / 300);
  } else {
    A = blendReferences(sp.c);
    const oh = sp.oh ?? 0;
    const coat = sp.coat ? COATS[sp.coat] : [];
    for (let i = 0; i < N; i++) {
      const l = LAMBDA[i];
      const p = oh * (0.07 * G(l, 1450, 36) + 0.17 * G(l, 1932, 40) + 0.05 * G(l, 1990, 40));
      ohPart[i] = p;
      let a = A[i] + (sp.off ?? 0) + (sp.tilt ?? 0) * ((l - 1700) / 1000) + p;
      for (const [mu, s, h] of coat) a += h * G(l, mu, s);
      A[i] = a;
    }
  }
  // noise
  const n2 = kind === 'dark' ? 900 : (sp.n2 ?? 28);
  const nz = kind === 'dark' ? 3 : (sp.nz ?? 0.05);
  const sigmaW = (n2 * 1e-5) / Math.sqrt(6);
  const corr = correlatedNoise(r);
  const k = nz / corrUnitSd();
  for (let i = 0; i < N; i++) {
    const f = tailFactor(LAMBDA[i], hires);
    A[i] += f * (sigmaW * gauss(r) + k * corr[i]);
  }
  // white-noise contribution to SD(E) through SG31 (weights norm), plus the correlated part
  const wNorm = Math.hypot(...sgWeights(31));
  const sdE = (nm: number) => tailFactor(nm, hires) * Math.hypot(nz, sigmaW * 1e5 * wNorm);
  return { A, ohPart, sdE };
}

export function toReflectance(A: Float64Array): Float64Array {
  return A.map((a) => 10 ** -a);
}

/** E = −1e5 × SG(w, 3, 2) of A (bands point up). */
export function minusD2(A: ArrayLike<number>, w: number): Float64Array {
  return savgol(A, w).map((v) => -v * 1e5);
}

/** Canonical-style N2 (PLAN Step 2): 1.4826 × MAD of the plain second difference of A over
 *  lo <= λ < hi, in 1e-5 absorbance. Mock re-implementation for display only. */
export function n2(A: Float64Array, lo: number, hi: number): number {
  const v: number[] = [];
  for (let i = Math.max(1, idx(lo)); i < Math.min(N - 1, idx(hi)); i++) v.push(A[i + 1] - 2 * A[i] + A[i - 1]);
  const med = (x: number[]) => {
    const s = [...x].sort((a, b) => a - b);
    const m = s.length >> 1;
    return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
  };
  const md = med(v);
  return 1.4826 * med(v.map((x) => Math.abs(x - md))) * 1e5;
}
