// The binary display-array path (PLAN §2.1): the core sends little-endian float32 bytes through
// `tauri::ipc::Response`; the UI views them as a Float32Array without copying. NaN marks a gap
// (zeroed SG edges, clipped reflectance); the chart wrapper turns NaN into null for uPlot.

const HOST_LE = new Uint8Array(new Uint32Array([1]).buffer)[0] === 1;

/** Accepts what `invoke` returns for a raw Response (ArrayBuffer) or a typed array / number[]
 *  (older runtimes), and returns a Float32Array. Throws on a length that is not a multiple of 4. */
export function decodeF32(data: ArrayBuffer | ArrayBufferView | number[]): Float32Array {
  let buf: ArrayBuffer;
  let offset = 0;
  let length: number;
  if (Array.isArray(data)) {
    const bytes = Uint8Array.from(data);
    buf = bytes.buffer;
    length = bytes.byteLength;
  } else if (ArrayBuffer.isView(data)) {
    buf = data.buffer as ArrayBuffer;
    offset = data.byteOffset;
    length = data.byteLength;
  } else {
    buf = data;
    length = data.byteLength;
  }
  if (length % 4 !== 0) throw new Error(`float32 payload of ${length} bytes is not a multiple of 4`);
  const n = length / 4;
  if (HOST_LE && offset % 4 === 0) return new Float32Array(buf, offset, n);
  const dv = new DataView(buf, offset, length);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = dv.getFloat32(i * 4, true);
  return out;
}

/** Encodes float32 little-endian bytes (used by the mock so it exercises the same decode path). */
export function encodeF32(values: ArrayLike<number>): ArrayBuffer {
  const buf = new ArrayBuffer(values.length * 4);
  const dv = new DataView(buf);
  for (let i = 0; i < values.length; i++) dv.setFloat32(i * 4, values[i], true);
  return buf;
}

/** Splits one k*n block into named views (no copies). */
export function splitViews<K extends string>(block: Float32Array, n: number, order: readonly K[]): Record<K, Float32Array> {
  if (block.length !== n * order.length) {
    throw new Error(`expected ${n * order.length} values for ${order.length} views, got ${block.length}`);
  }
  const out = {} as Record<K, Float32Array>;
  order.forEach((k, j) => (out[k] = block.subarray(j * n, (j + 1) * n)));
  return out;
}

/** The IPC self-test signal; must match `app/src-tauri/src/binary.rs::test_signal`. */
export function ipcTestSignal(i: number): number {
  return i === 0 ? NaN : Math.sin(i / 37) * 10 + i / 1000;
}

export function checkTestSignal(a: Float32Array, n: number): boolean {
  if (a.length !== n) return false;
  if (n > 0 && !Number.isNaN(a[0])) return false;
  for (let i = 1; i < n; i++) {
    if (Math.abs(a[i] - Math.fround(ipcTestSignal(i))) > 0) return false;
  }
  return true;
}
