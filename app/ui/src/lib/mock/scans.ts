// MOCK ONLY. An example high-res session (LabSpec default file names) that exercises every verdict
// state and note: Good, Borderline (incl. Matt's case: ≈ 1.2%, all six bands lit, clean → positive
// signs, and the ZooMS line), Unlikely, flat bands, lift, blocked lift, flags (consolidant, wax, heat),
// contaminants not checked, models disagree, second opinion differs, the three ZooMS lines (all six
// bands, protein bands, the 1545 nm band), Rescan and "Doesn't look like bone". Synthetic spectra; the
// numbers mean nothing.

import { decide, evidenceFrom, zoomsFrom } from './rules';
import { minusD2, n2, synthesise, toReflectance, type Coat, type Synth, type SynthSpec } from './synth';
import type {
  Analysis,
  CheckResult,
  InstrumentClass,
  ModelKey,
  ModelReading,
  ScanResult,
  Sign,
  SignId,
} from '../types';

export const MOCK_FOLDER = 'D:\\LabSpec\\2026-10-03';
export const MOCK_SERIAL = 28313; // a known high-res unit (PLAN Step 3)
const TZ = '-06:00';

export interface MockScanSpec {
  n: number;
  time: string;
  synth: Omit<SynthSpec, 'seed' | 'hires'>;
  /** CONS3 components [OH-corr. 2045, OH-corr. 1500, F05]; default: m with small jitter. */
  m?: number;
  comps?: [number, number, number];
  ryder?: number;
  s1r2?: number;
  burnt?: boolean;
  c1?: boolean;
  b9?: number;
  /** The OH-corrected 1545 nm band is lit (mock: drawn in the evidence grid; votes a flat band pattern up). */
  lit1545?: boolean;
  unusable?: 'low_signal';
  /** Noise seed override (default from n). */
  seed?: number;
  /** Set for a file that arrived in a watched folder: its real name, path and time (example score only). */
  live?: LiveInfo;
}

/** A real file from a watched folder, scored by this mock until spyder-core is wired in. */
export interface LiveInfo {
  file: string;
  path: string;
  acquiredAt: string;
  arrivedSeq: number;
  revision: number;
  kind: 'sample' | 'reference' | 'unreadable';
  detail?: string;
}

export const MOCK_SPECS: MockScanSpec[] = [
  { n: 21, time: '13:02:11', synth: { c: 0.05, n2: 34, nz: 0.08 }, m: 0.2, lit1545: true },
  { n: 22, time: '13:06:40', synth: { c: 2.0, n2: 78, nz: 0.18 }, m: 2.1 },
  { n: 23, time: '13:11:05', synth: { c: 1.5, n2: 36, nz: 0.08, off: 0.12 }, m: 1.5, burnt: true },
  { n: 24, time: '13:15:52', synth: { c: 0.1, n2: 380, nz: 1.6, off: 0.3 }, m: 0.2, ryder: 0.35 },
  { n: 25, time: '13:20:17', synth: { c: 4.0, n2: 40, nz: 0.1 }, m: 4.1, s1r2: 0.9 },
  { n: 26, time: '13:24:48', synth: { c: 0.7, n2: 38, nz: 0.1, oh: 1.6 }, comps: [0.2, 0.4, 4.2], ryder: -0.6 },
  { n: 27, time: '13:29:30', synth: { kind: 'dark', c: 0 }, unusable: 'low_signal' },
  { n: 28, time: '13:33:02', synth: { c: -0.4, n2: 33, nz: 0.06 }, m: 0.1, lit1545: true },
  { n: 29, time: '13:37:44', synth: { kind: 'plaster', c: 0, n2: 30, nz: 0.05 }, m: 0.4, b9: 0.21 },
  { n: 30, time: '13:42:19', synth: { c: 4.8, n2: 36, nz: 0.08, coat: 'wax' }, comps: [0.3, 0.6, 0.2], ryder: 0.9 },
  { n: 31, time: '13:46:58', synth: { c: 5.5, n2: 35, nz: 0.08 }, comps: [1.6, 1.3, 1.9], ryder: 1.4 },
  { n: 32, time: '13:51:23', synth: { c: 2.5, n2: 37, nz: 0.08, coat: 'ester' }, comps: [3.9, 3.1, 3.6], ryder: 5.8 },
  { n: 33, time: '13:55:40', synth: { c: 2.6, n2: 41, nz: 0.1 }, m: 2.8 },
  { n: 34, time: '14:00:12', synth: { c: 0.3, n2: 39, nz: 0.1 }, m: 0.3, lit1545: true },
  { n: 35, time: '14:04:49', synth: { c: 9.0, n2: 30, nz: 0.07 }, m: 8.8 },
  { n: 36, time: '14:09:31', synth: { c: 0.3, n2: 320, nz: 1.0, off: 0.25 }, m: 1.9, seed: 3 },
  { n: 37, time: '14:14:05', synth: { c: -0.35, n2: 32, nz: 0.06 }, m: 1.4 },
  { n: 38, time: '14:18:37', synth: { c: 3.2, n2: 140, nz: 0.25, off: 0.2 }, m: 3.4 },
  { n: 39, time: '14:23:10', synth: { c: 1.2, n2: 38, nz: 0.09 }, m: 1.2 },
  { n: 40, time: '14:27:46', synth: { c: 4.4, n2: 42, nz: 0.1 }, m: 4.3 },
];

