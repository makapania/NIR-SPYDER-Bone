import { describe, expect, it } from 'vitest';
import { classifyDrop } from './drop';

describe('classifyDrop', () => {
  it('opens the .asd files among the dropped paths (any case), ignoring the rest', () => {
    expect(classifyDrop(['C:/s/a.asd', 'C:/s/notes.txt', '/s/B.ASD'])).toEqual({ kind: 'files', paths: ['C:/s/a.asd', '/s/B.ASD'] });
  });
  it('opens FieldSpec 3 and ViewSpec files too (DECISIONS 87-88)', () => {
    expect(classifyDrop(['C:/s/spot.000', 'C:/s/spot.000.sco', 'C:/s/spot.00', 'C:/s/x.0001'])).toEqual({
      kind: 'files',
      paths: ['C:/s/spot.000', 'C:/s/spot.000.sco'],
    });
  });
  it('opens a single dropped non-.asd path as a folder (the core reports it if it is not one)', () => {
    expect(classifyDrop(['/Users/x/scans.2026'])).toEqual({ kind: 'folder', path: '/Users/x/scans.2026' });
  });
  it('explains anything else', () => {
    expect(classifyDrop(['a.txt', 'b.csv']).kind).toBe('none');
    expect(classifyDrop([]).kind).toBe('none');
  });
});
