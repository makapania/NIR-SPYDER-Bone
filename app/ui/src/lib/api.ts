// api.ts is the ONLY IPC boundary between the UI and the Rust side (PLAN §2.2). Components import
// `api` from here and nothing from @tauri-apps directly.
//
// Two implementations:
//  - TauriApi: inside the desktop shell. Everything real: spyder-core analyses the opened files and every
//    watched-folder arrival in Rust; the session's results, the display arrays (binary float32) and the CSV
//    export come from Rust. The UI computes nothing numerical.
//  - BrowserApi: plain `npm run dev` / tests / Playwright. Everything from the mock (example scans).
//
// Live folder watching (PLAN Phase 5): Rust watches the folder, settles each file, analyses the immutable
// snapshot with spyder-core into the session and only then sends `watch-arrival`, carrying the new `scanId`.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { ask, open, save } from '@tauri-apps/plugin-dialog';
import { checkTestSignal, decodeF32, encodeF32, ipcTestSignal, splitViews } from './f32';
import { folderName } from './live';
import { MockBackend, VIEW_ORDER } from './mock';
import type {
  Analysis,
  AppInfo,
  DisplayInfo,
  ExportResult,
  InstrumentClass,
  IpcSelfTest,
  ReferenceSpectrum,
  ScanResult,
  SessionInfo,
  SpectrumViews,
  WatchArrival,
  WatchIncomplete,
  WatchProbe,
  WatchStatus,
} from './types';

export interface OpenResult {
  paths: string[];
  message: string;
  /** True when a new session replaced the list. */
  opened?: boolean;
  /** A later Open or Watch replaced this one before it finished (nothing of it reaches the screen). */
  superseded?: boolean;
}

export interface WatchHandlers {
  arrival: (a: WatchArrival) => void;
  status: (s: WatchStatus) => void;
  incomplete: (i: WatchIncomplete) => void;
}

export interface WatchStartResult {
  status: WatchStatus | null;
  message: string;
}

export interface SpyderApi {
  appInfo(): Promise<AppInfo>;
  /** Round-trips a known float32 array over the binary path and checks it. */
  ipcSelfTest(): Promise<IpcSelfTest>;
  session(): Promise<SessionInfo>;
  setInstrumentClass(c: InstrumentClass): Promise<SessionInfo>;
  listScans(analysis: Analysis): Promise<ScanResult[]>;
  /** Display arrays for one scan (float32, binary path). `smoothing` is the display SG window. */
  scanViews(scanId: string, smoothing: number): Promise<SpectrumViews>;
  references(smoothing: number): Promise<ReferenceSpectrum[]>;
  openFiles(): Promise<OpenResult | null>;
  openFolder(): Promise<OpenResult | null>;
  /** Open these paths without a dialog (files, or one folder). Used by the dialogs and by test hooks. */
  openPaths(paths: string[], folder: boolean): Promise<OpenResult>;
  /** What the charts can draw (view order, OH-corrected windows and why one is unavailable). */
  displayInfo(): Promise<DisplayInfo>;
  /** "Export CSV…": a save dialog, then the core's CSV of the session's scans for this analysis type. */
  /** Export: the readable CSV at the chosen path, the technical CSV beside it as "<name> (technical).csv". */
  exportCsv(analysis: Analysis, readable: string): Promise<{ message: string; result: ExportResult | null } | null>;
  /** Export to a given path (no dialog). */
  exportCsvTo(path: string, analysis: Analysis, readable?: string): Promise<ExportResult>;
  /** Example data only: adds a scan as if it had just arrived in the watched folder. */
  simulateScan(): Promise<string>;
  /** Native menu actions (macOS menu bar). Returns an unsubscribe function. */
  onMenu(cb: (id: string) => void): Promise<() => void>;
  /** Files or folders dragged onto the window: `hover` while they are over it, `drop` with their paths. */
  onFileDrop(h: { hover: (over: boolean) => void; drop: (paths: string[]) => void }): Promise<() => void>;

