// Types that cross the IPC boundary. They mirror PLAN v3.2 Step 10's export fields so the CSV,
// the JSONL session file and the UI all read the same record. The core (spyder-core) owns the
// numbers and the rule outcomes; the UI only formats and draws them.

export type InstrumentClass = 'standard' | 'hires';
/** Who set the instrument class: the user's switch, a known serial preset, a preset from the file header's detector
 *  settings (an unlisted serial; a heuristic), or the default (DECISIONS 50). */
export type ClassSource = 'user' | 'serial_preset' | 'header_preset' | 'default';
/** The core's analysis profiles. The app shows ONE verdict (radiocarbon / isotopes) for every analysis, with a
 *  ZooMS line where the ZooMS band check calls a scan better (DECISIONS 80 amended); it only asks for 'radiocarbon'. */
export type Analysis = 'radiocarbon' | 'zooms' | 'isotopes';

export type Verdict = 'good' | 'borderline' | 'unlikely' | 'cant_tell' | 'rescan' | 'not_bone';

/** The rule step that set the verdict (PLAN Step 9), exported with every row. */
export type VerdictRule =
  | 'model' // item 1: CONS3 against 0.5 / 3
  | 'flat_bands' // item 2: evidence "none" -> Unlikely
  | 'plus_d' // item 3: protein bands flat, m >= 3 -> Borderline
  | 'lift_good' // item 4: strong full pattern on a clean scan
  | 'lift_borderline' // item 4: clear full pattern on a clean scan
  | 'lift_blocked' // item 4: a lift was due but a contaminant sign (or noise) blocked it; the model verdict stands
  | 'no_model_reading' // no collagen model could read the scan: Can't tell
  | 'zooms_pattern' // the core's ZooMS profile (not shown by the app since DECISIONS 80 amended)
  | 'unusable' // B1-B4: Rescan
  | 'not_bone'; // B9 below the cut

/** Every check, band and model exports its status separately from its result. 'skipped' is C1 when a specific
 *  sign already fired (that counts as checked). `note` qualifies an assessed result (e.g. "truncated"). */
export type Assessment =
  | { status: 'assessed'; note?: string }
  | { status: 'not_assessed'; reason: string }
  | { status: 'gated'; reason: string }
  | { status: 'skipped'; reason: string };

export type EvidenceLevel = 'none' | 'trace' | 'clear' | 'strong' | 'cant_tell';

export type BandId = 'ch1689' | 'ch1728' | 'nh2044' | 'amide2175' | 'ch2262' | 'ch2284' | 'nh1545';
/** Display state of one band. "trace", "clear" and "strong" are lit (E > 0, ZooMS definition). */
export type BandState = 'strong' | 'clear' | 'trace' | 'flat' | 'cant_tell';

export interface BandReading {
  id: BandId;
  nm: number;
  /** Mean of E = -1e5 x SG31 d2A over +/-2 nm. */
  e: number;
  /** e / band weight. */
  u: number;
  readable: boolean;
  lit: boolean;
  state: BandState;
  /** The organic-evidence rule's own state (Step 7; 'faint' sits below a band's faint threshold and counts as
   *  flat there). Absent in the mock; null for C–H 2284 (not an evidence band). */
  evidenceState?: 'strong' | 'clear' | 'faint' | 'flat' | 'cant_tell' | null;
}

export interface Evidence {
  level: EvidenceLevel;
  /** Median u over readable core bands; null when no core band is readable. */
  s: number | null;
  bands: BandReading[];
  status: Assessment;
}

/** The ZooMS band check's pattern. The letter is internal (exports); the UI says it in words. */
export type ZoomsPattern = 'A' | 'B' | 'C' | 'D' | 'E' | null;
/** The ZooMS band check (A17 band patterns with the 1545 nm vote). Not a verdict of its own: where it calls a scan
 *  better than the verdict, the core adds a ZooMS line (zooms_better_*; DECISIONS 80 amended). */
export interface Zooms {
  pattern: ZoomsPattern;
  litCount: number;
  readableCount: number;
  /** The band check's call (Good / Borderline / Unlikely / Can't tell), after the 1545 nm vote. */
  verdict?: Verdict;
  /** The OH-corrected 1545 nm band voted Unlikely up to Borderline (DECISIONS 75). */
  vote1545?: boolean;
}

export type ModelKey = 'cons3' | 'wc2045' | 'wc1500' | 'f05' | 'ryder2045' | 's1r2';
export interface ModelReading {
  key: ModelKey;
  id: string; // e.g. collagen.ryder2026.2045
  version: string; // semver
  /** Signed, unrounded prediction (wt% collagen). Null when not assessed. */
  value: number | null;
  status: Assessment;
  /** Grey "less familiar spectrum" note: beyond 3x the 99% T2/Q limits. Never a colour. */
  domainNote: boolean;
  /** Noise propagated to this reading (B6), % collagen. */
  impliedSd?: number | null;
  /** B6 "Check": the noise widens this reading's error. */
  noiseCheck?: boolean;
  /** What the model read: a transfer id@version, or "none" (as measured). */
  transfer?: string | null;
}

