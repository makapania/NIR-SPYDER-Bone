import { describe, expect, it } from 'vitest';
import { DOMAIN_NOTE, checkRows, modelAnnotations } from './details';
import { MOCK_SPECS, buildResult } from './mock/scans';
import type { CheckResult, ModelReading, ScanResult } from './types';

const reading = (over: Partial<ModelReading> = {}): ModelReading => ({
  key: 'cons3',
  id: 'collagen.spyder.cons3',
  version: '1.0.0',
  value: 1.2,
  status: { status: 'assessed' },
  domainNote: false,
  ...over,
});

/** A clean, scored example scan with its checks replaced. */
function scanWith(checks: CheckResult[]): ScanResult {
  const s = buildResult(MOCK_SPECS[0], 'radiocarbon', 'hires');
  return { ...s, verdict: 'borderline', checks, signs: s.signs.map((x) => ({ ...x, fired: false, status: { status: 'assessed' } })) };
}
const ok = (id: CheckResult['id']): CheckResult => ({ id, outcome: 'ok', status: { status: 'assessed' } });
const titles = (s: ScanResult) => checkRows(s).map((r) => r.title);

describe('model row annotations (details column)', () => {
  it('a plainly assessed reading says nothing extra', () => {
    expect(modelAnnotations(reading())).toEqual([]);
  });
  it('keeps the grey "less familiar spectrum" note for an unfamiliar spectrum', () => {
    const a = modelAnnotations(reading({ domainNote: true }));
    expect(a).toEqual([{ kind: 'domain', text: DOMAIN_NOTE }]);
    expect(DOMAIN_NOTE).toMatch(/^Less familiar spectrum/);
  });
  it('says why a model was not assessed', () => {
    const a = modelAnnotations(reading({ value: null, status: { status: 'not_assessed', reason: 'no reading' } }));
    expect(a).toEqual([{ kind: 'assessment', text: 'Not assessed: no reading' }]);
    expect(modelAnnotations(reading({ status: { status: 'gated', reason: 'too noisy' } }))[0].text).toBe('Not assessed: too noisy');
  });
  it('shows an assessed qualifier', () => {
    expect(modelAnnotations(reading({ status: { status: 'assessed', note: 'truncated' } }))).toEqual([{ kind: 'assessment', text: 'Truncated' }]);
  });
  it('annotates a B6 noise Check with its implied SD, and all three together in order', () => {
    const a = modelAnnotations(
      reading({ domainNote: true, noiseCheck: true, impliedSd: 0.74, status: { status: 'assessed', note: 'truncated' } }),
    );
    expect(a.map((x) => x.kind)).toEqual(['assessment', 'domain', 'noise']);
    expect(a[2].text).toBe('Noise widens this reading’s error (noise SD ≈ 0.7 points).');
    expect(modelAnnotations(reading({ noiseCheck: true, impliedSd: null }))[0].text).toBe('Noise widens this reading’s error.');
    expect(modelAnnotations(reading({ noiseCheck: false, impliedSd: 0.3 }))).toEqual([]);
  });
});

describe('checks: B6 and B6b are independent', () => {
  it('neither: good signal', () => {
    const t = titles(scanWith([ok('b6'), ok('b6b')]));
    expect(t[0]).toBe('Scan quality');
    expect(checkRows(scanWith([ok('b6'), ok('b6b')]))[0].d).toBe('Good signal, no saturation.');
  });
  it('a B6 Check without a B6b Note is not "Good signal"', () => {
    const rows = checkRows(scanWith([{ id: 'b6', outcome: 'check', status: { status: 'assessed' }, value: 0.81 }, ok('b6b')]));
    expect(rows.map((r) => r.d)).not.toContain('Good signal, no saturation.');
    const b6 = rows.find((r) => r.title === 'Noise widens the reading’s error');
    expect(b6?.notice).toBe(true);
    expect(b6?.d).toContain('noise SD ≈ 0.8 points');
  });
  it('a B6b Note without a B6 Check: noisy scan only', () => {
    const t = titles(scanWith([ok('b6'), { id: 'b6b', outcome: 'note', status: { status: 'assessed' } }]));
    expect(t).toContain('Noisy scan');
    expect(t).not.toContain('Noise widens the reading’s error');
    expect(t).not.toContain('Scan quality');
  });
  it('both: both rows', () => {
    const t = titles(
      scanWith([
        { id: 'b6', outcome: 'check', status: { status: 'assessed' } },
        { id: 'b6b', outcome: 'note', status: { status: 'assessed' } },
      ]),
    );
    expect(t.slice(0, 2)).toEqual(['Noisy scan', 'Noise widens the reading’s error']);
  });
  it('a Rescan shows the scan-quality reason, not the noise rows', () => {
    const s = { ...scanWith([{ id: 'b6', outcome: 'check', status: { status: 'assessed' } }]), verdict: 'rescan' as const };
    const t = titles(s);
    expect(t[0]).toBe('Scan quality');
    expect(t).not.toContain('Noise widens the reading’s error');
  });
  it('never changes the verdict', () => {
    const s = scanWith([{ id: 'b6', outcome: 'check', status: { status: 'assessed' } }]);
    checkRows(s);
    expect(s.verdict).toBe('borderline');
  });
});

describe('heat sign assessment is reported truthfully', () => {
  const withHeat = (status: ScanResult['signs'][number]['status']) => {
    const s = scanWith([ok('b6'), ok('b6b')]);
    return { ...s, signs: s.signs.map((x) => (x.id === 'burnt' ? { ...x, status } : x)) };
  };
  it('all assessed and clean: the combined clear line', () => {
    expect(titles(withHeat({ status: 'assessed' }))).toContain('No contaminant or heat signs');
  });
  it('heat not assessed: never claims "no heat signs", says it was not checked', () => {
    const t = titles(withHeat({ status: 'not_assessed', reason: 'non-finite reading' }));
    expect(t).not.toContain('No contaminant or heat signs');
    expect(t).toContain('No contaminant signs');
    expect(t).toContain('Heat sign not checked');
  });
});