  // ---- live folder watching ----
  /** Pick a folder and start watching it. Asks once per folder whether to include scans already there. */
  watchFolder(): Promise<WatchStartResult | null>;
  /** Watch this folder (no dialog; `includeExisting` answers the question, null asks). */
  watchPath(folder: string, includeExisting: boolean | null): Promise<WatchStartResult>;
  /** On launch: resume watching the folder watched when the app last closed (null if none). */
  watchResumeLast(): Promise<WatchStatus | null>;
  watchPause(): Promise<WatchStatus | null>;
  watchResume(): Promise<WatchStatus | null>;
  watchStop(): Promise<void>;
  /** Subscribe to arrivals, status changes and incomplete-file notices. Returns an unsubscribe function. */
  onWatch(h: WatchHandlers): Promise<() => void>;
  /** The scan an arrival became. Desktop: already analysed by spyder-core (its `scanId`). Browser: the mock
   *  gives it an example score. */
  scoreArrival(a: WatchArrival): Promise<string>;
  /** The immutable bytes of an arrival (binary IPC). */
  snapshotBytes(snapshotId: number): Promise<Uint8Array>;
}

const N = 2151;
const BROWSER_ONLY = 'needs the desktop app. This browser preview shows example scans.';

class BrowserApi implements SpyderApi {
  protected mock = new MockBackend();

  async appInfo(): Promise<AppInfo> {
    return { name: 'SPYDER Bone', version: '0.1.0', os: 'browser', arch: '', coreConnected: false, runtime: 'browser' };
  }

  async ipcSelfTest(): Promise<IpcSelfTest> {
    const t0 = performance.now();
    const src = Float64Array.from({ length: N }, (_, i) => ipcTestSignal(i));
    const buf = encodeF32(src);
    const ok = checkTestSignal(decodeF32(buf), N);
    return { ok, n: N, bytes: buf.byteLength, ms: performance.now() - t0, path: 'mock' };
  }

  async session() {
    return this.mock.session();
  }
  async setInstrumentClass(c: InstrumentClass) {
    this.mock.setInstrumentClass(c);
    return this.mock.session();
  }
  async listScans(a: Analysis) {
    return this.mock.scans(a);
  }
  async scanViews(id: string, smoothing: number) {
    return this.mock.views(id, smoothing);
  }
  async references(smoothing: number) {
    return this.mock.references(smoothing);
  }
  async openFiles(): Promise<OpenResult | null> {
    return { paths: [], message: `Opening files ${BROWSER_ONLY}` };
  }
  async openFolder(): Promise<OpenResult | null> {
    return { paths: [], message: `Opening a folder ${BROWSER_ONLY}` };
  }
  async openPaths(paths: string[], _folder: boolean): Promise<OpenResult> {
    return { paths, message: `Opening files ${BROWSER_ONLY}` };
  }
  async displayInfo(): Promise<DisplayInfo> {
    return {
      viewOrder: [...VIEW_ORDER],
      startNm: 350,
      stepNm: 1,
      n: N,
      ohWindows: [
        { label: '2045', modelId: 'collagen.spyder.2045_oh_corrected', loNm: 2030, hiNm: 2060, available: true, reason: null },
        { label: '1500', modelId: 'collagen.spyder.1500_oh_corrected', loNm: 1500, hiNm: 1550, available: true, reason: null },
      ],
      unavailable: null,
    };
  }
  async exportCsv(_analysis: Analysis, _readable: string): Promise<{ message: string; result: ExportResult | null } | null> {
    return { message: `Exporting a CSV ${BROWSER_ONLY}`, result: null };
  }
  async exportCsvTo(_path: string, _analysis: Analysis, _readable?: string): Promise<ExportResult> {
    throw new Error(`Exporting a CSV ${BROWSER_ONLY}`);
  }
  async simulateScan() {
    return this.mock.simulate();
  }
  async onMenu(_cb: (id: string) => void): Promise<() => void> {
    return () => {};
  }
  async onFileDrop(_h: { hover: (over: boolean) => void; drop: (paths: string[]) => void }): Promise<() => void> {
    return () => {};
  }

