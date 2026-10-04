// The details column's words: each model row's annotations and the Checks list. Pure functions of the core's
// result (no numbers computed here; nothing here changes a verdict), so vitest can hold them.

import { UNUSABLE_TEXT, flagLabel } from './strings';
import type { Assessment, ModelReading, ScanResult } from './types';

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
      text: `Noise widens this reading’s error${r.impliedSd != null && Number.isFinite(r.impliedSd) ? ` (noise SD ≈ ${r.impliedSd.toFixed(1)} points)` : ''}.`,
    });
  return out;
}

export interface CheckRow {
  icon: 'ok' | 'info' | 'noise' | 'flag';
  title: string;
  d: string;
  /** True: shown in the strong style (something to notice); false: a quiet "all fine" line. */
  notice: boolean;
}

const CHECK_NAME: Record<string, string> = { wax: 'wax', ester: 'consolidant (ester)', plaster: 'plaster', c1: 'other foreign organic' };

/** "a, b and c" for sign ids. */
function listNames(ids: string[]): string {
  const n = ids.map((id) => CHECK_NAME[id] ?? id);
  return n.length <= 1 ? n.join('') : `${n.slice(0, -1).join(', ')} and ${n[n.length - 1]}`;
}

/** The Checks list. B6 (noise widens the reading's error) and B6b (too noisy for the contaminant signs) are read
 *  independently: either one replaces "Good signal". */
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
    if (b6bNote)
      out.push({
        icon: 'noise',
        title: 'Noisy scan',
        d: 'A dark or noisy spot. Consider 100–200 averages on high-res.',
        notice: true,
      });
    if (b6Check)
      out.push({
        icon: 'noise',
        title: 'Noise widens the reading’s error',
        d: `The scan’s noise makes the collagen reading less certain${b6?.value != null && Number.isFinite(b6.value) ? ` (noise SD ≈ ${b6.value.toFixed(1)} points)` : ''}. More averages help.`,
        notice: true,
      });
    if (!b6bNote && !b6Check) out.push({ icon: 'ok', title: 'Scan quality', d: 'Good signal, no saturation.', notice: false });
  }
  const fired = scan.signs.filter((s) => s.fired);
  // Each contaminant sign is gated on the noise in its own part of the spectrum (DECISIONS 76). C1 "skipped" (a
  // specific sign already fired) counts as checked; gated or not assessed does not.
  const contaminant = scan.signs.filter((s) => s.id !== 'burnt');
  const notChecked = contaminant.filter((s) => s.status.status === 'gated' || s.status.status === 'not_assessed');
  const checked = contaminant.filter((s) => !notChecked.includes(s));
  const heatSign = scan.signs.find((s) => s.id === 'burnt');
  const heatUnchecked = !!heatSign && (heatSign.status.status === 'gated' || heatSign.status.status === 'not_assessed');
  for (const f of fired) out.push({ icon: 'flag', title: flagLabel(f.id), d: 'Shown beside the verdict; it never changes the verdict.', notice: true });
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
        ? { icon: 'ok', title: 'No contaminant signs', d: 'Nothing resembling wax, consolidants, plaster or foreign organics.', notice: false }
        : { icon: 'ok', title: 'No contaminant or heat signs', d: 'Nothing resembling wax, consolidants, plaster, foreign organics or charring.', notice: false },
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
    const c1Ran = c1?.status.status === 'assessed';
    const c1Reduced = c1?.status.status === 'assessed' && !!c1.status.note;
    out.push({
      icon: 'info',
      title: 'Long-wave region not used',
      d: c1Reduced
        ? 'Above 2300 nm is too noisy on this high-res scan, so the bone check and the general organic check run without it. In this reduced form the organic check can miss some glues and lacquers (for example cellulose-nitrate glues and animal glue).'
        : c1Ran
          ? 'Above 2300 nm is too noisy on this high-res scan for the bone check, which runs without it. The general organic check still uses it.'
          : 'Above 2300 nm is too noisy on this high-res scan for the bone check, which runs without it.',
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
    });
  if (!rescan)
    out.push({
      icon: 'info',
      title: 'Glues and thin coats',
      d: "Thin coats of glue or lacquer can raise every collagen reading by 1–4 points without tripping a sign, and animal (hide) glue cannot be detected at all: it looks like the bone's own collagen. On a bone that may have been glued or coated, check its history and treat a Borderline reading with extra caution.",
      notice: false,
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
