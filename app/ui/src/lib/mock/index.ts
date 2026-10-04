// MOCK ONLY. The in-browser stand-in for spyder-core, used by api.ts when the core is not connected
// (plain `npm run dev`, tests, and the desktop shell until spyder-core is wired in).

import { decodeF32, encodeF32, splitViews } from '../f32';
import { MOCK_FOLDER, MOCK_SERIAL, MOCK_SPECS, SIM_SPECS, buildResult, viewsSource, type MockScanSpec } from './scans';
import { REF_A, REF_META, minusD2, toReflectance, GRID } from './synth';
import { isoLocal, pickIndex } from '../live';
import type {
  Analysis,
  ClassSource,
  InstrumentClass,
  ReferenceSpectrum,
  ScanResult,
  SessionInfo,
  SpectrumViews,
  ViewKey,
  WatchArrival,
} from '../types';

export const VIEW_ORDER: readonly ViewKey[] = ['R', 'A', 'D2', 'D2_31', 'D2_31_ohc', 'R_meas', 'A_meas', 'D2_meas'];

/** The OH-corrected view exists only inside the model windows (as in the desktop app). */
const OHC_WINDOWS: [number, number][] = [
  [1500, 1550],
  [2030, 2060],
];
function windowsOnly(d: ArrayLike<number>): Float64Array {
  const out = new Float64Array(GRID.n).fill(NaN);
  for (const [lo, hi] of OHC_WINDOWS) for (let nm = lo; nm <= hi; nm++) out[nm - GRID.startNm] = d[nm - GRID.startNm];
  return out;
}

const classKey = (folder: string) => `spyder.class.${folder}`;
function loadClass(folder: string): InstrumentClass | null {
  try {
    const v = localStorage.getItem(classKey(folder));
    return v === 'standard' || v === 'hires' ? v : null;
  } catch {
    return null;
  }
}
function saveClass(folder: string, c: InstrumentClass) {
  try {
    localStorage.setItem(classKey(folder), c);
  } catch {
    /* storage unavailable: the switch still works for this session */
  }
}

/** Packs arrays into one float32 block, encodes it to bytes and decodes it again, so the mock runs
 *  through exactly the decode path the binary IPC uses. */
function packViews(id: string, smoothing: number, arrays: Record<ViewKey, ArrayLike<number>>): SpectrumViews {
  const n = GRID.n;
  const block = new Float64Array(n * VIEW_ORDER.length);
  VIEW_ORDER.forEach((k, j) => block.set(arrays[k] as ArrayLike<number>, j * n));
  const decoded = decodeF32(encodeF32(block));
  return { id, startNm: GRID.startNm, stepNm: GRID.stepNm, n, smoothing, views: splitViews(decoded, n, VIEW_ORDER) };
}

/** Example spectra handed out to real files from a watched folder (by content hash), so a live session shows
 *  the whole range of verdicts, flags and notes. The scores mean nothing until spyder-core is wired in. */
const LIVE_POOL: Omit<MockScanSpec, 'n' | 'time'>[] = [...SIM_SPECS, ...MOCK_SPECS.map(({ n: _n, time: _t, ...rest }) => rest)];

export class MockBackend {
  private folder = MOCK_FOLDER;
  private cls: InstrumentClass;
  private source: ClassSource;
  private specs: MockScanSpec[] = [...MOCK_SPECS];
  private simCount = 0;
  private refCache = new Map<number, ReferenceSpectrum[]>();
  /** Set while showing a real (watched) folder instead of the example folder. */
  private liveFolder: string | null = null;
  private nextLiveN = 1001;
  private resultCache = new Map<string, ScanResult>();

  constructor() {
    const saved = loadClass(this.folder);
    this.cls = saved ?? 'hires'; // preset from the known serial
    this.source = saved ? 'user' : 'serial_preset';
  }

  session(): SessionInfo {
    if (this.liveFolder) {
      return {
        folder: this.liveFolder,
        watching: true,
        instrumentClass: this.cls,
        classSource: this.source,
        serial: null,
        serialClass: null,
        example: false,
      };
    }
    return {
      folder: this.folder,
      watching: true,
      instrumentClass: this.cls,
      classSource: this.source,
      serial: MOCK_SERIAL,
      serialClass: 'hires',
      example: true,
    };
  }

