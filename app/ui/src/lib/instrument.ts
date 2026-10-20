// The instrument and the correction a scan was read through, in plain words (DECISIONS 137): the two LabSpec 4 classes
// by the switch's names, any other instrument by the registry's name; the correction by what the core applied, never
// assumed from the class ("provisional" only when the correction file says so).
import type { ClassSource, InstrumentClass, ScanResult, SessionInfo, TransferInfo } from './types';

/** How a user reaches Matt to have an instrument built in (DECISIONS 137; Matt's choice, confirmed 2026-10-19). Keep
 *  this one string the only place it lives in the app. */
export const CONTACT = 'contact Matt Sponheimer (makapania@gmail.com)';

/** Not built in: what a user of such an instrument can do (DECISIONS 137: reference bones first; a lab's own bones of
 *  known yield as the fallback, worked on by us). */
export const NOT_BUILT_IN = `Not built in: to have this instrument built in, ${CONTACT}: he can send reference bones to scan, or work from scans of your own bones of known yield.`;

export const isLabSpec = (cls: InstrumentClass): boolean => cls === 'standard' || cls === 'hires';

/** The instrument's name: Standard / High-res (the LabSpec 4 switch), or another instrument's registry name. */
export function instrumentName(cls: InstrumentClass, name?: string | null): string {
  if (cls === 'standard') return 'Standard';
  if (cls === 'hires') return 'High-res';
  return name || cls;
}

/** The correction that made the standard-resolution stream, or null when the scan is read as measured. */
export function correctionText(t: TransferInfo | null | undefined): string | null {
  if (!t) return null;
  return `corrected to standard resolution (${t.id} v${t.version}${t.provisional ? ', provisional' : ''})`;
}

/** Who set the class, in words. */
export function classSourceText(src: ClassSource, serial?: number | null): string {
  switch (src) {
    case 'user':
      return 'set by you';
    case 'default':
      return 'default setting';
    case 'header_preset':
      return 'preset from the detector settings';
    case 'file_format':
      return 'identified from the file';
    default:
      return serial != null ? `preset from serial ${serial}` : 'preset from the serial';
  }
}

/** One scan's instrument line: its instrument, the correction (or "uncalibrated" when another instrument's scan has
 *  none), and who set the class. */
export function scanInstrument(s: ScanResult): string {
  const corr = correctionText(s.transfer);
  const what = corr
    ? `${instrumentName(s.instrumentClass, s.instrumentName)}, ${corr}`
    : isLabSpec(s.instrumentClass)
      ? instrumentName(s.instrumentClass)
      : `${instrumentName(s.instrumentClass, s.instrumentName)}, uncalibrated (no correction installed)`;
  return `${what}; ${classSourceText(s.classSource, s.serial)}`;
}

/** The status bar's instrument line: the session's switch, and the selected scan's own instrument when another
 *  instrument wrote it (shown, never set here: the form for choosing other instruments comes later, DECISIONS 98). */
export function statusInstrument(session: SessionInfo, selected: ScanResult | null): string {
  const sel = selected && selected.scanKind !== 'unscored' ? selected : null;
  const corr = sel && sel.instrumentClass === session.instrumentClass ? correctionText(sel.transfer) : null;
  const cls =
    session.instrumentClass === 'hires'
      ? `High-res · ${corr ?? 'corrected to standard resolution'}`
      : session.instrumentClass === 'standard'
        ? 'Standard resolution'
        : instrumentName(session.instrumentClass);
  const src =
    session.classSource === 'user'
      ? 'set by you'
      : session.classSource === 'default'
        ? 'default; flip it if this unit is high-res'
        : session.classSource === 'header_preset'
          ? 'preset from the detector settings; flip it if wrong'
          : session.classSource === 'file_format'
            ? 'identified from the file'
            : `preset from serial ${session.serial}`;
  const other =
    sel && !isLabSpec(sel.instrumentClass)
      ? ` · this scan: ${instrumentName(sel.instrumentClass, sel.instrumentName)} (identified from the file${sel.transfer ? '' : '; uncalibrated'})`
      : '';
  return `${cls} · ${src}${other}`;
}
