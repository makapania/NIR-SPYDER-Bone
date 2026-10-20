// The details column's words: each model row's annotations and the Checks list. Pure functions of the core's
// result (no numbers computed here; nothing here changes a verdict), so vitest can hold them.

import { isLabSpec } from './instrument';
import { PROVISIONAL, UNUSABLE_TEXT, shownFlag } from './strings';
import type { Assessment, ModelReading, PanelContrast, ScanResult, VerdictRule } from './types';

export type AnnotationKind = 'assessment' | 'domain' | 'noise';
export interface ModelAnnotation {
  kind: AnnotationKind;
  text: string;
}

/** The grey "less familiar spectrum" note (PLAN §1 item 4; Step 6: beyond 3x the 99% T²/Q limits). Grey text,
 *  never a colour. */
export const DOMAIN_NOTE = 'Less familiar spectrum: outside the range of the scans this model was built on. Read it with more caution.';

const cap = (t: string) => (t ? t[0].toUpperCase() + t.slice(1) : t);

/** An assessment in words, or null when it is plainly assessed. */
export function assessmentText(a: Assessment): string | null {
  switch (a.status) {
    case 'assessed':
      return a.note ? cap(a.note) : null;
    case 'not_assessed':
      return `Not assessed: ${a.reason}`;
    case 'gated':
      return `Not assessed: ${a.reason}`;
    case 'skipped':
      return `Skipped: ${a.reason}`;
  }
}

/** Everything a model row must say beside its number: its assessment (why it is missing, or a qualifier), the
 *  grey domain note, and the B6 noise annotation. */
export function modelAnnotations(r: ModelReading): ModelAnnotation[] {
  const out: ModelAnnotation[] = [];
  const a = assessmentText(r.status);
  if (a) out.push({ kind: 'assessment', text: a });
  if (r.domainNote) out.push({ kind: 'domain', text: DOMAIN_NOTE });
  if (r.noiseCheck)
    out.push({
      kind: 'noise',
      text: `Noise could carry this reading across a verdict cut${r.impliedSd != null && Number.isFinite(r.impliedSd) ? ` (noise SD ≈ ${r.impliedSd.toFixed(1)} points)` : ''}.`,
    });
  return out;
}

export interface CheckRow {
  icon: 'ok' | 'info' | 'noise' | 'flag';
  title: string;
  d: string;
  /** True: shown in the strong style (something to notice); false: a quiet "all fine" line. */
  notice: boolean;
  /** A standing caveat, the same on every scan (shown behind an expander; DECISIONS 125). */
  standing?: boolean;
}

const CHECK_NAME: Record<string, string> = { wax: 'lipid (such as wax)', ester: 'consolidant (ester)', plaster: 'plaster', c1: 'other foreign organic' };

/** "a, b and c" for sign ids. */
function listNames(ids: string[]): string {
  const n = ids.map((id) => CHECK_NAME[id] ?? id);
  return n.length <= 1 ? n.join('') : `${n.slice(0, -1).join(', ')} and ${n[n.length - 1]}`;
}

/** Above this implied SD (% collagen) a reading far from any cut still gets a quiet "some noise" line instead of
 *  "Good signal" (the old B6 threshold; display only since DECISIONS 109). */
const NOISE_SD_QUIET = 0.5;

/** The Checks list. B6 (noise could change the verdict: a cut within noise reach, DECISIONS 109) and B6b (too noisy
 *  for the contaminant signs) are read independently: either one replaces "Good signal". */
