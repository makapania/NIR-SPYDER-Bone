import { describe, expect, it } from 'vitest';
import { LatestOnly, chartIdentity, chartKey, shownViews } from './views';

describe('chart identity: arrays are shown only for the scan they were fetched for', () => {
  const a = { scanId: 's1-1', inputSha256: 'aa', instrumentClass: 'standard' as const };
  it('a new session never shares an identity with an old one (the id carries the generation)', () => {
    expect(chartIdentity(a)).not.toBe(chartIdentity({ ...a, scanId: 's2-1' }));
  });
  it('the class and the input bytes are part of it', () => {
    expect(chartIdentity(a)).not.toBe(chartIdentity({ ...a, instrumentClass: 'hires' }));
    expect(chartIdentity(a)).not.toBe(chartIdentity({ ...a, inputSha256: 'bb' }));
    expect(chartIdentity(a)).toBe(chartIdentity({ ...a }));
    expect(chartIdentity(null)).toBeNull();
  });
  it('the smoothing is part of the request key only', () => {
    const id = chartIdentity(a)!;
    expect(chartKey(id, 31)).not.toBe(chartKey(id, 33));
  });
  it('a serial preset that re-scores the selected scan hides the old class arrays', () => {
    const held = { identity: chartIdentity(a)!, key: chartKey(chartIdentity(a)!, 31), views: 'standard arrays' };
    expect(shownViews(held, chartIdentity(a))).toBe('standard arrays');
    expect(shownViews(held, chartIdentity({ ...a, instrumentClass: 'hires' }))).toBeNull();
    expect(shownViews(held, chartIdentity({ ...a, scanId: 's2-1' }))).toBeNull();
    expect(shownViews(held, null)).toBeNull();
    expect(shownViews(null, chartIdentity(a))).toBeNull();
  });
});

describe('latest request wins', () => {
  it('drops a stale response', () => {
    const r = new LatestOnly();
    const first = r.begin();
    const second = r.begin();
    expect(r.isCurrent(first)).toBe(false);
    expect(r.isCurrent(second)).toBe(true);
  });
});
