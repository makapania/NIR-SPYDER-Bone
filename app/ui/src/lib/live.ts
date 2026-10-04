// Live folder watching, UI side: pure helpers (no IPC, no DOM), so they are unit-tested. The watching itself is
// Rust (app/src-tauri/watch); api.ts forwards its events.

import type { WatchArrival, WatchIncomplete, WatchStatus } from './types';

/** ISO 8601 local time with offset, e.g. "2026-10-03T14:27:46-06:00" (what the export uses). */
export function isoLocal(ms: number, offsetMin = -new Date(ms).getTimezoneOffset()): string {
  const d = new Date(ms + offsetMin * 60_000);
  const p = (n: number, w = 2) => String(Math.abs(n)).padStart(w, '0');
  const sign = offsetMin < 0 ? '-' : '+';
  return (
    `${p(d.getUTCFullYear(), 4)}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}` +
    `T${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}` +
    `${sign}${p(Math.trunc(Math.abs(offsetMin) / 60))}:${p(Math.abs(offsetMin) % 60)}`
  );
}

/** Last component of a Windows or POSIX path. */
export function folderName(path: string): string {
  const parts = path.replace(/[\\/]+$/, '').split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export type WatchTone = 'live' | 'paused' | 'missing' | 'idle';

/** The toolbar pill: a verb, the folder NAME (the full path is in the tooltip), and a tone for the dot. */
export function watchPill(s: WatchStatus | null): { verb: string; path: string; tone: WatchTone } {
  if (!s || s.state === 'stopped') return { verb: 'Watch folder…', path: '', tone: 'idle' };
  switch (s.state) {
    case 'watching':
      return { verb: 'Watching', path: folderName(s.folder), tone: 'live' };
    case 'paused':
      return { verb: 'Paused', path: folderName(s.folder), tone: 'paused' };
    case 'folder_missing':
      return { verb: 'Waiting for', path: folderName(s.folder), tone: 'missing' };
  }
}

/** Tooltip for the pill: what is happening and why, in plain words. */
export function watchTitle(s: WatchStatus | null): string {
  if (!s || s.state === 'stopped') {
    return 'Pick a folder (the instrument save folder, a drop folder or a network share). New .asd scans in it are scored as they arrive.';
  }
  const lines = [s.folder];
  if (s.state === 'folder_missing') lines.push('The folder is not available right now; watching resumes by itself when it is back.');
  if (s.state === 'paused') lines.push('Paused: new scans are picked up when you resume.');
  lines.push(s.mode === 'poll' ? `Checked every ${Math.round(s.intervalMs / 1000)} s (${s.modeReason}).` : `Live (${s.modeReason}).`);
  if (s.note) lines.push(cap(s.note) + '.');
  if (s.sessionFile) lines.push(`Session autosave: ${s.sessionFile}`);
  lines.push('Click to watch a different folder.');
  return lines.join('\n');
}

/** Short status-bar text while live, or '' when nothing is worth saying. */
export function watchStatusLine(s: WatchStatus | null): string {
  if (!s || s.state === 'stopped') return '';
  const bits: string[] = [];
  if (s.mode === 'poll') {
    bits.push(`${s.volume === 'network' ? 'Network folder' : 'Poll mode'} · checked every ${Math.round(s.intervalMs / 1000)} s`);
  }
  const files = `${s.pending} file${s.pending === 1 ? '' : 's'}`;
  if (s.pending > 0) bits.push(s.state === 'paused' ? `${files} waiting until you resume` : `${files} still being written`);
  if (s.state === 'folder_missing') bits.push('Folder not available; waiting for it');
  return bits.join(' · ');
}

export function incompleteMessage(i: WatchIncomplete): string {
  return `${i.file}: ${i.reason}.`;
}

/** A one-line notice for arrivals that need one (a changed file, an unreadable file), else null. */
export function arrivalMessage(a: WatchArrival): string | null {
  if (!a.recognised) return `${a.file}: not an ASD scan this app can read (${a.detail}).`;
  if (a.revision > 1) return `${a.file} changed on disk; shown again as revision ${a.revision}.`;
  if (a.kindHint === 'white_reference_save') return `${a.file} is a white-reference save: listed, not scored.`;
  return null;
}

/** Deterministic index from a content hash: the same bytes always get the same example score (mock only). */
export function pickIndex(sha256: string, n: number): number {
  const v = parseInt(sha256.slice(0, 8) || '0', 16);
  return Number.isFinite(v) && n > 0 ? v % n : 0;
}

function cap(s: string): string {
  return s ? s[0].toUpperCase() + s.slice(1) : s;
}
