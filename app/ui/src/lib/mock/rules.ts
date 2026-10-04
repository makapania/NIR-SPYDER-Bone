// MOCK ONLY. A compact re-statement of PLAN v3.2 Steps 7 and 9 (with DECISIONS 52–54) so the
// browser mock produces self-consistent ScanResults. spyder-core is the source of truth; nothing
// here ships as a rule.

import { BANDS, type BandDef } from '../bands';
import { idx } from './synth';
import type {
  BandId,
  BandReading,
  BandState,
  Evidence,
  EvidenceLevel,
  ModelKey,
  ModelReading,
  Note,
  Sign,
  Verdict,
  VerdictRule,
  Zooms,
  ZoomsPattern,
} from '../types';

const median = (a: number[]) => {
  const s = [...a].sort((x, y) => x - y);
  const n = s.length;
  return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2;
};

/** Step 7 on the SG31 E-curve. `sdE(nm)` is the noise SD of E at that wavelength. */
export function evidenceFrom(E: Float64Array, sdE: (nm: number) => number): Evidence {
  const bands: BandReading[] = BANDS.map((b) => {
    let s = 0;
    for (let l = b.nm - 2; l <= b.nm + 2; l++) s += E[idx(l)];
    const e = s / 5;
    const u = e / b.w;
    const sdU = sdE(b.nm) / Math.sqrt(5) / b.w;
    const readable = !(sdU > 0.25 && u < 1.5);
    const lit = readable && e > 0;
    const state: BandState = !readable ? 'cant_tell' : u >= 4 ? 'strong' : u >= 1.5 ? 'clear' : e > 0 ? 'trace' : 'flat';
    return { id: b.id, nm: b.nm, e, u, readable, lit, state };
  });
  const by = (id: BandId) => bands[BANDS.findIndex((b) => b.id === id)];
  const core = BANDS.map((b, i) => ({ b, r: bands[i] })).filter((x) => x.b.core);
  const readableCore = core.filter((x) => x.r.readable);
  const coreLit = (x: { b: BandDef; r: BandReading }) => x.r.readable && x.r.u > x.b.faint;
  const s = readableCore.length ? median(readableCore.map((x) => x.r.u)) : null;
  let guard = -Infinity;
  for (let l = 1675; l <= 1770; l++) guard = Math.max(guard, E[idx(l)]);
  const nh = by('nh2044');
  const am = by('amide2175');
  const allReadableFlat = readableCore.every((x) => !coreLit(x));
  const nhType = Math.max(am.readable ? am.u : -Infinity, nh.readable ? nh.u : -Infinity);
  let level: EvidenceLevel;
  if (
    allReadableFlat &&
    by('ch1728').readable &&
    am.readable &&
    readableCore.length >= 3 &&
    (!nh.readable || nh.e <= 0) &&
    guard < 1.5
  ) {
    level = 'none';
  } else if (core.every((x) => !coreLit(x)) && allReadableFlat && !(nh.readable && nh.u >= 1.5)) {
    level = 'cant_tell';
  } else if (s !== null && s >= 4 && readableCore.every(coreLit) && nhType >= 1.5) {
    level = 'strong';
  } else if (s !== null && s >= 1.5 && core.filter(coreLit).length >= 3 && nhType >= 1.5) {
    level = 'clear';
  } else if (readableCore.length === 0) {
    level = 'cant_tell';
  } else {
    level = 'trace';
  }
  return { level, s, bands, status: { status: 'assessed' } };
}

/** The A17 ZooMS band check (normative table in PLAN Step 9): pattern and call. `lit1545`: the OH-corrected 1545 nm
 *  band is lit (mock: given by the scan spec), which turns an Unlikely pattern into Borderline (the vote, DECISIONS 75). */
export function zoomsFrom(ev: Evidence, lit1545 = false): Zooms {
  // the pattern and the counts read the six named bands only; the 1545 band (if listed) is the vote, kept apart
  const b = BANDS.map((d) => ev.bands.find((x) => x.id === d.id)).filter((x): x is BandReading => x !== undefined);
  const P = [b[2], b[3]];
  const CH = [b[0], b[1], b[4], b[5]];
  const allR = b.every((x) => x.readable);
  let pattern: ZoomsPattern = null;
  if (P.every((x) => x.readable && !x.lit)) pattern = allR && b.every((x) => !x.lit) ? 'A' : 'B';
  else if (P.every((x) => x.readable)) {
    const lp = P.filter((x) => x.lit).length;
    if (lp === 1) pattern = 'C';
    else if (allR && b.every((x) => x.lit)) pattern = 'E';
    else if (CH.some((x) => x.readable && !x.lit)) pattern = 'D';
  }
  const call: Verdict =
    pattern === 'A' || pattern === 'B' ? 'unlikely' : pattern === 'C' || pattern === 'D' ? 'borderline' : pattern === 'E' ? 'good' : 'cant_tell';
  const vote1545 = call === 'unlikely' && lit1545;
  return {
    pattern,
    litCount: b.filter((x) => x.lit).length,
    readableCount: b.filter((x) => x.readable).length,
    verdict: vote1545 ? 'borderline' : call,
    vote1545,
  };
}

export interface VerdictInput {
  hires: boolean;
  models: Record<ModelKey, ModelReading | null>;
  evidence: Evidence;
  zooms: Zooms;
  signs: Sign[];
  b6bGated: boolean;
  c1Assessed: boolean;
}

const LIFT_BLOCKERS = new Set(['wax', 'ester', 'plaster', 'c1']);