  async watchFolder(): Promise<WatchStartResult | null> {
    return {
      status: null,
      message: 'Watching a folder needs the desktop app. This browser preview shows example scans; "Simulate a new scan" shows an arrival.',
    };
  }
  async watchPath(_folder: string, _includeExisting: boolean | null): Promise<WatchStartResult> {
    return { status: null, message: `Watching a folder ${BROWSER_ONLY}` };
  }
  async watchResumeLast(): Promise<WatchStatus | null> {
    return null;
  }
  async watchPause(): Promise<WatchStatus | null> {
    return null;
  }
  async watchResume(): Promise<WatchStatus | null> {
    return null;
  }
  async watchStop(): Promise<void> {}
  async onWatch(_h: WatchHandlers): Promise<() => void> {
    return () => {};
  }
  async scoreArrival(a: WatchArrival): Promise<string> {
    return this.mock.addArrival(a);
  }
  async snapshotBytes(_id: number): Promise<Uint8Array> {
    throw new Error('Snapshots exist only in the desktop app.');
  }
}

interface RefMeta {
  id: string;
  label: string;
  legend: string;
  meanYieldPct: number;
  n: number;
}

const toBytes = (raw: ArrayBuffer | number[] | Uint8Array): Uint8Array =>
  raw instanceof Uint8Array ? raw : raw instanceof ArrayBuffer ? new Uint8Array(raw) : Uint8Array.from(raw);

class TauriApi extends BrowserApi {
  override async appInfo(): Promise<AppInfo> {
    const info = await invoke<Omit<AppInfo, 'runtime'>>('app_info');
    return { ...info, runtime: 'tauri' };
  }

  override async ipcSelfTest(): Promise<IpcSelfTest> {
    const t0 = performance.now();
    const raw = await invoke<ArrayBuffer | number[]>('ipc_selftest', { n: N });
    const a = decodeF32(raw);
    return { ok: checkTestSignal(a, N), n: a.length, bytes: a.byteLength, ms: performance.now() - t0, path: 'tauri-binary' };
  }

  private info: DisplayInfo | null = null;
  private refMeta: RefMeta[] | null = null;

  override async displayInfo(): Promise<DisplayInfo> {
    this.info ??= await invoke<DisplayInfo>('display_info');
    return this.info;
  }

  override async session(): Promise<SessionInfo> {
    return invoke<SessionInfo>('session_get');
  }

  override async setInstrumentClass(c: InstrumentClass): Promise<SessionInfo> {
    return invoke<SessionInfo>('session_set_class', { class: c });
  }

  override async listScans(a: Analysis): Promise<ScanResult[]> {
    return invoke<ScanResult[]>('session_scans', { analysis: a });
  }

  override async scanViews(id: string, smoothing: number): Promise<SpectrumViews> {
    const info = await this.displayInfo();
    const raw = await invoke<ArrayBuffer | number[]>('scan_views', { id, smoothing });
    const block = decodeF32(raw);
    return { id, startNm: info.startNm, stepNm: info.stepNm, n: info.n, smoothing, views: splitViews(block, info.n, info.viewOrder) };
  }

  override async references(smoothing: number): Promise<ReferenceSpectrum[]> {
    const info = await this.displayInfo();
    this.refMeta ??= await invoke<RefMeta[]>('reference_meta');
    const raw = await invoke<ArrayBuffer | number[]>('reference_views', { smoothing });
    const block = decodeF32(raw);
    const per = info.n * info.viewOrder.length;
    return this.refMeta.map((m, k) => ({
      ...m,
      spectra: {
        id: m.id,
        startNm: info.startNm,
        stepNm: info.stepNm,
        n: info.n,
        smoothing,
        views: splitViews(block.subarray(k * per, (k + 1) * per), info.n, info.viewOrder),
      },
    }));
  }

  override async openFiles(): Promise<OpenResult | null> {
    const sel = await open({ multiple: true, directory: false, filters: [{ name: 'ASD scans', extensions: ['asd'] }] });
    if (!sel) return null;
    return this.openPaths(Array.isArray(sel) ? sel : [sel], false);
  }

  override async openFolder(): Promise<OpenResult | null> {
    const sel = await open({ directory: true, multiple: false });
    if (!sel || Array.isArray(sel)) return null;
    return this.openPaths([sel], true);
  }

  override async openPaths(paths: string[], folder: boolean): Promise<OpenResult> {
    const r = await invoke<{ session: SessionInfo; count: number; message: string; superseded: boolean }>('session_open', {
      paths,
      folder,
    });
    return { paths, message: r.message, opened: !r.superseded, superseded: r.superseded };
  }

