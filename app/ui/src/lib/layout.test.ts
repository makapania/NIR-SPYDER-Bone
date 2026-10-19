import { describe, expect, it } from 'vitest';
import { LIST_MAX, LIST_MIN, MAIN_MIN, clampListWidth, columnDefaults, listWidthBounds, parseListWidth } from './layout';

describe('the scans list width', () => {
  it('defaults follow the window breakpoints', () => {
    expect(columnDefaults(1100)).toEqual({ list: 216, details: 290 });
    expect(columnDefaults(1300)).toEqual({ list: 216, details: 290 });
    expect(columnDefaults(1366)).toEqual({ list: 236, details: 312 });
    expect(columnDefaults(1440)).toEqual({ list: 272, details: 344 });
  });

  it('never squeezes the main column below its minimum, down to the 1100 px window', () => {
    for (const vw of [1100, 1180, 1280, 1366, 1440, 1920]) {
      const { min, max } = listWidthBounds(vw);
      expect(min).toBe(LIST_MIN);
      expect(max).toBeGreaterThanOrEqual(min);
      expect(max).toBeLessThanOrEqual(LIST_MAX);
      expect(vw - max - columnDefaults(vw).details).toBeGreaterThanOrEqual(MAIN_MIN);
    }
    // a 1100 px window still lets the list grow a little
    expect(listWidthBounds(1100).max).toBe(270);
  });

  it('clamps a request into bounds, in whole pixels', () => {
    expect(clampListWidth(100, 1280)).toBe(LIST_MIN);
    expect(clampListWidth(300.4, 1280)).toBe(300);
    expect(clampListWidth(900, 1280)).toBe(listWidthBounds(1280).max);
    expect(clampListWidth(Number.NaN, 1280)).toBe(LIST_MIN);
    // a width chosen on a wide window is re-clamped when the window shrinks
    expect(clampListWidth(500, 1100)).toBe(270);
  });

  it('reads a stored width, ignoring anything unusable', () => {
    expect(parseListWidth('320')).toBe(320);
    expect(parseListWidth(null)).toBeNull();
    expect(parseListWidth('')).toBeNull();
    expect(parseListWidth('wide')).toBeNull();
    expect(parseListWidth('-4')).toBeNull();
  });
});