/** Extra scans for "Simulate a new scan" (live arrival). */
export const SIM_SPECS: Omit<MockScanSpec, 'n' | 'time'>[] = [
  { synth: { c: 2.3, n2: 39, nz: 0.09 }, m: 2.2 },
  { synth: { c: 5.6, n2: 35, nz: 0.08 }, m: 5.4 },
  { synth: { c: -0.3, n2: 36, nz: 0.07 }, m: 0.1 },
];

const MODEL_IDS: Record<ModelKey, [string, string]> = {
  cons3: ['collagen.consensus3.median', '1.0.0'],
  wc2045: ['collagen.oh_corrected.2045', '1.0.0'],
  wc1500: ['collagen.oh_corrected.1500', '1.0.0'],
  f05: ['collagen.nh3.1500_2045_2175', '1.0.0'],
  ryder2045: ['collagen.ryder2026.2045', '2.0.0'],
  s1r2: ['collagen.1500_snv.transfer_free', '1.0.0'],
};

const synthCache = new Map<string, Synth>();
export function synthFor(spec: MockScanSpec): Synth {
  const key = `${spec.n}|${spec.seed ?? ""}|${JSON.stringify(spec.synth)}`;
  let s = synthCache.get(key);
  if (!s) {
    s = synthesise({ ...spec.synth, hires: true, seed: spec.seed ?? 1000 + spec.n * 97 });
    synthCache.set(key, s);
  }
  return s;
}

function jitter(seed: number) {
  let a = seed;
  return (s: number) => {
    a = (a * 16807) % 2147483647;
    return (a / 2147483647 - 0.5) * s;
  };
}

const ok = { status: 'assessed' } as const;
const median3 = (a: number, b: number, c: number) => [a, b, c].sort((x, y) => x - y)[1];

function reading(key: ModelKey, value: number | null, status: ModelReading['status'] = ok): ModelReading {
  return { key, id: MODEL_IDS[key][0], version: MODEL_IDS[key][1], value, status, domainNote: false };
}