/** Contaminant and heat signs. A fired sign is a FLAG (badge + text beside the verdict); it never
 *  changes the verdict (DECISIONS 53). Wax, ester, plaster and C1 still block an evidence lift. */
export type SignId = 'plaster' | 'wax' | 'ester' | 'burnt' | 'c1';
export interface Sign {
  id: SignId;
  fired: boolean;
  status: Assessment;
  /** Heat sign only: where the visible edge reaches half the 1250–1300 nm reflectance. */
  edge50Nm?: number | null;
  /** Heat sign only: what the spectrum marks (charred or calcined both imply the sign fired). */
  heatKind?: 'charred' | 'calcined';
}

export type CheckId = 'b1' | 'b2' | 'b3' | 'b4' | 'b5' | 'b6' | 'b6b' | 'b7' | 'b8' | 'b9' | 'longwave';
export type CheckOutcome = 'ok' | 'note' | 'check' | 'unusable';
export interface CheckResult {
  id: CheckId;
  outcome: CheckOutcome;
  status: Assessment;
  /** Optional numeric result (e.g. B9 score, N2). */
  value?: number;
}

/** Note keys. The core decides which apply; the texts live in one UI string table (strings.ts). */
export type NoteKey =
  // notes of the rule that set the verdict (precedence order). No v1 rule makes radiocarbon or
  // isotopes "Can't tell" (DECISIONS 53 supersedes rule C): contaminant and burnt signs are flags.
  | 'plus_d'
  | 'flat_bands'
  | 'lift_good'
  | 'lift_borderline'
  | 'lift_blocked'
  // the ZooMS line (after the rule's note, before the supporting note; DECISIONS 80 amended)
  | 'zooms_better_good'
  | 'zooms_better_protein'
  | 'zooms_better_1545'
  | 'zooms_better_1545_flat'
  | 'zooms_better_ryder'
  // the faint protein sign for ZooMS (weak evidence: shown only when a slot is free; no list mark)
  | 'zooms_faint_protein'
  // supporting notes (precedence order)
  | 'above_bone_range'
  | 'possible_thin_coating'
  | 'contaminants_not_checked'
  | 'models_disagree'
  | 'positive_signs_all_six'
  | 'positive_signs_clear'
  | 'bands_too_noisy'
  // details only
  | 'c1_reduced'
  | 'second_opinion_differs'
  | 'serial_class_mismatch'
  | 'header_class_mismatch'
  | 'no_transfer_available'
  | 'no_model_reading';

export interface Note {
  key: NoteKey;
  params: Record<string, number | string | string[]>;
}

export interface TransferInfo {
  id: string;
  version: string;
  provisional: boolean;
}

/** Names and order of the float32 display arrays for one scan (binary path). */
/** Names and order of the float32 display arrays for one scan (binary path; Rust `display::VIEW_ORDER`).
 *  R/A/D2: the standard-resolution-equivalent stream (transferred on high-res) at the chart smoothing; D2_31: the
 *  evidence kernel (what the band rule and the model windows read); D2_31_ohc: the OH-corrected model windows
 *  (NaN outside 1500–1550 and 2030–2060 nm); R_meas/A_meas/D2_meas: the scan as measured. */
export type ViewKey = 'R' | 'A' | 'D2' | 'D2_31' | 'D2_31_ohc' | 'R_meas' | 'A_meas' | 'D2_meas';

export interface ScanResult {
  scanId: string;
  file: string;
  path: string;
  /** ISO 8601 with offset; the export adds the raw OLE value. */
  acquiredAt: string;
  serial: number | null;
  instrumentClass: InstrumentClass;
  classSource: ClassSource;
  transfer: TransferInfo | null;
  analysis: Analysis;
  profileId: string;

  verdict: Verdict;
  verdictRule: VerdictRule;
  /** All notes that apply, any order; the UI applies the two-note precedence (notes.ts). */
  notes: Note[];
  /** The notes shown under the verdict (at most two), as the core chose them. Absent: notes.ts picks. */
  notesShown?: NoteKey[];
  /** Flag badges from the core ({key, signs}); the fired signs carry the same information. */
  flags?: { key: string; signs: SignId[] }[];
  /** The verdict from the collagen models alone (before flat bands, +D or a lift). */
  modelVerdict?: Verdict | null;
  /** The core's rule step verbatim (exports). */
  ruleStep?: string;
  /** Reason text key for Rescan / not-bone. */
  unusableReason?: 'low_signal' | 'saturated' | 'panel' | 'unsupported' | 'unreadable';
  /** The reader's reason, for an unreadable file. */
  unusableDetail?: string;

  models: Record<ModelKey, ModelReading | null>;
  evidence: Evidence;
  zooms: Zooms;
  signs: Sign[];
  checks: CheckResult[];
  /** Informational: the 1450/1930 OH/water bands differ from the reference bones. */
  alteredOhBand: boolean;

