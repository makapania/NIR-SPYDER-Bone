import { describe, expect, it } from 'vitest';
import { CITATION, readableCsv } from './readable';
import { formatReading } from './display';
import { MOCK_SPECS, buildResult } from './mock/scans';
import { verdictMeta } from './verdict';
import type { ScanResult } from './types';

/** A small CSV reader for the tests (quoted fields, doubled quotes, CRLF). */
function parse(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let f = '';
  let q = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (q) {
      if (c === '"' && text[i + 1] === '"') (f += '"'), i++;
      else if (c === '"') q = false;
      else f += c;
    } else if (c === '"') q = true;
    else if (c === ',') row.push(f), (f = '');
    else if (c === '\r') continue;
    else if (c === '\n') row.push(f), rows.push(row), (row = []), (f = '');
    else f += c;
  }
  return rows;
}

const scans: ScanResult[] = MOCK_SPECS.map((s) => buildResult(s, 'radiocarbon', 'hires'));
const text = readableCsv(scans, { version: '0.1.0', exportedAt: '2026-10-04 09:00' });
const rows = parse(text.replace(/^﻿/, ''));
const header = rows[4];
const body = rows.slice(5);
const col = (name: string) => header.indexOf(name);

describe('readable export', () => {
  it('is Excel-friendly: byte-order mark, CRLF line ends, citation in the preamble', () => {
    expect(text.startsWith('﻿')).toBe(true);
    expect(text.includes('\r\n')).toBe(true);
    expect(text.replace(/\r\n/g, '').includes('\n')).toBe(false);
    expect(rows[1][0]).toBe(`Please cite: ${CITATION}`);
  });
  it('has one row per listed file, in list order, all the same width as the header', () => {
    expect(body.length).toBe(scans.length);
    expect(body.map((r) => r[0])).toEqual(scans.map((s) => s.file));
    for (const r of body) expect(r.length).toBe(header.length);
  });
  it('uses the words on screen: verdict word, the reading as displayed, model display names', () => {
    for (const [i, s] of scans.entries()) {
      expect(body[i][col('Verdict')]).toBe(verdictMeta(s.verdict).word);
      if (s.verdict !== 'rescan' && s.verdict !== 'not_bone')
        expect(body[i][col('Collagen (consensus of three)')]).toBe(formatReading(s.models.cons3?.value ?? null));
    }
    expect(header).toContain('2045 nm, OH-corrected');
    expect(header).toContain('N–H set, 2175 + 2045 nm, OH-corrected');
    expect(header).toContain('1545 nm, no transfer needed');
  });
  it('writes sentences, never internal keys or JSON', () => {
    const cells = body.flat().join(' | ');
    expect(cells).not.toMatch(/\b(zooms_better|positive_signs|flag_|lift_|cons3|wc2045|F05)\b|\[\s*"/);
  });
  it('carries the ZooMS line text where the app shows one, and flags in words', () => {
    const z = scans.findIndex((s) => s.notes.some((n) => n.key.startsWith('zooms_better')));
    expect(z).toBeGreaterThanOrEqual(0);
    expect(body[z][col('ZooMS')]).toMatch(/chance with ZooMS/);
    const f = scans.findIndex((s) => s.signs.some((x) => x.fired));
    expect(f).toBeGreaterThanOrEqual(0);
    expect(body[f][col('Flags')]).toMatch(/sign/);
  });
  it('never says "wet"', () => {
    expect(text.toLowerCase()).not.toMatch(/\bwet\b/);
  });
});

describe('readable export: each fact once per row', () => {
  it('never repeats a sentence between Why and Also noted', () => {
    const iw = col('Why');
    const ia = col('Also noted');
    for (const r of body) {
      const split = (t: string) => t.split(/(?<=[.!?])\s+/).filter(Boolean);
      const seen = new Set(split(r[iw]));
      for (const x of split(r[ia]).map((y) => y.replace(/^[^:]+: /, ''))) expect(seen.has(x)).toBe(false);
    }
  });
});