export function checkRows(scan: ScanResult): CheckRow[] {
  const out: CheckRow[] = [];
  const find = (id: string) => scan.checks.find((c) => c.id === id);
  const b6 = find('b6');
  const b6b = find('b6b');
  const lw = find('longwave');
  const b9 = find('b9');
  const rescan = scan.verdict === 'rescan';
  if (rescan) out.push({ icon: 'noise', title: 'Scan quality', d: UNUSABLE_TEXT[scan.unusableReason ?? 'low_signal'], notice: true });
  else {
    const b6bNote = b6b?.outcome === 'note';
    const b6Check = b6?.outcome === 'check';
    // B6 not assessed (a correction the engine cannot propagate noise through, not available on this instrument, ...):
    // the noise is unknown, so this row never says "Good signal" (DECISIONS 137)
    const b6NotAssessed = !!b6 && b6.status.status !== 'assessed';
    const c3 = scan.models?.cons3?.value;
    const hasCons3 = c3 != null && Number.isFinite(c3);
    const sdText = b6?.value != null && Number.isFinite(b6.value) ? ` (noise SD ≈ ${b6.value.toFixed(1)} points)` : '';
    if (b6bNote)
      out.push({
        icon: 'noise',
        title: 'Noisy scan',
        d: 'A dark or noisy spot. Consider 100–200 averages.',
        notice: true,
      });
    if (b6Check)
      out.push({
        icon: 'noise',
        title: 'Noise could change the verdict',
        d: `The collagen reading sits within noise reach of a verdict cut${sdText}. A rescan with more averages could change the verdict.`,
        notice: true,
      });
    else if (b6NotAssessed && !b6bNote)
      out.push({
        icon: 'info',
        title: 'Scan quality',
        // the core's reason without its technical detail in brackets ("not available on this instrument (band ...)")
        d: `Noise not assessed${b6.status.status === 'not_assessed' && b6.status.reason ? `: ${b6.status.reason.split(' (')[0]}` : ''}.`,
        notice: false,
      });
    // only about a reading that exists (a not-assessed CONS3 can still carry an implied SD)
    else if (!b6bNote && hasCons3 && b6?.value != null && Number.isFinite(b6.value) && b6.value > NOISE_SD_QUIET)
      out.push({ icon: 'info', title: 'Scan quality', d: `Some noise in the reading${sdText}, not enough to change the verdict.`, notice: false });
    else if (!b6bNote) out.push({ icon: 'ok', title: 'Scan quality', d: 'Good signal, no saturation.', notice: false });
  }
  const fired = scan.signs.filter((s) => s.fired);
  // Each contaminant sign is gated on the noise in its own part of the spectrum (DECISIONS 76). C1 "skipped" (a
  // specific sign already fired) counts as checked; gated or not assessed does not.
  const contaminant = scan.signs.filter((s) => s.id !== 'burnt');
  const notChecked = contaminant.filter((s) => s.status.status === 'gated' || s.status.status === 'not_assessed');
  const checked = contaminant.filter((s) => !notChecked.includes(s));
  const heatSign = scan.signs.find((s) => s.id === 'burnt');
  const heatUnchecked = !!heatSign && (heatSign.status.status === 'gated' || heatSign.status.status === 'not_assessed');
  // an instrument with no built-in correction: its contaminant and heat signs are provisional (DECISIONS 137)
  const prov = scan.uncalibrated ? PROVISIONAL : '';
  for (const f of fired)
    out.push({
      icon: 'flag',
      title: shownFlag(scan, f.id).label,
      d: `${prov}Shown beside the verdict; it never changes the verdict.`,
      notice: true,
    });
  if (notChecked.length && !rescan)
    out.push({
      icon: 'info',
      title: notChecked.length === contaminant.length ? 'Contaminant signs not checked' : 'Some contaminant signs not checked',
      d: `Not checked: ${listNames(notChecked.map((s) => s.id))}. The scan is too noisy in that part of the spectrum; a rescan with 100–200 averages usually fixes this.${checked.length ? ` Checked: ${listNames(checked.map((s) => s.id))}.` : ''}`,
      notice: true,
    });
  else if (!fired.length && !rescan)
    out.push(
      heatUnchecked
        ? { icon: 'ok', title: 'No contaminant signs', d: `${prov}No sign of lipids (such as wax), consolidants, plaster or foreign organics. Thin coats, animal glue and some consolidants can still go unseen.`, notice: false }
        : { icon: 'ok', title: 'No contaminant or heat signs', d: `${prov}No sign of lipids (such as wax), consolidants, plaster, foreign organics or charring. Thin coats, animal glue and some consolidants can still go unseen.`, notice: false },
    );
  if (heatUnchecked && !rescan)
    out.push({ icon: 'info', title: 'Heat sign not checked', d: 'The heat check could not read this scan, so charring or calcining was not checked.', notice: true });
  if (b9?.status.status === 'assessed' && !rescan)
    out.push(
      scan.verdict === 'not_bone'
        ? { icon: 'info', title: 'Does not look like bone', d: `Similarity to bone ${b9.value?.toFixed(2)} (bone scans sit above 0.40).`, notice: true }
        : {
            icon: 'ok',
            title: 'Looks like bone',
            d: `Similarity to bone ${b9.value?.toFixed(2)}${b9.status.note ? ' (shorter check: the long-wave region is too noisy on this scan)' : ''}.`,
            notice: false,
          },
    );
  if (lw?.outcome === 'note') {
    // B9 drops the region above 2300 nm from N2 150; the foreign-organic check (C1) only from N2 250 (DECISIONS 76)
    const c1 = scan.signs.find((s) => s.id === 'c1');
    // a C1 qualifier is either the >2300 nm truncation or the reduced fallback below 1800 nm (signs_reduced note)
    const c1Ran = c1?.status.status === 'assessed' && !c1.status.note;
    const c1Reduced = c1?.status.status === 'assessed' && !!c1.status.note?.startsWith('truncated');
    out.push({
      icon: 'info',
      title: 'Long-wave region not used',
      d: c1Reduced
        ? 'Above 2300 nm is too noisy on this scan, so the bone check and the general organic check run without it. In this reduced form the organic check can miss some glues and lacquers (for example cellulose-nitrate glues and animal glue).'
        : c1Ran
          ? 'Above 2300 nm is too noisy on this scan for the bone check, which runs without it. The general organic check still uses it.'
          : 'Above 2300 nm is too noisy on this scan for the bone check, which runs without it.',
      notice: true,
    });
  }
  const b8 = find('b8');
  if (b8 && b8.outcome !== 'ok')
    out.push({ icon: 'info', title: 'Detector join step', d: 'A visible step where two detectors join. The readings do not use the joins.', notice: true });
  const b7 = find('b7');
  if (b7?.outcome === 'note')
    out.push({ icon: 'info', title: 'Brighter than the white reference', d: 'Reflectance above 1 in places; the estimates are unaffected.', notice: true });
  const b5 = find('b5');
  if (b5?.outcome === 'note') out.push({ icon: 'info', title: 'Very dark spot', d: 'A dark spot reads noisier; more averages help.', notice: true });
  if (scan.instrumentClass === 'hires' && !rescan)
    out.push({
      icon: 'info',
      title: 'Contaminant checks on high-res',
      d: "The checks were developed on standard-resolution scans. On high-resolution instruments they have been tested only by adding known contaminant spectra to real high-resolution scans. Thick coatings and plaster were caught on most scans; thin coats of glue or lacquer are often missed on both kinds of instrument. Treat 'no sign found' as reassuring, not as proof.",
      notice: false,
      standing: true,
    });
  else if (!isLabSpec(scan.instrumentClass) && !rescan)
    out.push({
      icon: 'info',
      title: 'Contaminant checks on this instrument',
      d: "The checks were developed on LabSpec 4 standard-resolution scans and have not been tested on this instrument. Treat 'no sign found' as reassuring at most, not as proof.",
      notice: false,
      standing: true,
    });
  if (!rescan)
    out.push({
      icon: 'info',
      title: 'Glues and thin coats',
      d: "Thin coats of glue or lacquer can raise every collagen reading by 1–4 points, and the foreign-organic sign catches only about half of them. Animal (hide) glue reads as the bone's own collagen: about half of glued scans trip the foreign-organic sign, but it is never named as glue. On a bone that may have been glued or coated, check its history and treat a Borderline reading with extra caution.",
      notice: false,
      standing: true,
    });
  if (scan.alteredOhBand)
    out.push({
      icon: 'info',
      title: 'Altered OH/water band',
      d: 'The 1450 and 1930 nm bands differ from the reference bones. The OH-corrected models allow for this.',
      notice: true,
    });
  return out;
}

