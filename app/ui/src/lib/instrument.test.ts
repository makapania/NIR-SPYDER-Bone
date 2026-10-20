// DECISIONS 137: the instrument and correction text follow what the core applied, never the class name; another
// instrument's scan is shown read-only, uncalibrated when no correction for it is installed.
import { describe, expect, it } from 'vitest';
import { cardItems } from './card';
import { checkRows } from './details';
import { bandSummaryTitle } from './bands';
import { levelWord, zoomsHeadline } from './display';
import { CONTACT, correctionText, isLabSpec, scanInstrument, statusInstrument } from './instrument';
import { MOCK_SPECS, buildResult } from './mock/scans';
import { NOTE_TEXT, PROVISIONAL, noteText } from './strings';
import { readableCsv } from './readable';
import type { ScanResult, SessionInfo } from './types';

const hires = (): ScanResult => buildResult(MOCK_SPECS[0], 'radiocarbon', 'hires');
const fs3 = (): ScanResult => ({
  ...buildResult(MOCK_SPECS[0], 'radiocarbon', 'standard'),
  instrumentClass: 'asd.fieldspec3',
  instrumentName: 'FieldSpec 3 (older ASD file format)',
  classSource: 'file_format',
  transfer: null,
});
const session = (over: Partial<SessionInfo> = {}): SessionInfo => ({
  folder: 'D:\\scans',
  watching: false,
  instrumentClass: 'hires',
  classSource: 'user',
  serial: null,
  serialClass: null,
  example: false,
  ...over,
});