  engineVersion: string;
  /** The profile's "most promising first" key from the core (unrounded); absent in the mock. */
  sortGroup?: number | null;
  sortValue?: number | null;
  inputSha256?: string | null;
  /** Milliseconds the core took to analyse this scan. */
  scoreMs?: number;

  // Live folder watching (UI-side; not export fields).
  /** Delivery order within the current watch; "newest first" uses it while live. */
  arrivedSeq?: number;
  /** 1 for the first content seen under this file name; 2, 3, ... when the file changed on disk. */
  fileRevision?: number;
  /** 'reference': a white-reference save, listed but not scored (PLAN Step 1). 'unreadable': not an ASD file
   *  this app can read. 'unscored': no verdict model is available (startup error). Absent or 'sample': a scan. */
  scanKind?: 'sample' | 'reference' | 'unreadable' | 'unscored';
  /** Why a file is 'unreadable' (from the structure check). */
  kindDetail?: string;
}

export interface SessionInfo {
  folder: string;
  watching: boolean;
  instrumentClass: InstrumentClass;
  classSource: ClassSource;
  /** Serial seen in the folder's files, if any. */
  serial: number | null;
  /** The class the serial registry says, if known. */
  serialClass: InstrumentClass | null;
  example: boolean;
  /** Scans in the session. */
  count?: number;
}

export interface SpectrumViews {
  id: string;
  startNm: number;
  stepNm: number;
  n: number;
  smoothing: number;
  views: Record<ViewKey, Float32Array>;
}

export interface ReferenceSpectrum {
  id: string;
  /** Measured yield label: "0%", "1%", ... */
  label: string;
  /** Legend text (the measured-yield label, e.g. "1%"). */
  legend: string;
  meanYieldPct: number;
  n: number;
  spectra: SpectrumViews;
}

export interface AppInfo {
  name: string;
  version: string;
  os: string;
  arch: string;
  coreConnected: boolean;
  /** The startup-error state in plain words (shown as "No verdict model available: …"). */
  coreError?: string | null;
  pluginNotes?: string[];
  bundledPlugins?: string;
  userPlugins?: string | null;
  runtime: 'tauri' | 'browser';
}

/** One OH-corrected model window (the close-ups): whether the corrected view is available, and why not. */
export interface OhWindowInfo {
  label: '2045' | '1500' | string;
  modelId: string;
  loNm: number;
  hiNm: number;
  available: boolean;
  reason: string | null;
}

export interface DisplayInfo {
  viewOrder: ViewKey[];
  startNm: number;
  stepNm: number;
  n: number;
  ohWindows: OhWindowInfo[];
  unavailable: string | null;
}

export interface ExportResult {
  path: string;
  rows: number;
  /** The technical CSV written beside the readable one (absent when only the technical file was written). */
  technicalPath?: string | null;
}

export interface IpcSelfTest {
  ok: boolean;
  n: number;
  bytes: number;
  ms: number;
  path: 'tauri-binary' | 'mock';
}

// ---- live folder watching (PLAN v3.2 Phase 5; app/src-tauri/src/live.rs) ----

export type WatchState = 'watching' | 'paused' | 'folder_missing' | 'stopped';

export interface WatchStatus {
  folder: string;
  state: WatchState;
  /** 'poll' on network volumes (automatic) or when chosen; else native change events plus a rescan. */
  mode: 'native' | 'poll';
  volume: 'local' | 'network';
  volumeDetail: string;
  /** Why this mode, in plain words. */
  modeReason: string;
  /** Files seen but not settled yet. */
  pending: number;
  delivered: number;
  intervalMs: number;
  note: string | null;
  sessionFile: string | null;
}

/** A settled, complete file (the bytes stay in Rust as an immutable snapshot; fetch with snapshotBytes). */
export interface WatchArrival {
  seq: number;
  snapshotId: number;
  folder: string;
  path: string;
  file: string;
  size: number;
  modifiedMs: number | null;
  receivedMs: number;
  sha256: string;
  revision: number;
  existing: boolean;
  sameContentAs: string | null;
  recognised: boolean;
  kindHint: 'sample' | 'white_reference_save' | null;
  detail: string;
  /** The session scan this arrival became (desktop app: scored by spyder-core before the event). */
  scanId?: string | null;
  /** Milliseconds spyder-core took to analyse it. */
  scoreMs?: number | null;
}

export interface WatchIncomplete {
  path: string;
  file: string;
  reason: string;
}

export interface FolderSettings {
  path: string;
  instrumentClass: InstrumentClass | null;
  includeExisting: boolean | null;
  watchMode: 'auto' | 'native' | 'poll';
  lastUsedMs: number;
}

export interface WatchProbe {
  folder: string;
  exists: boolean;
  existingCount: number;
  volume: 'local' | 'network';
  volumeDetail: string;
  remembered: FolderSettings | null;
}
