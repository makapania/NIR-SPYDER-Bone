import { describe, expect, it } from 'vitest';
import { formatModel, formatReading, trackPos, zoomsHeadline } from './display';

describe('display policy (PLAN Step 10, A20)', () => {
  it('never shows a number below 0.5, and never a negative', () => {
    expect(formatReading(-2.54)).toBe('< 0.5%');
    expect(formatReading(0)).toBe('< 0.5%');
    expect(formatReading(0.499)).toBe('< 0.5%');
  });
  it('shows one decimal with ± 1 from 0.5 to 3', () => {
    expect(formatReading(0.5)).toBe('≈ 0.5% (± 1)');
    expect(formatReading(1.74)).toBe('≈ 1.7% (± 1)');
    expect(formatReading(1.75)).toBe('≈ 1.8% (± 1)');
    expect(formatReading(1.74, true)).toBe('≈ 1.7%');
  });
  it('never rounds a Borderline reading up to the 3% line', () => {
    expect(formatReading(2.96)).toBe('≈ 2.9% (± 1)');
    expect(formatReading(2.999, true)).toBe('≈ 2.9%');
  });
  it('shows whole numbers from 3 to 6 and "> 6%" above', () => {
    expect(formatReading(3)).toBe('≈ 3%');
    expect(formatReading(4.4)).toBe('≈ 4%');
    expect(formatReading(5.6)).toBe('≈ 6%');
    expect(formatReading(6)).toBe('> 6%');
    expect(formatReading(17.2)).toBe('> 6%');
  });
  it('a single model shows its own output: one decimal, no cap, nothing below zero', () => {
    expect(formatModel(3.388)).toBe('3.4%');
    expect(formatModel(0.21)).toBe('0.2%');
    expect(formatModel(12.46)).toBe('12.5%');
    expect(formatModel(-0.8)).toBe('< 0%');
    expect(formatModel(null)).toBe('–');
  });
  it('shows a dash for missing values', () => {
    expect(formatReading(null)).toBe('–');
    expect(formatReading(Number.NaN)).toBe('–');
  });
  it('gives the low end the room on the square-root track', () => {
    expect(trackPos(0)).toBe(0);
    expect(trackPos(7)).toBe(100);
    expect(trackPos(12)).toBe(100);
    expect(trackPos(1)).toBeGreaterThan(35); // 1% sits over a third of the way along
  });
});

describe('the ZooMS band check in words follows its call', () => {
  const band = (id: 'nh2044' | 'amide2175', readable: boolean) => ({ id, nm: 0, e: 1, u: 1, readable, lit: true, state: 'trace' as const });
  it('a pattern with an unreadable protein band reads "too noisy", not "resolved"', () => {
    const z = { pattern: 'C' as const, litCount: 4, readableCount: 2, verdict: 'cant_tell' as const };
    expect(zoomsHeadline(z, [band('nh2044', false), band('amide2175', false)])).toBe('Too noisy to read at 2044/2175 nm');
    expect(zoomsHeadline(z, [band('nh2044', true), band('amide2175', true)])).toBe('Too noisy to read every collagen band');
  });
  it('without a core verdict (mock) the pattern decides', () => {
    expect(zoomsHeadline({ pattern: 'E', litCount: 6, readableCount: 6 })).toBe('All six collagen bands resolved');
  });
});

describe('the 1545 nm ZooMS vote (DECISIONS 75)', () => {
  it('a vote-lifted pattern never reads "flat" alone', () => {
    const z = { pattern: 'B' as const, litCount: 2, readableCount: 6, verdict: 'borderline' as const, vote1545: true };
    expect(zoomsHeadline(z)).toContain('1545 nm N–H band shows protein');
  });
});
