import { describe, expect, it } from 'vitest';
import { checkTestSignal, decodeF32, encodeF32, ipcTestSignal, splitViews } from './f32';

describe('binary float32 IPC path', () => {
  it('decodes little-endian float32 bytes into a Float32Array', () => {
    const bytes = new Uint8Array([0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x20, 0xc0]); // 1.0, -2.5
    expect(Array.from(decodeF32(bytes.buffer))).toEqual([1, -2.5]);
  });
  it('accepts a typed-array view at an offset and a plain number[] (older runtimes)', () => {
    const buf = new Uint8Array(12);
    new DataView(buf.buffer).setFloat32(4, 3.25, true);
    expect(decodeF32(new Uint8Array(buf.buffer, 4, 4))[0]).toBe(3.25);
    expect(decodeF32([0, 0, 0x80, 0x3f])[0]).toBe(1);
    expect(decodeF32(new Uint8Array(buf.buffer, 1, 4))[0]).toBe(0); // unaligned: copied
  });
  it('rejects a payload that is not a multiple of 4 bytes', () => {
    expect(() => decodeF32(new ArrayBuffer(6))).toThrow();
  });
  it('round-trips the IPC self-test signal, NaN gap included', () => {
    const n = 2151;
    const a = decodeF32(encodeF32(Float64Array.from({ length: n }, (_, i) => ipcTestSignal(i))));
    expect(a.length).toBe(n);
    expect(Number.isNaN(a[0])).toBe(true);
    expect(checkTestSignal(a, n)).toBe(true);
    a[100] += 0.5;
    expect(checkTestSignal(a, n)).toBe(false);
  });
  it('splits one k×n block into named views without copying', () => {
    const block = Float32Array.from([1, 2, 3, 4, 5, 6]);
    const v = splitViews(block, 3, ['R', 'A'] as const);
    expect(Array.from(v.A)).toEqual([4, 5, 6]);
    expect(v.R.buffer).toBe(block.buffer);
    expect(() => splitViews(block, 4, ['R', 'A'] as const)).toThrow();
  });
});