/** Step 9 (rule L2, +D, flags per DECISIONS 53, notes per 52): the one verdict for every analysis, plus the ZooMS
 *  line where the ZooMS band check calls the scan better (DECISIONS 80 amended). */
export function decide(x: VerdictInput): { verdict: Verdict; rule: VerdictRule; notes: Note[] } {
  const notes: Note[] = [];
  const m = x.models.cons3?.value ?? 0;
  const fired = x.signs.filter((s) => s.fired);
  const blocker = fired.find((s) => LIFT_BLOCKERS.has(s.id));
  const clean = !blocker && !x.b6bGated && x.c1Assessed;
  const comps = [x.models.wc2045?.value, x.models.wc1500?.value, x.models.f05?.value].filter(
    (v): v is number => v != null,
  );
  // DECISIONS 65: only when the components fall in different verdict bands (straddle the 0.5 or 3 cut) and span >= 1 point
  const band = (v: number) => (v < 0.5 ? 0 : v < 3 ? 1 : 2);
  if (
    comps.length === 3 &&
    new Set(comps.map(band)).size > 1 &&
    Math.max(...comps) - Math.min(...comps) >= 1
  ) {
    notes.push({ key: 'models_disagree', params: { a: comps[0], b: comps[1], c: comps[2] } });
  }
  const s1 = x.models.s1r2?.value;
  if (x.hires && s1 != null && Math.abs(s1 - m) > 2.5) notes.push({ key: 'second_opinion_differs', params: { s: s1 } });

  let verdict: Verdict = m < 0.5 ? 'unlikely' : m < 3 ? 'borderline' : 'good';
  let rule: VerdictRule = 'model';
  const ev = x.evidence;
  const pat = x.zooms.pattern;
  if (ev.level === 'none') {
    verdict = 'unlikely';
    rule = 'flat_bands';
    notes.push({ key: 'flat_bands', params: { m } });
  } else if ((pat === 'A' || pat === 'B') && m >= 3) {
    verdict = 'borderline';
    rule = 'plus_d';
    notes.push({ key: 'plus_d', params: { m } });
  } else if (pat === 'E' && (ev.level === 'strong' || ev.level === 'clear')) {
    const rank = { unlikely: 0, borderline: 1, good: 2 } as Record<string, number>;
    const target: Verdict = ev.level === 'strong' ? 'good' : rank[verdict] < 1 ? 'borderline' : verdict;
    if (rank[target] > rank[verdict]) {
      if (clean) {
        verdict = target;
        rule = target === 'good' ? 'lift_good' : 'lift_borderline';
        notes.push({ key: rule === 'lift_good' ? 'lift_good' : 'lift_borderline', params: { m } });
      } else {
        notes.push({
          key: 'lift_blocked',
          params: blocker ? { strength: ev.level, sign: blocker.id } : { strength: ev.level, reason: 'noise' },
        });
      }
    }
  }
  // the ZooMS line: the band check ranks above the verdict (Good > Borderline > Unlikely; Can't tell never counts)
  const rank: Partial<Record<Verdict, number>> = { unlikely: 0, borderline: 1, good: 2 };
  const zs = x.zooms.verdict;
  // no line when a lift-blocking sign fired (wax, ester, plaster, C1 can create the C-H bands); burnt does not count
  if (!blocker && zs && rank[zs] != null && rank[verdict] != null && rank[zs]! > rank[verdict]!) {
    if (zs === 'good') notes.push({ key: 'zooms_better_good', params: {} });
    else if (x.zooms.vote1545) notes.push({ key: rule === 'flat_bands' ? 'zooms_better_1545_flat' : 'zooms_better_1545', params: {} });
    else {
      // the protein line needs m >= 0.3 and two N-type bands lit (2044, 2175, the 1545 band); else the faint sign
      const lit = ev.bands.filter((b) => ['nh2044', 'amide2175', 'nh1545'].includes(b.id) && b.lit && b.readable).map((b) => b.id);
      if (m >= 0.3 && lit.length >= 2) notes.push({ key: 'zooms_better_protein', params: {} });
      else notes.push({ key: 'zooms_faint_protein', params: { lit, m } });
    }
  }
  // the Ryder 2045 line: Unlikely with no band line (a faint sign may stand), the published model >= 0.34
  const ry = x.models.ryder2045?.value;
  const bandLine = notes.some((n) => n.key.startsWith('zooms_better'));
  if (!blocker && !bandLine && verdict === 'unlikely' && ry != null && ry >= 0.34) {
    const faint = notes.findIndex((n) => n.key === 'zooms_faint_protein');
    const line = { key: 'zooms_better_ryder' as const, params: { r: ry, at_least: 0.34 } };
    if (faint >= 0) notes.splice(faint, 0, line);
    else notes.push(line);
  }
  if (ev.level === 'cant_tell') notes.push({ key: 'bands_too_noisy', params: {} });
  const promising = verdict === 'good' || verdict === 'borderline';
  if (promising && (x.b6bGated || !x.c1Assessed)) notes.push({ key: 'contaminants_not_checked', params: {} });
  const lifted = rule === 'lift_good' || rule === 'lift_borderline';
  if (promising && clean && !lifted) {
    if (pat === 'E') notes.push({ key: 'positive_signs_all_six', params: {} });
    else if ((ev.level === 'clear' || ev.level === 'strong') && m >= 1) notes.push({ key: 'positive_signs_clear', params: {} });
  }
  return { verdict, rule, notes };
}