/** Plain names of the reader comparisons (DECISIONS 119). */
export const CONTRAST_NAME: Record<string, string> = {
  CH: 'C–H above the N–H consensus',
  CHN: 'C–H below the N–H consensus',
  GL: 'N–H consensus above the narrow 1545 nm band',
  SHP: '1545 nm model above the narrow 1545 nm band',
  SP3: 'Spread of the three consensus models',
  RAW: 'Consensus against Ryder 2045 (uncorrected)',
};

export interface NumberRow {
  title: string;
  /** The row in one sentence (the readable form). */
  d: string;
  /** The same as labelled values, one per line, for the details panel to lay out (DECISIONS 125). */
  items?: { label: string; value: string }[];
  /** What the values are measured in, or a qualifier, under them. */
  note?: string;
}

const sgn = (v: number) => `${v >= 0 ? '+' : '−'}${Math.abs(v).toFixed(1)}`;

/** The rule that set this scan's verdict, in plain words (the first row of "How this verdict was made"; DECISIONS 123). */
export const RULE_PLAIN: Partial<Record<VerdictRule, string>> = {
  model: 'The consensus reading against the screening lines (Unlikely below 0.5%, Good from 3%); the bands did not change it.',
  flat_bands: 'The protein bands are flat: Unlikely, whatever the models read.',
  plus_d: 'Lowered to Borderline: the models read 3% or more, but the protein bands at 2044 and 2175 nm are flat.',
  lift_good: 'Raised to Good: all six collagen bands clearly resolved and no contaminant sign fired.',
  lift_borderline: 'Raised to Borderline: the collagen bands resolved and no contaminant sign fired.',
  lift_blocked: 'The bands could have raised the verdict, but a contaminant sign or noise blocked it; the consensus reading stands.',
  no_model_reading: "No collagen model could read this scan: Can't tell.",
  unusable: 'The scan is unusable: Rescan.',
  not_bone: 'The target does not look like bone.',
};