export function buildResult(spec: MockScanSpec, analysis: Analysis, cls: InstrumentClass): ScanResult {
  const live = spec.live;
  const r = buildScored(spec, analysis, cls);
  if (!live) return r;
  const tagged = { ...r, arrivedSeq: live.arrivedSeq, fileRevision: live.revision, scanKind: live.kind };
  if (live.kind === 'sample') return tagged;
  // Listed, not scored: a white-reference save (PLAN Step 1) or a file that is not a readable ASD scan.
  const none: ScanResult['models'] = { cons3: null, wc2045: null, wc1500: null, f05: null, ryder2045: null, s1r2: null };
  const why = live.kind === 'reference' ? 'reference scan (not scored)' : 'not a readable ASD scan';
  const na = { status: 'not_assessed', reason: why } as const;
  return {
    ...tagged,
    kindDetail: live.detail,
    verdict: 'rescan',
    verdictRule: 'unusable',
    unusableReason: 'unsupported',
    notes: [],
    models: none,
    evidence: { level: 'cant_tell', s: null, bands: [], status: na },
    zooms: { pattern: null, litCount: 0, readableCount: 0 },
    signs: r.signs.map((x) => ({ ...x, fired: false, status: na })),
    checks: r.checks.map((c) => ({ ...c, status: na })),
    alteredOhBand: false,
  };
}

