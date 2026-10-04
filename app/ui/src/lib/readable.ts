// The readable export: one row per listed file, in the words the screen uses (same functions as the verdict card
// and the details panel), so the file and the app never disagree. The technical CSV (the core's oracle-identical
// rows, every value unrounded) is written beside it by the Rust side.
import { checkRows, assessmentText } from './details';
import { LEVEL_WORD, MODEL_NAME, formatModel, formatReading, zoomsHeadline } from './display';
import { isZoomsLine, pickNotes } from './notes';
import { isUnscored } from './sort';
import { NOTE_TEXT, NOT_BONE_TEXT, UNUSABLE_TEXT, flagLabel, flagText, noteText } from './strings';
import { verdictMeta, verdictSentence } from './verdict';
import type { ModelKey, ScanResult } from './types';

export const CITATION =
  'Ryder, C. et al. 2026. Refining near-infrared spectroscopy for collagen quantification in archaeological bone. Journal of Archaeological Science 185:106448. doi:10.1016/j.jas.2025.106448';

const MODEL_COLS: ModelKey[] = ['wc2045', 'wc1500', 'f05', 'ryder2045', 's1r2'];

function cell(v: string): string {
  return /[",\r\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v;
}
const line = (cells: string[]) => cells.map(cell).join(',');
/** Sentences of a text (split after . ! ? followed by a space). */
// (no lookbehind: macOS 13's first Safari 16 releases cannot parse it)
const sentences = (t: string) =>
  t
    .replace(/([.!?])\s+/g, '$1\u0000')
    .split('\u0000')
    .map((x) => x.trim())
    .filter(Boolean);
const join = (xs: (string | null | undefined)[]) => xs.filter((x): x is string => !!x && x.trim() !== '').join(' ');

/** "2026-10-03T14:27:46+02:00" -> "2026-10-03 14:27:46" (the instrument's local time, as saved). */
function scanned(iso: string): string {
  const m = /^(\d{4}-\d\d-\d\d)T(\d\d:\d\d:\d\d)/.exec(iso);
  return m ? `${m[1]} ${m[2]}` : iso;
}

function instrument(s: ScanResult): string {
  const cls =
    s.instrumentClass === 'hires'
      ? `High-res, transferred to standard resolution${s.transfer ? ` (v${s.transfer.version}, provisional)` : ''}`
      : 'Standard';
  const src =
    s.classSource === 'user'
      ? 'set by you'
      : s.classSource === 'default'
        ? 'default setting'
        : s.classSource === 'header_preset'
          ? 'preset from the detector settings'
          : 'preset from the serial';
  return `${cls}; ${src}`;
}

/** The sentence(s) under the verdict, exactly as the verdict card builds them. */
function why(s: ScanResult, headline: { key: string }[]): string {
  if (s.verdict === 'rescan') return join([UNUSABLE_TEXT[s.unusableReason ?? 'low_signal'], s.unusableDetail]);
  if (s.verdict === 'not_bone') return NOT_BONE_TEXT;
  const ruleNote = headline.some((n) => ['plus_d', 'flat_bands', 'lift_good', 'lift_borderline'].includes(n.key));
  const sentence = ruleNote
    ? ''
    : s.verdictRule === 'no_model_reading'
      ? NOTE_TEXT.no_model_reading({})
      : verdictSentence(s.verdict);
  const notes = pickNotes(s.notes, s.notesShown).headline.filter((n) => !isZoomsLine(n.key));
  return join([sentence, ...notes.map(noteText)]);
}

function model(s: ScanResult, k: ModelKey): string {
  const r = s.models[k];
  if (!r) return '';
  const extra = [r.domainNote ? 'less familiar spectrum' : null, assessmentText(r.status)].filter(Boolean);
  return `${formatModel(r.value)}${extra.length ? ` (${extra.join('; ')})` : ''}`;
}

export interface ReadableMeta {
  version: string;
  /** When the export was made, already formatted for people. */
  exportedAt: string;
}

/** The readable CSV text: UTF-8 with a byte-order mark (Excel), CRLF, a short preamble, then one row per file. */
export function readableCsv(scans: ScanResult[], meta: ReadableMeta): string {
  const models = MODEL_COLS.filter((k) => scans.some((s) => !isUnscored(s) && s.models[k]));
  const header = [
    'File',
    'Scanned',
    'Verdict',
    'Collagen (consensus of three)',
    'Why',
    'ZooMS',
    'Flags',
    'Also noted',
    'Protein bands',
    'ZooMS band check',
    ...models.map((k) => MODEL_NAME[k]),
    'Instrument',
    'Serial',
  ];
  const rows = scans.map((s) => {
    if (isUnscored(s)) {
      const word =
        s.scanKind === 'reference' ? 'White reference (not scored)' : s.scanKind === 'unscored' ? 'Not scored' : 'Not readable';
      return [s.file, scanned(s.acquiredAt), word, '', s.kindDetail ?? '', '', '', '', '', '', ...models.map(() => ''), '', ''];
    }
    const picked = pickNotes(s.notes, s.notesShown);
    const zooms = s.notes.find((n) => isZoomsLine(n.key));
    const fired = s.signs.filter((x) => x.fired);
    const shownReading = s.verdict === 'rescan' || s.verdict === 'not_bone' ? '' : formatReading(s.models.cons3?.value ?? null);
    const whyText = why(s, picked.headline);
    // The app shows some facts twice (under the verdict and in the details checks); a spreadsheet row says each once.
    const said = new Set(sentences(whyText));
    const also: string[] = [];
    const add = (title: string | null, d: string) => {
      const fresh = sentences(d).filter((x) => !said.has(x));
      fresh.forEach((x) => said.add(x));
      if (fresh.length) also.push(`${title ? `${title}: ` : ''}${fresh.join(' ')}`);
    };
    picked.details.filter((n) => !isZoomsLine(n.key)).forEach((n) => add(null, noteText(n)));
    // the contaminants-not-checked note already says what the matching check row says, in other words
    const hasNotChecked = s.notes.some((n) => n.key === 'contaminants_not_checked');
    checkRows(s)
      .filter((r) => r.notice && r.icon !== 'flag')
      .filter((r) => !(hasNotChecked && /contaminant signs not checked/i.test(r.title)))
      .forEach((r) => add(r.title, r.d));
    return [
      s.file,
      scanned(s.acquiredAt),
      verdictMeta(s.verdict).word,
      shownReading,
      whyText,
      zooms ? noteText(zooms) : '',
      fired.map((x) => `${flagLabel(x.id)}: ${flagText(x.id, x.heatKind)}`).join(' '),
      also.join(' '),
      LEVEL_WORD[s.evidence.level],
      zoomsHeadline(s.zooms, s.evidence.bands),
      ...models.map((k) => model(s, k)),
      instrument(s),
      s.serial == null ? '' : String(s.serial),
    ];
  });
  const preamble = [
    [`SPYDER Bone ${meta.version}: ${scans.length} file${scans.length === 1 ? '' : 's'}, exported ${meta.exportedAt}`],
    [`Please cite: ${CITATION}`],
    ['One row per scan. Results are per scan, never per bone. The "(technical).csv" file saved beside this one holds every value unrounded.'],
    [],
  ];
  return '\uFEFF' + [...preamble, header, ...rows].map(line).join('\r\n') + '\r\n';
}