  override async exportCsv(analysis: Analysis, readable: string): Promise<{ message: string; result: ExportResult | null } | null> {
    const s = await this.session();
    const base = (s.folder ? folderName(s.folder) : 'scans').replace(/[^\w.-]+/g, '_');
    const path = await save({
      title: 'Export CSV',
      defaultPath: `${base}_spyder_bone.csv`,
      filters: [{ name: 'CSV (UTF-8)', extensions: ['csv'] }],
    });
    if (!path) return null;
    const result = await this.exportCsvTo(path, analysis, readable);
    const tech = result.technicalPath ? ` (every value unrounded: ${result.technicalPath})` : '';
    return { message: `Exported ${result.rows} scan${result.rows === 1 ? '' : 's'} to ${result.path}${tech}.`, result };
  }

  override async exportCsvTo(path: string, analysis: Analysis, readable?: string): Promise<ExportResult> {
    return invoke<ExportResult>('export_csv', { path, analysis, readable: readable ?? null });
  }

  override async onMenu(cb: (id: string) => void): Promise<() => void> {
    const un: UnlistenFn = await listen<string>('menu', (e) => cb(e.payload));
    return un;
  }

  override async onFileDrop(h: { hover: (over: boolean) => void; drop: (paths: string[]) => void }): Promise<() => void> {
    return getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === 'enter' || p.type === 'over') h.hover(true);
      else if (p.type === 'leave') h.hover(false);
      else if (p.type === 'drop') {
        h.hover(false);
        h.drop(p.paths);
      }
    });
  }

  override async watchFolder(): Promise<WatchStartResult | null> {
    const sel = await open({ directory: true, multiple: false, title: 'Watch a folder for new scans' });
    if (!sel || Array.isArray(sel)) return null;
    return this.watchPath(sel, null);
  }

  override async watchPath(folder: string, includeExisting: boolean | null): Promise<WatchStartResult> {
    const probe = await invoke<WatchProbe>('watch_probe', { folder });
    let include = includeExisting ?? probe.remembered?.includeExisting ?? null;
    if (include == null) {
      include =
        probe.existingCount === 0 ||
        (await ask(
          `${probe.existingCount} scan${probe.existingCount === 1 ? ' is' : 's are'} already in this folder. Include ${probe.existingCount === 1 ? 'it' : 'them'} in the list, or show only scans saved from now on?`,
          { title: 'Watch folder', kind: 'info', okLabel: 'Include them', cancelLabel: 'Only new scans' },
        ));
    }
    // Rust starts a new session for the folder before watching, so no arrival is lost.
    const status = await invoke<WatchStatus>('watch_start', { folder, includeExisting: include });
    const what = include && probe.existingCount ? ` (including ${probe.existingCount} scan${probe.existingCount === 1 ? '' : 's'} already there)` : '';
    return { status, message: `Watching ${folder}${what}.` };
  }

  override async watchResumeLast(): Promise<WatchStatus | null> {
    return invoke<WatchStatus | null>('watch_resume_last');
  }

  override async watchPause() {
    return invoke<WatchStatus | null>('watch_pause');
  }
  override async watchResume() {
    return invoke<WatchStatus | null>('watch_resume');
  }
  override async watchStop() {
    await invoke('watch_stop');
  }

  override async onWatch(h: WatchHandlers): Promise<() => void> {
    const uns = await Promise.all([
      listen<WatchArrival>('watch-arrival', (e) => h.arrival(e.payload)),
      listen<WatchStatus>('watch-status', (e) => h.status(e.payload)),
      listen<WatchIncomplete>('watch-incomplete', (e) => h.incomplete(e.payload)),
    ]);
    return () => uns.forEach((u) => u());
  }

  override async scoreArrival(a: WatchArrival): Promise<string> {
    // the snapshot id is unique for the app's lifetime (seq restarts with every watch)
    return a.scanId ?? `unscored-${a.snapshotId}`;
  }

  override async snapshotBytes(snapshotId: number): Promise<Uint8Array> {
    return toBytes(await invoke<ArrayBuffer | number[]>('snapshot_bytes', { id: snapshotId }));
  }
}

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const api: SpyderApi = isTauri ? new TauriApi() : new BrowserApi();
