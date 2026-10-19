// The compact verdict card (Matt, 2026-10-18: "room for three banners or more", the explanatory text in tooltips or
// expanders): every flag, strong warning and card note as one item with a short label and its full text. Which notes are
// on the card is unchanged (pickNotes: the core's choice, every fired warning); this only shapes how they are drawn.

import { isFaint, isPositive, isZoomsLine, noteIcon, pickNotes, type NoteIcon } from './notes';
import { NOTE_SHORT, flagLabel, flagText, flatBandsCard, noteParts } from './strings';
import type { NoteKey, ScanResult } from './types';

export type ItemTone = 'flag' | 'warn' | 'zooms' | 'positive' | 'faint' | 'note';

export interface CardItem {
  /** Unique on the card: `flag:<sign>` or the note key. */
  id: string;
  /** Flags and strong warnings ('alert') rank above the quieter notes ('note'). */
  tier: 'alert' | 'note';
  tone: ItemTone;
  icon: NoteIcon | 'flag';
  /** The chip's short label. */
  label: string;
  /** The full text, shown on hover / focus and when expanded. It never repeats the label as its opening words. */
  body: string;
  /** The note key (notes and warnings) for data-note; flags have none. */
  note?: NoteKey;
}

const cap = (s: string) => s.replace(/^[.:;,]\s*/, '').replace(/^./, (c) => c.toUpperCase());

/** The card's items in reading order: flags, then the strong warnings (C–H "likely" first), then the card notes in the
 *  core's order. Nothing is dropped: every fired flag and warning and every note pickNotes puts on the card is here once. */
export function cardItems(scan: ScanResult): { alerts: CardItem[]; notes: CardItem[] } {
  const picked = pickNotes(scan.notes, scan.notesShown);
  const flags: CardItem[] = scan.signs
    .filter((s) => s.fired)
    .map((s) => ({ id: `flag:${s.id}`, tier: 'alert', tone: 'flag', icon: 'flag', label: flagLabel(s.id), body: flagText(s.id, s.heatKind) }));
  const warnings: CardItem[] = picked.warnings.map((n) => {
    const p = noteParts(n, scan);
    return { id: n.key, tier: 'alert', tone: 'warn', icon: noteIcon(n.key), label: p.lead || NOTE_SHORT[n.key], body: cap(p.rest), note: n.key };
  });
  const notes: CardItem[] = picked.headline.map((n) => {
    const p = n.key === 'flat_bands' && scan.verdictRule === 'flat_bands' ? { lead: '', rest: flatBandsCard(n.params) } : noteParts(n, scan);
    const label = NOTE_SHORT[n.key];
    const full = p.lead ? cap(p.rest) : p.rest;
    // a full text that opens with its own label (e.g. "Possible thin coating: ...") reads on from it
    const body = !p.lead && full.startsWith(label) && /^[.:;,]/.test(full.slice(label.length)) ? cap(full.slice(label.length)) : full;
    const tone: ItemTone = isZoomsLine(n.key) ? 'zooms' : isPositive(n.key) ? 'positive' : isFaint(n.key) ? 'faint' : 'note';
    return { id: n.key, tier: 'note', tone, icon: noteIcon(n.key), label, body, note: n.key };
  });
  return { alerts: [...flags, ...warnings], notes };
}
