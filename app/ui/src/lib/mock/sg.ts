// MOCK ONLY. A small Savitzky–Golay implementation so the browser mock can draw spectra without the
// core. The real app never computes numbers in the UI: spyder-core sends every display array.

const cache = new Map<string, Float64Array>();

/** Convolution weights for window w (odd), polynomial order p, derivative d, unit spacing. */
export function sgWeights(w: number, p = 3, d = 2): Float64Array {
  const key = `${w}|${p}|${d}`;
  const hit = cache.get(key);
  if (hit) return hit;
  const h = (w - 1) / 2;
  const m = p + 1;
  // Normal equations: (J^T J) a = J^T y; the d-th derivative at 0 is d! * a_d.
  const jtj = Array.from({ length: m }, () => new Float64Array(m));
  for (let x = -h; x <= h; x++) for (let i = 0; i < m; i++) for (let k = 0; k < m; k++) jtj[i][k] += x ** (i + k);
  // Solve jtj * c = e_d (column of the inverse), by Gauss–Jordan.
  const aug = jtj.map((row, i) => [...row, i === d ? 1 : 0]);
  for (let c = 0; c < m; c++) {
    let piv = c;
    for (let r = c + 1; r < m; r++) if (Math.abs(aug[r][c]) > Math.abs(aug[piv][c])) piv = r;
    [aug[c], aug[piv]] = [aug[piv], aug[c]];
    const div = aug[c][c];
    for (let k = c; k <= m; k++) aug[c][k] /= div;
    for (let r = 0; r < m; r++) {
      if (r === c) continue;
      const f = aug[r][c];
      for (let k = c; k <= m; k++) aug[r][k] -= f * aug[c][k];
    }
  }
  const inv = aug.map((row) => row[m]);
  let fact = 1;
  for (let i = 2; i <= d; i++) fact *= i;
  const wts = new Float64Array(w);
  for (let j = 0; j < w; j++) {
    const x = j - h;
    let s = 0;
    for (let i = 0; i < m; i++) s += inv[i] * x ** i;
    wts[j] = s * fact;
  }
  cache.set(key, wts);
  return wts;
}

/** Applies SG; the h edge points are NaN (a gap), as the core's "edges zeroed" display arrays are. */
export function savgol(y: ArrayLike<number>, w: number, p = 3, d = 2): Float64Array {
  const c = sgWeights(w, p, d);
  const h = (w - 1) / 2;
  const n = y.length;
  const out = new Float64Array(n).fill(NaN);
  for (let i = h; i < n - h; i++) {
    let s = 0;
    for (let j = 0; j < w; j++) s += c[j] * y[i - h + j];
    out[i] = s;
  }
  return out;
}
