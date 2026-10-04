import { describe, expect, it } from 'vitest';
import { arrivalMessage, folderName, isoLocal, pickIndex, watchPill, watchStatusLine, watchTitle } from './live';
import { MockBackend } from './mock';
import { orderScans } from './sort';
import type { WatchArrival, WatchStatus } from './types';

const status = (over: Partial<WatchStatus> = {}): WatchStatus => ({
  folder: 'D:\\LabSpec\\2026-10-03',
  state: 'watching',
  mode: 'native',
  volume: 'local',
  volumeDetail: 'local drive D:',
  modeReason: 'local drive D:: change events plus a rescan every 5 s',
  pending: 0,
  delivered: 0,
  intervalMs: 5000,
  note: null,
  sessionFile: null,
  ...over,
});

let seq = 0;
const arrival = (file: string, over: Partial<WatchArrival> = {}): WatchArrival => ({
  seq: ++seq,
  snapshotId: seq,
  folder: 'D:\\LabSpec\\2026-10-03',
  path: `D:\\LabSpec\\2026-10-03\\${file}`,
  file,
  size: 35132,
  modifiedMs: Date.UTC(2026, 9, 3, 20, 0, seq),
  receivedMs: Date.UTC(2026, 9, 3, 20, 0, seq),
  sha256: (seq * 2654435761).toString(16).padStart(8, '0') + '00'.repeat(28),
  revision: 1,
  existing: false,
  sameContentAs: null,
  recognised: true,
  kindHint: 'sample',
  detail: 'as8',
  ...over,
});

describe('live helpers', () => {
  it('formats local ISO time with the offset', () => {
    expect(isoLocal(Date.UTC(2026, 9, 3, 20, 27, 46), -360)).toBe('2026-10-03T14:27:46-06:00');
    expect(isoLocal(Date.UTC(2026, 9, 3, 20, 27, 46), 330)).toBe('2026-10-04T01:57:46+05:30');
  });

  it('names folders on both platforms', () => {
    expect(folderName('D:\\LabSpec\\2026-10-03\\')).toBe('2026-10-03');
    expect(folderName('/Volumes/lab/drop')).toBe('drop');
  });

  it('the pill says what is happening', () => {
    expect(watchPill(null).verb).toBe('Watch folder…');
    expect(watchPill(status()).tone).toBe('live');
    expect(watchPill(status()).path).toBe('2026-10-03');
    expect(watchPill(status({ state: 'paused' })).verb).toBe('Paused');
    expect(watchPill(status({ state: 'folder_missing' })).verb).toBe('Waiting for');
    expect(watchPill(status({ state: 'stopped' })).tone).toBe('idle');
  });

  it('explains poll mode and files still being written', () => {
    const s = status({ mode: 'poll', volume: 'network', intervalMs: 2000, pending: 2 });
    expect(watchStatusLine(s)).toBe('Network folder · checked every 2 s · 2 files still being written');
    expect(watchTitle(s)).toContain('Checked every 2 s');
    expect(watchStatusLine(status())).toBe('');
    expect(watchStatusLine(status({ mode: 'poll', intervalMs: 2000 }))).toBe('Poll mode · checked every 2 s');
    expect(watchStatusLine(status({ state: 'paused', pending: 1 }))).toBe('1 file waiting until you resume');
  });

  it('says when a file changed on disk, is unreadable or is a reference save', () => {
    expect(arrivalMessage(arrival('a.asd'))).toBeNull();
    expect(arrivalMessage(arrival('a.asd', { revision: 2 }))).toContain('changed on disk');
    expect(arrivalMessage(arrival('b.asd', { recognised: false, detail: 'not an ASD file' }))).toContain('not an ASD scan');
    expect(arrivalMessage(arrival('w.asd', { kindHint: 'white_reference_save' }))).toContain('listed, not scored');
  });

  it('picks example scores deterministically from the content hash', () => {
    expect(pickIndex('0000000a', 3)).toBe(1);
    expect(pickIndex('', 3)).toBe(0);
  });
});

describe('mock live session (example scores on real file names)', () => {
  it('starts empty, lists arrivals newest first, and replaces a rewritten file', () => {
    const m = new MockBackend();
    m.startLive('D:\\LabSpec\\2026-10-03', null);
    expect(m.scans('radiocarbon')).toHaveLength(0);
    expect(m.session()).toMatchObject({ folder: 'D:\\LabSpec\\2026-10-03', example: false, classSource: 'default', serial: null });

    const a = m.addArrival(arrival('Spectrum00041.asd'));
    const b = m.addArrival(arrival('Spectrum00042.asd'));
    let list = orderScans(m.scans('radiocarbon'), 'newest');
    expect(list.map((s) => s.scanId)).toEqual([b, a]);
    expect(list[0].file).toBe('Spectrum00042.asd');
    expect(list[0].serial).toBeNull();
    expect(list[0].notes.map((n) => n.key)).not.toContain('serial_class_mismatch');

    // The older file is rewritten: one row for it, revision 2, now on top.
    const a2 = m.addArrival(arrival('Spectrum00041.asd', { revision: 2 }));
    list = orderScans(m.scans('radiocarbon'), 'newest');
    expect(list).toHaveLength(2);
    expect(list[0].scanId).toBe(a2);
    expect(list[0].fileRevision).toBe(2);
    expect(() => m.views(a2, 31)).not.toThrow();
  });

  it('lists reference saves and unreadable files without scoring them', () => {
    const m = new MockBackend();
    m.startLive('/Volumes/lab/drop', 'hires');
    m.addArrival(arrival('whiteref.asd', { kindHint: 'white_reference_save' }));
    m.addArrival(arrival('junk.asd', { recognised: false, kindHint: null, detail: 'not an ASD file' }));
    m.addArrival(arrival('bone.asd'));
    const list = m.scans('radiocarbon');
    const kinds = Object.fromEntries(list.map((s) => [s.file, s.scanKind]));
    expect(kinds).toEqual({ 'whiteref.asd': 'reference', 'junk.asd': 'unreadable', 'bone.asd': 'sample' });
    for (const s of list.filter((x) => x.scanKind !== 'sample')) expect(s.models.cons3).toBeNull();
    // Most promising first puts unscored files last.
    const p = orderScans(list, 'promising');
    expect(p[0].file).toBe('bone.asd');
    expect(m.session()).toMatchObject({ instrumentClass: 'hires', classSource: 'user' });
  });
});