  /** Switch to a real watched folder: the list starts empty and fills with arrivals. `cls` is the folder's
   *  remembered switch (null: not set yet, so Standard by default). */
  startLive(folder: string, cls: InstrumentClass | null) {
    this.liveFolder = folder;
    this.specs = [];
    this.cls = cls ?? 'standard';
    this.source = cls ? 'user' : 'default';
    this.resultCache.clear();
  }

  isLive(): boolean {
    return this.liveFolder !== null;
  }

  /** MOCK scorer: an arrival becomes an example scored scan under its real name. A rewritten file replaces
   *  its earlier row. Returns the new scan id. */
  addArrival(a: WatchArrival): string {
    const n = this.nextLiveN++;
    const base = LIVE_POOL[pickIndex(a.sha256, LIVE_POOL.length)];
    const kind = !a.recognised ? 'unreadable' : a.kindHint === 'white_reference_save' ? 'reference' : 'sample';
    const spec: MockScanSpec = {
      ...base,
      n,
      time: '',
      live: {
        file: a.file,
        path: a.path,
        acquiredAt: isoLocal(a.modifiedMs ?? a.receivedMs),
        arrivedSeq: a.seq,
        revision: a.revision,
        kind,
        detail: a.detail,
      },
    };
    this.specs = this.specs.filter((s) => s.live?.path !== a.path);
    this.specs.push(spec);
    return `scan-${n}`;
  }

  setInstrumentClass(c: InstrumentClass) {
    this.cls = c;
    this.source = 'user';
    this.resultCache.clear();
    // A watched folder's switch is remembered by the Rust settings file (api.ts); the example folder's here.
    if (!this.liveFolder) saveClass(this.folder, c);
  }

  scans(analysis: Analysis): ScanResult[] {
    return this.specs.map((s) => {
      const key = `${s.n}|${analysis}|${this.cls}|${this.source}`;
      let r = this.resultCache.get(key);
      if (!r) {
        r = { ...buildResult(s, analysis, this.cls), classSource: this.source };
        this.resultCache.set(key, r);
      }
      return r;
    });
  }

  simulate(): string {
    const base = SIM_SPECS[this.simCount % SIM_SPECS.length];
    this.simCount++;
    const n = 40 + this.simCount;
    const t = new Date(2026, 9, 3, 14, 27, 46 + this.simCount * 251);
    const time = t.toTimeString().slice(0, 8);
    this.specs.push({ ...base, n, time });
    return `scan-${n}`;
  }

  views(scanId: string, smoothing: number): SpectrumViews {
    const spec = this.specs.find((s) => `scan-${s.n}` === scanId);
    if (!spec) throw new Error(`unknown scan ${scanId}`);
    const src = viewsSource(spec);
    const d31 = minusD2(src.A, 31);
    const corrected = src.ohPart ? src.A.map((a, i) => a - src.ohPart![i]) : src.A;
    const d2 = smoothing === 31 ? d31 : minusD2(src.A, smoothing);
    return packViews(scanId, smoothing, {
      R: src.R,
      A: src.A,
      D2: d2,
      D2_31: d31,
      D2_31_ohc: windowsOnly(minusD2(corrected, 31)),
      R_meas: src.R,
      A_meas: src.A,
      D2_meas: d2,
    });
  }

  references(smoothing: number): ReferenceSpectrum[] {
    const hit = this.refCache.get(smoothing);
    if (hit) return hit;
    const refs = REF_META.map((m, k) => {
      const A = REF_A[k];
      const d31 = minusD2(A, 31);
      const label = m.label;
      return {
        id: `ref-${label}`,
        label,
        legend: label,
        meanYieldPct: m.meanYield,
        n: m.n,
        spectra: packViews(`ref-${label}`, smoothing, {
          R: toReflectance(A),
          A,
          D2: smoothing === 31 ? d31 : minusD2(A, smoothing),
          D2_31: d31,
          D2_31_ohc: windowsOnly(d31),
          R_meas: toReflectance(A),
          A_meas: A,
          D2_meas: smoothing === 31 ? d31 : minusD2(A, smoothing),
        }),
      };
    });
    this.refCache.set(smoothing, refs);
    return refs;
  }
}