/** "How this verdict was made": the rule applied, then the scan's own numbers as labelled values (DECISIONS 121, 123).
 *  Pure: reads the core's readings and panel record; rows whose readings are missing are left out. */
export function verdictNumbers(scan: ScanResult): NumberRow[] {
  const out: NumberRow[] = [];
  const rule = RULE_PLAIN[scan.verdictRule];
  if (rule) out.push({ title: 'Rule applied', d: rule });
  const m = scan.models;
  const parts = [m.wc2045?.value, m.wc1500?.value, m.f05?.value];
  if (parts.every((v) => v != null && Number.isFinite(v))) {
    const v = parts as number[];
    const mid = [...v].sort((a, b) => a - b)[1];
    const spread = (Math.max(...v) - Math.min(...v)).toFixed(1);
    out.push({
      title: 'Consensus models',
      d: `2045 nm ${v[0].toFixed(1)}% · 1545 nm ${v[1].toFixed(1)}% · N–H set ${v[2].toFixed(1)}%. Middle value ${mid.toFixed(1)}% (the starting estimate) · spread ${spread} percentage points.`,
      items: [
        { label: '2045 nm', value: `${v[0].toFixed(1)}%` },
        { label: '1545 nm', value: `${v[1].toFixed(1)}%` },
        { label: 'N–H set', value: `${v[2].toFixed(1)}%` },
        { label: 'Middle value (starting estimate)', value: `${mid.toFixed(1)}%` },
        { label: 'Spread', value: `${spread} points` },
      ],
    });
  }
  const p = scan.panel;
  if (p?.chGap != null && Number.isFinite(p.chGap) && p.chPossibleAbove != null && p.chStrongAbove != null)
    out.push({
      title: 'C–H against N–H',
      d: `Observed gap ${sgn(p.chGap)} · "possible" note above ${sgn(p.chPossibleAbove)} · "likely" note above ${sgn(p.chStrongAbove)} (C–H reader minus the consensus, percentage points; lines for this reading).`,
      items: [
        { label: 'Observed gap', value: sgn(p.chGap) },
        { label: '"Possible" note above', value: sgn(p.chPossibleAbove) },
        { label: '"Likely" note above', value: sgn(p.chStrongAbove) },
      ],
      note: 'C–H reader minus the consensus, in percentage points; the lines are for this reading.',
    });
  const others = (p?.contrasts ?? []).filter((c) => c.name !== 'CH' && Number.isFinite(c.fraction));
  if (others.length) {
    // the pair closest to its own firing line (largest fraction), not the largest raw difference (Astra review)
    const c = others.reduce((a: PanelContrast, b: PanelContrast) => (b.fraction > a.fraction ? b : a));
    out.push({
      title: 'Most unusual reader difference',
      d: `${CONTRAST_NAME[c.name] ?? c.name}: observed ${c.value.toFixed(1)} · note fires above ${c.firesAbove.toFixed(1)} (percentage points)${c.fraction <= 0 ? '; no further apart than clean bone usually is' : ''}.`,
      items: [
        { label: 'Observed', value: c.value.toFixed(1) },
        { label: 'Note fires above', value: c.firesAbove.toFixed(1) },
      ],
      note: `${CONTRAST_NAME[c.name] ?? c.name}, in percentage points${c.fraction <= 0 ? '; no further apart than clean bone usually is' : ''}.`,
    });
  }
  const b6 = scan.checks.find((c) => c.id === 'b6');
  if (b6?.value != null && Number.isFinite(b6.value))
    out.push({
      title: 'Noise',
      d: `SD ≈ ${b6.value.toFixed(1)} percentage points in the consensus reading.`,
      items: [{ label: 'SD of the consensus reading', value: `≈ ${b6.value.toFixed(1)} points` }],
    });
  return out;
}