describe('instrument and correction text', () => {
  it('names the correction applied, provisional only when the file says so', () => {
    expect(correctionText({ id: 'transfer.labspec4.hires_to_std', version: '0.3.0', provisional: true })).toBe(
      'corrected to standard resolution (transfer.labspec4.hires_to_std v0.3.0, provisional)',
    );
    expect(correctionText({ id: 'transfer.lab.unit', version: '1.0.0', provisional: false })).toBe(
      'corrected to standard resolution (transfer.lab.unit v1.0.0)',
    );
    expect(correctionText(null)).toBeNull();
  });
  it('a high-res scan: High-res and its correction; a standard scan: Standard', () => {
    const h = hires();
    expect(scanInstrument(h)).toContain('High-res, corrected to standard resolution');
    const s = { ...h, instrumentClass: 'standard', transfer: null, classSource: 'user' } as ScanResult;
    expect(scanInstrument(s)).toBe('Standard; set by you');
  });
  it('another instrument with no correction: its name, uncalibrated, identified from the file', () => {
    expect(isLabSpec('asd.fieldspec3')).toBe(false);
    expect(scanInstrument(fs3())).toBe(
      'FieldSpec 3 (older ASD file format), uncalibrated (no correction installed); identified from the file',
    );
  });
  it('the status bar keeps the switch and names another instrument read-only', () => {
    expect(statusInstrument(session(), hires())).toMatch(/^High-res · corrected to standard resolution \(transfer\.hires_to_std v0\.3\.0, provisional\) · set by you$/);
    expect(statusInstrument(session({ instrumentClass: 'standard', classSource: 'default' }), fs3())).toBe(
      'Standard resolution · default; flip it if this unit is high-res · this scan: FieldSpec 3 (older ASD file format) (identified from the file; uncalibrated)',
    );
  });
  it('the uncalibrated note names the instrument and what to do; the contact is one string', () => {
    const t = NOTE_TEXT.instrument_uncalibrated({ class: 'asd.fieldspec3', name: 'FieldSpec 3' });
    expect(t).toContain('no correction for FieldSpec 3 is installed');
    expect(t).toContain(`Not built in: to have this instrument built in, ${CONTACT}: he can send reference bones to scan`);
    expect(CONTACT).toMatch(/^contact Matt Sponheimer \(\S+@\S+\)$/);
    expect(NOTE_TEXT.visible_levels_not_fitted({})).toContain('heat sign reads the scan as measured');
  });
  it('an instrument not built in: band evidence not assessed, the readings stay, signs provisional', () => {
    const why = 'band evidence needs this instrument built in';
    const na = { status: 'not_assessed', reason: why } as const;
    const base = fs3();
    const u: ScanResult = {
      ...base,
      uncalibrated: true,
      evidence: { ...base.evidence, status: na },
      zooms: { ...base.zooms, verdict: 'cant_tell', pattern: null, status: na },
      signs: base.signs.map((x) => (x.id === 'wax' ? { ...x, fired: true } : x)),
    };
    expect(levelWord(u.evidence)).toBe('Not assessed');
    expect(levelWord(base.evidence)).not.toBe('Not assessed');
    expect(zoomsHeadline(u.zooms, u.evidence.bands)).toBe(`Not assessed: ${why}`);
    expect(bandSummaryTitle(u.evidence.level, u.evidence.bands, u.evidence.status)).toContain(`Protein bands: not assessed (${why})`);
    const flag = cardItems(u).alerts.find((a) => a.id === 'flag:wax');
    expect(flag?.label).toMatch(/\(provisional\)$/);
    expect(flag?.body.startsWith(PROVISIONAL)).toBe(true);
    const row = checkRows(u).find((r) => r.icon === 'flag');
    expect(row?.title).toMatch(/\(provisional\)$/);
    expect(row?.d.startsWith(PROVISIONAL)).toBe(true);
    // a LabSpec scan is unchanged
    const lab = { ...base, instrumentClass: 'standard', instrumentName: null, signs: u.signs } as ScanResult;
    expect(cardItems(lab).alerts.find((a) => a.id === 'flag:wax')?.label).not.toMatch(/provisional/);
    expect(NOTE_TEXT.instrument_uncalibrated({ name: 'FieldSpec 3' })).toContain('band evidence (flat bands, lifts, the ZooMS band check) needs the instrument built in');
  });
  it('the second-opinion note no longer blames a high-res transfer', () => {
    const t = NOTE_TEXT.second_opinion_differs({ s: 4.2 });
    expect(t).not.toMatch(/high-res|transfer/i);
    expect(t).toContain('4.2');
  });
  it('the checks caveat follows the instrument: high-res, or untested on another instrument', () => {
    const titles = (s: ScanResult) => checkRows(s).map((r) => r.title);
    expect(titles(hires())).toContain('Contaminant checks on high-res');
    expect(titles(fs3())).toContain('Contaminant checks on this instrument');
    expect(titles({ ...fs3(), instrumentClass: 'standard', instrumentName: null })).not.toContain('Contaminant checks on this instrument');
  });
  it('an instrument not built in: the soft contaminant note and the readable export say provisional too', () => {
    // Astra's whole-branch review: the card qualified the flags only; the soft note and the export dropped it
    const lab = buildResult(MOCK_SPECS.find((s) => s.n === 41)!, 'radiocarbon', 'standard');
    const coat = lab.notes.find((n) => n.key === 'possible_thin_coating')!;
    expect(coat).toBeDefined();
    const fired = lab.signs.map((x) => (x.id === 'wax' ? { ...x, fired: true } : x));
    const u: ScanResult = {
      ...lab,
      instrumentClass: 'asd.fieldspec3',
      instrumentName: 'FieldSpec 3',
      classSource: 'file_format',
      transfer: null,
      uncalibrated: true,
      signs: fired,
    };
    // the card: the note's label and text, as for a flag
    const item = cardItems(u).notes.find((n) => n.id === 'possible_thin_coating')!;
    expect(item.label).toBe('Possible thin coating (provisional)');
    expect(item.body).toBe(`${PROVISIONAL}A weak sign of glue or consolidant at this spot. It may be nothing, but check the spot before sampling.`);
    const plain = cardItems(lab).notes.find((n) => n.id === 'possible_thin_coating')!;
    expect(plain.label).toBe('Possible thin coating');
    expect(plain.body.startsWith(PROVISIONAL)).toBe(false);
    // the note's text wherever it is written (details panel, export)
    expect(noteText(coat, u)).toBe(`${PROVISIONAL}${NOTE_TEXT.possible_thin_coating({})}`);
    expect(noteText(coat, lab)).toBe(NOTE_TEXT.possible_thin_coating({}));
    // the readable export: the flags and the note carry the qualification; the same scan from a LabSpec does not
    const csv = readableCsv([u, { ...lab, signs: fired }], { version: 'test', exportedAt: 'now' });
    const [rowU, rowLab] = csv.split('\r\n').filter((l) => l.startsWith(lab.file));
    expect(rowU).toContain(`Lipid sign (provisional): ${PROVISIONAL}Lipid sign at this spot`);
    expect(rowU).toContain(`${PROVISIONAL}Possible thin coating: a weak sign of glue`);
    expect(rowLab).toContain('Lipid sign: Lipid sign at this spot');
    expect(rowLab).not.toContain('provisional');
    expect(rowLab).not.toContain(PROVISIONAL.trim());
  });
});
