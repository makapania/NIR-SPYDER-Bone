import { describe, expect, it } from 'vitest';
import { DOMAIN_NOTE, checkRows, modelAnnotations, verdictNumbers } from './details';
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
    expect(a[2].text).toBe('Noise could carry this reading across a verdict cut (noise SD ≈ 0.7 points).');
    expect(modelAnnotations(reading({ noiseCheck: true, impliedSd: null }))[0].text).toBe('Noise could carry this reading across a verdict cut.');
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
    const b6 = rows.find((r) => r.title === 'Noise could change the verdict');
    expect(b6?.notice).toBe(true);
    expect(b6?.d).toContain('noise SD ≈ 0.8 points');
  });
  it('noise far from any cut: a quiet "some noise" line, not "Good signal" (DECISIONS 109)', () => {
    const rows = checkRows(scanWith([{ id: 'b6', outcome: 'ok', status: { status: 'assessed' }, value: 1.42 }, ok('b6b')]));
    expect(rows[0]).toEqual({
      icon: 'info',
      title: 'Scan quality',
      d: 'Some noise in the reading (noise SD ≈ 1.4 points), not enough to change the verdict.',
      notice: false,
    });
    expect(checkRows(scanWith([{ id: 'b6', outcome: 'ok', status: { status: 'assessed' }, value: 0.3 }, ok('b6b')]))[0].d).toBe(
      'Good signal, no saturation.',
    );
  });
  it('no quiet noise line when CONS3 has no reading', () => {
    const s = scanWith([{ id: 'b6', outcome: 'ok', status: { status: 'assessed' }, value: 1.42 }, ok('b6b')]);
    const noReading = { ...s, models: { ...s.models, cons3: s.models.cons3 ? { ...s.models.cons3, value: null } : null } };
    expect(checkRows(noReading)[0].d).toBe('Good signal, no saturation.');
  });
  it('a B6b Note without a B6 Check: noisy scan only', () => {
    const t = titles(scanWith([ok('b6'), { id: 'b6b', outcome: 'note', status: { status: 'assessed' } }]));
    expect(t).toContain('Noisy scan');
    expect(t).not.toContain('Noise could change the verdict');
    expect(t).not.toContain('Scan quality');
  });
  it('both: both rows', () => {
    const t = titles(
      scanWith([
        { id: 'b6', outcome: 'check', status: { status: 'assessed' } },
        { id: 'b6b', outcome: 'note', status: { status: 'assessed' } },
      ]),
    );
    expect(t.slice(0, 2)).toEqual(['Noisy scan', 'Noise could change the verdict']);
  });
  it('a Rescan shows the scan-quality reason, not the noise rows', () => {
    const s = { ...scanWith([{ id: 'b6', outcome: 'check', status: { status: 'assessed' } }]), verdict: 'rescan' as const };
    const t = titles(s);
    expect(t[0]).toBe('Scan quality');
    expect(t).not.toContain('Noise could change the verdict');
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

describe('the long-wave row names the region the general organic check actually used', () => {
  const withC1 = (note?: string) => {
    const s = scanWith([ok('b6'), ok('b6b'), { id: 'longwave', outcome: 'note', status: { status: 'assessed' } }]);
    return { ...s, signs: s.signs.map((x) => (x.id === 'c1' ? { ...x, status: { status: 'assessed' as const, note } } : x)) };
  };
  const longwave = (s: ScanResult) => checkRows(s).find((r) => r.title === 'Long-wave region not used')?.d ?? '';
  it('truncated above 2300 nm: the reduced-organic-check wording', () => {
    expect(longwave(withC1('truncated (long-wave region too noisy)'))).toContain('general organic check run without it');
  });
  it('the reduced form below 1800 nm is not described as the full check or as the 2300 nm truncation', () => {
    const d = longwave(withC1('reduced (below 1800 nm only; noise in 2000-2100 nm)'));
    expect(d).not.toContain('still uses it');
    expect(d).not.toContain('general organic check run without it');
  });
  it('the full check: it still uses the long-wave region', () => {
    expect(longwave(withC1())).toContain('still uses it');
  });
});

describe('the panel readers (2026-10-18) in the mock results', () => {
  it('a scored scan carries the low-range estimate and the C–H reader, both plainly assessed', () => {
    const scored = MOCK_SPECS.map((sp) => buildResult(sp, 'radiocarbon', 'hires')).find((x) => x.verdict !== 'rescan');
    expect(scored?.models.lowrange?.value).toEqual(expect.any(Number));
    expect(scored?.models.ch1728?.value).toEqual(expect.any(Number));
    const r = scored!.models.lowrange as ModelReading;
    expect(modelAnnotations(r)).toEqual([]);
  });
});

describe('"How this verdict was made": the scan’s own numbers (DECISIONS 121)', () => {
  const base = () => buildResult(MOCK_SPECS[0], 'radiocarbon', 'standard');
  it('shows the consensus parts, the C–H gap against its two levels, the largest reader gap and the noise', () => {
    const s = {
      ...base(),
      checks: [{ id: 'b6' as const, outcome: 'ok' as const, status: { status: 'assessed' as const }, value: 0.34 }],
      panel: {
        ch: 2.0,
        chGap: 0.4,
        chPossibleAbove: 1.338,
        chStrongAbove: 2.359,
        contrasts: [
          { name: 'CH', value: 0.4, firesAbove: 2.5, fraction: 0.9 },
          { name: 'GL', value: 0.3, firesAbove: 0.7, fraction: 0.6 },
          { name: 'RAW', value: 1.0, firesAbove: 6.3, fraction: 0.1 },
        ],
      },
    };
    const rows = verdictNumbers(s);
    expect(rows.map((r) => r.title)).toEqual(['Consensus models', 'C–H against N–H', 'Most unusual reader difference', 'Noise']);
    expect(rows[1].d).toContain('+0.4 points');
    expect(rows[1].d).toContain('above +1.3');
    expect(rows[1].d).toContain('"likely" above +2.4');
    // the most unusual difference excludes C–H (it has its own row) and picks the pair nearest its own line, not the largest
    expect(rows[2].d).toContain('N–H consensus above the narrow 1545 nm band: 0.3 points, 60% of the way to its line');
    expect(rows[2].d).toContain('fires above 0.7');
    expect(rows[3].d).toContain('SD ≈ 0.3');
  });
  it('a negative C–H gap reads as minus', () => {
    const s = { ...base(), panel: { ch: 0, chGap: -0.6, chPossibleAbove: 1.3, chStrongAbove: 2.4, contrasts: [] } };
    expect(verdictNumbers(s).find((r) => r.title === 'C–H against N–H')?.d).toContain('−0.6 points');
  });
  it('leaves out what it cannot show (no panel record: ZooMS, or the mock)', () => {
    const s = { ...base(), panel: null };
    const t = verdictNumbers(s).map((r) => r.title);
    expect(t).not.toContain('C–H against N–H');
    expect(t).not.toContain('Most unusual reader difference');
  });
});
