// Scan ordering for the flat list. Uses unrounded values only (PLAN Step 9 "Sorting").
// "Most promising first": Good, Borderline, Can't tell, Unlikely, then Rescan and "Doesn't look like
// bone"; within Good, Borderline and Can't tell by CONS3; within Unlikely by the evidence score S.
// One sort for every analysis (DECISIONS 80 amended: the ZooMS sort went with the ZooMS verdict).

import { PROMISING_ORDER } from './verdict';
import type { ScanResult } from './types';

export type SortMode = 'newest' | 'promising';

const cons3 = (s: ScanResult) => s.models.cons3?.value ?? -Infinity;
const score = (s: ScanResult) => s.evidence.s ?? -Infinity;

/** Newest first. While a folder is watched, "newest" is arrival order, so a scan that just arrived is always on
 *  top (a file copied in keeps its old modification time); otherwise acquisition time. */
export function byNewest(a: ScanResult, b: ScanResult): number {
  if (a.arrivedSeq != null && b.arrivedSeq != null && a.arrivedSeq !== b.arrivedSeq) return b.arrivedSeq - a.arrivedSeq;
  return b.acquiredAt.localeCompare(a.acquiredAt) || a.file.localeCompare(b.file);
}

/** Listed but not scored (white-reference saves, unreadable files, no verdict model). */
export const isUnscored = (s: ScanResult) =>
  s.scanKind === 'reference' || s.scanKind === 'unreadable' || s.scanKind === 'unscored';

/** The core's own "most promising first" key (desktop app): verdict group ascending, then the profile's value
 *  descending (unrounded; a missing value sorts last within its group). */
function byCoreKey(a: ScanResult, b: ScanResult): number {
  const g = (a.sortGroup ?? Infinity) - (b.sortGroup ?? Infinity);
  if (g) return g;
  const va = a.sortValue ?? -Infinity;
  const vb = b.sortValue ?? -Infinity;
  return va === vb ? 0 : vb > va ? 1 : -1;
}

export function byPromising(a: ScanResult, b: ScanResult): number {
  const u = Number(isUnscored(a)) - Number(isUnscored(b));
  if (u) return u;
  if (a.sortGroup != null && b.sortGroup != null) return byCoreKey(a, b) || byNewest(a, b);
  const g = PROMISING_ORDER[a.verdict] - PROMISING_ORDER[b.verdict];
  if (g) return g;
  const key = a.verdict === 'unlikely' ? score : cons3;
  const d = key(b) - key(a);
  if (d && Number.isFinite(d)) return d;
  if (key(a) !== key(b)) return key(a) === -Infinity ? 1 : -1;
  return byNewest(a, b);
}

/** Scans with at least a trace of protein signal ("Any organic signal" filter). */
export function hasOrganicSignal(s: ScanResult): boolean {
  return s.evidence.level === 'trace' || s.evidence.level === 'clear' || s.evidence.level === 'strong';
}

export function orderScans(scans: ScanResult[], mode: SortMode, organicOnly = false): ScanResult[] {
  const list = organicOnly ? scans.filter(hasOrganicSignal) : [...scans];
  return list.sort(mode === 'promising' ? byPromising : byNewest);
}

/** 1-based rank in "most promising first" order over the whole folder (filters ignored). */
export function promisingRanks(scans: ScanResult[]): Map<string, number> {
  const m = new Map<string, number>();
  orderScans(scans, 'promising').forEach((s, i) => m.set(s.scanId, i + 1));
  return m;
}