function buildScored(spec: MockScanSpec, analysis: Analysis, cls: InstrumentClass): ScanResult {
  const live = spec.live;
  const file = live?.file ?? `Spectrum000${spec.n}.asd`;
  const hires = cls === 'hires';
  const base = {
    scanId: `scan-${spec.n}`,
    file,
    path: live?.path ?? `${MOCK_FOLDER}\\${file}`,
    acquiredAt: live?.acquiredAt ?? `2026-10-03T${spec.time}${TZ}`,
    // A watched folder's serial is read by the core; the mock does not know it.
    serial: live ? null : MOCK_SERIAL,
    instrumentClass: cls,
    classSource: 'serial_preset' as const,
    transfer: hires ? { id: 'transfer.hires_to_std', version: '0.2.0', provisional: true } : null,
    analysis,
    profileId: `${analysis}@1.0.0`,
    engineVersion: '1.1 (mock)',
  };
  const syn = synthFor(spec);
  const E = minusD2(syn.A, 31);
  const evidence = evidenceFrom(E, syn.sdE);
  if (spec.lit1545) {
    // the OH-corrected 1545 nm trough, lit (u = 2.1; mock value)
    evidence.bands.push({ id: 'nh1545', nm: 1545, e: -1.26, u: 2.1, readable: true, lit: true, state: 'clear', evidenceState: 'clear' });
  }
  const zooms = zoomsFrom(evidence, !!spec.lit1545);
  const n2main = n2(syn.A, 2000, 2100);
  const n2tail = n2(syn.A, 2300, 2400);
  const tailUnreliable = hires && n2tail > 100;
  const b6bGated = n2main > 60;
  const gate = b6bGated ? ({ status: 'gated', reason: 'too noisy for contaminant signs (B6b)' } as const) : ok;
  const c1Status = tailUnreliable
    ? ({ status: 'not_assessed', reason: 'long-wave region too noisy' } as const)
    : gate;
  const signs: Sign[] = (['plaster', 'wax', 'ester', 'burnt', 'c1'] as SignId[]).map((id) => ({
    id,
    fired:
      !b6bGated &&
      ((id === 'wax' && spec.synth.coat === ('wax' as Coat)) ||
        (id === 'ester' && spec.synth.coat === ('ester' as Coat)) ||
        (id === 'burnt' && !!spec.burnt) ||
        (id === 'c1' && !!spec.c1 && !tailUnreliable)),
    status: id === 'c1' ? c1Status : id === 'burnt' ? ok : gate,
    ...(id === 'burnt' && spec.burnt && !b6bGated ? { edge50Nm: 1012, heatKind: 'charred' as const } : {}),
  }));
  const b9 = spec.b9 ?? Math.max(0.45, 0.86 - n2main / 1500);
  const checks: CheckResult[] = [
    { id: 'b1', outcome: spec.unusable ? 'unusable' : 'ok', status: ok },
    { id: 'b2', outcome: 'ok', status: ok },
    { id: 'b3', outcome: 'ok', status: ok },
    { id: 'b4', outcome: 'ok', status: ok },
    { id: 'b5', outcome: (spec.synth.off ?? 0) > 0.15 ? 'note' : 'ok', status: ok },
    { id: 'b6', outcome: n2main > 150 ? 'check' : 'ok', status: ok, value: n2main },
    { id: 'b6b', outcome: b6bGated ? 'note' : 'ok', status: ok, value: n2main },
    { id: 'b7', outcome: 'ok', status: ok },
    { id: 'b8', outcome: 'ok', status: ok },
    {
      id: 'b9',
      outcome: b9 < 0.4 ? 'unusable' : 'ok',
      status: tailUnreliable ? { status: 'not_assessed', reason: 'long-wave region too noisy' } : ok,
      value: b9,
    },
    { id: 'longwave', outcome: tailUnreliable ? 'note' : 'ok', status: ok, value: n2tail },
  ];

  // models
  const j = jitter(7000 + spec.n * 131);
  const m0 = spec.m ?? (spec.comps ? median3(...spec.comps) : 0);
  let comps: [number, number, number] = spec.comps ?? [m0 + j(0.5), m0 + j(0.6), m0 + j(0.5)];
  if (!spec.comps) comps = [comps[0], comps[1], 3 * m0 - comps[0] - comps[1]]; // keep the median near m
  let ryder = spec.ryder ?? comps[0] - 0.6 * (spec.synth.oh ?? 0) + j(0.4);
  let s1 = spec.s1r2 ?? m0 + j(0.8);
  if (!hires) {
    // a high-res file scored without the transfer reads high (PLAN §12 risk 10)
    const up = (v: number) => v * 1.6 + 0.3;
    comps = comps.map(up) as [number, number, number];
    ryder = up(ryder);
  }
  const cons3 = median3(...comps);
  const models: ScanResult['models'] = spec.unusable
    ? { cons3: null, wc2045: null, wc1500: null, f05: null, ryder2045: null, s1r2: null }
    : {
        cons3: reading('cons3', cons3),
        wc2045: reading('wc2045', comps[0]),
        wc1500: reading('wc1500', comps[1]),
        f05: reading('f05', comps[2]),
        ryder2045: reading('ryder2045', ryder),
        s1r2: hires ? reading('s1r2', s1) : null,
      };

  if (spec.unusable) {
    return {
      ...base,
      verdict: 'rescan',
      verdictRule: 'unusable',
      notes: [],
      unusableReason: spec.unusable,
      models,
      evidence: { level: 'cant_tell', s: null, bands: [], status: { status: 'not_assessed', reason: 'scan unusable' } },
      zooms: { pattern: null, litCount: 0, readableCount: 0 },
      signs: signs.map((s) => ({ ...s, fired: false, status: { status: 'not_assessed', reason: 'scan unusable' } })),
      checks,
      alteredOhBand: false,
    };
  }
  const d = decide({
    hires,
    models,
    evidence,
    zooms,
    signs,
    b6bGated,
    c1Assessed: c1Status.status === 'assessed',
  });
  const notes = [...d.notes];
  if (!hires && !live) notes.push({ key: 'serial_class_mismatch', params: { serial: MOCK_SERIAL, known: 'hires', set: 'standard' } });
  const notBone = b9 < 0.4 && !tailUnreliable;
  return {
    ...base,
    verdict: notBone ? 'not_bone' : d.verdict,
    verdictRule: notBone ? 'not_bone' : d.rule,
    notes: notBone ? notes.filter((n) => n.key === 'serial_class_mismatch') : notes,
    models,
    evidence,
    zooms,
    signs,
    checks,
    alteredOhBand: (spec.synth.oh ?? 0) > 1,
  };
}

export interface MockViewsSource {
  A: Float64Array;
  R: Float64Array;
  ohPart: Float64Array | null;
}

export function viewsSource(spec: MockScanSpec): MockViewsSource {
  const s = synthFor(spec);
  return { A: s.A, R: toReflectance(s.A), ohPart: s.ohPart };
}
