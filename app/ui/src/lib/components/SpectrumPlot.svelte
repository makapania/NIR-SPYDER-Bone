<script lang="ts" module>
  import type { BandState } from '../types';

  export interface PlotSeries {
    label: string;
    data: Float32Array;
    /** CSS custom property name, e.g. "--s1". */
    color: string;
    width?: number;
    dash?: number[];
    /** The selected scan: drawn last, over a soft glow (under everything) and a surface-coloured gap
     *  that cuts the reference lines where they cross it. */
    halo?: boolean;
    /** Label this series at its peak inside `refLabelsIn`. */
    peakLabel?: string;
  }

  export interface Shade {
    lo: number;
    hi: number;
    /** 'flag': where a heat sign was read (flag colour). */
    kind: 'water' | 'window' | 'band' | 'flag';
    label?: string;
    state?: BandState;
    /** Band lit while a contaminant flag is up: dashed flag-colour outline. */
    flagged?: boolean;
    /** Draw the band's evidence glyph above it (close-ups). */
    glyph?: boolean;
    highlight?: boolean;
  }
</script>

<script lang="ts">
  // SpectrumPlot: the only place uPlot is used (02_spectra_viz §1.3). No uPlot type leaks out, so a
  // hand-written renderer could replace it without touching the app.
  import uPlot from 'uplot';
  import { onDestroy, onMount } from 'svelte';

  interface Props {
    startNm: number;
    stepNm?: number;
    series: PlotSeries[];
    xRange: [number, number];
    /** Series used for the y range (default: all). The evidence close-ups pass the 10% reference
     *  and the scan, so a flat band can never be blown up to look lit. */
    yFrom?: Float32Array[];
    /** Include zero in the y range and draw the zero line (derivative views). */
    zero?: boolean;
    shades?: Shade[];
    joins?: number[];
    xTicks?: number[];
    small?: boolean;
    axes?: boolean;
    yDecimals?: number;
    refLabelsIn?: [number, number];
    bandLabels?: boolean;
    /** Text drawn at the top right of the plot area (e.g. "coating sign"). */
    cornerLabel?: string;
    themeKey: string;
    ariaLabel: string;
    readout?: boolean;
    /** Called once per build with the milliseconds from the start of the build to its first draw (data
     *  preparation + uPlot + first paint of the canvas): the performance budget. */
    onDrawn?: (ms: number) => void;
  }

  let {
    startNm,
    stepNm = 1,
    series,
    xRange,
    yFrom,
    zero = false,
    shades = [],
    joins = [],
    xTicks,
    small = false,
    axes = !small,
    yDecimals,
    refLabelsIn,
    bandLabels = false,
    cornerLabel,
    themeKey,
    ariaLabel,
    readout = false,
    onDrawn,
  }: Props = $props();
  /** performance.now() at the start of the pending build, until its first draw. */
  let drawT0: number | null = null;

  let host: HTMLDivElement;
  let tip: HTMLDivElement | undefined = $state();
  let plot: uPlot | null = null;
  let ro: ResizeObserver | null = null;
  /** uPlot series index of each entry of `series` (the glow and gap lines shift them). */
  let sIdx: number[] = [];

  const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || '#888';

  function alpha(color: string, a: number): string {
    const m = /^#([0-9a-f]{6})$/i.exec(color);
    if (!m) return color;
    const n = parseInt(m[1], 16);
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
  }

  function hatchPattern(ctx: CanvasRenderingContext2D, color: string, dpr: number): CanvasPattern | string {
    const s = Math.round(6 * dpr);
    const c = document.createElement('canvas');
    c.width = s;
    c.height = s;
    const g = c.getContext('2d');
    if (!g) return color;
    g.strokeStyle = color;
    g.lineWidth = 1.1 * dpr;
    g.beginPath();
    g.moveTo(0, s);
    g.lineTo(s, 0);
    g.moveTo(-s / 2, s / 2);
    g.lineTo(s / 2, -s / 2);
    g.moveTo(s / 2, s * 1.5);
    g.lineTo(s * 1.5, s / 2);
    g.stroke();
    return ctx.createPattern(c, 'repeat') ?? color;
  }

  function sliceIdx() {
    const i0 = Math.max(0, Math.round((xRange[0] - startNm) / stepNm));
    const i1 = Math.min(series[0]?.data.length ?? 0, Math.round((xRange[1] - startNm) / stepNm) + 1);
    return [i0, i1] as const;
  }

  function yRange(i0: number, i1: number): [number, number] {
    let lo = Infinity;
    let hi = -Infinity;
    for (const d of yFrom ?? series.map((s) => s.data)) {
      for (let i = i0; i < i1; i++) {
        const v = d[i];
        if (Number.isFinite(v)) {
          if (v < lo) lo = v;
          if (v > hi) hi = v;
        }
      }
    }
    if (!Number.isFinite(lo)) return [-1, 1];
    if (zero) {
      lo = Math.min(lo, -1);
      hi = Math.max(hi, 1);
    }
    const span = hi - lo || 1;
    return [lo - span * 0.08, hi + span * (small ? 0.2 : 0.12)];
  }

  function build() {
    if (!host || !series.length) return;
    drawT0 = performance.now();
    plot?.destroy();
    plot = null;
    const W = host.clientWidth;
    const H = host.clientHeight;
    if (W < 20 || H < 20) return;
    const [i0, i1] = sliceIdx();
    const xs: number[] = [];
    for (let i = i0; i < i1; i++) xs.push(startNm + i * stepNm);
    const cols: (number | null)[][] = series.map((s) => {
      const ys: (number | null)[] = new Array(i1 - i0);
      for (let i = i0; i < i1; i++) {
        const v = s.data[i];
        ys[i - i0] = Number.isFinite(v) ? v : null;
      }
      return ys;
    });
    const [ylo, yhi] = yRange(i0, i1);
    const ink3 = cssVar('--ink-3');
    const grid = cssVar('--grid');
    const axis = cssVar('--axis');
    const chart = cssVar('--chart');
    const font = `${small ? 10 : 11}px ${getComputedStyle(document.body).fontFamily}`;
    // Draw order: [references ...] [surface gap] [scan], over a soft glow drawn in drawUnder. The gap
    // cuts the reference lines where they cross the scan, so the scan always reads on top (dataviz:
    // a 2px surface ring on overlapping marks); the glow gives it a little light on the dark chart.
    const data: (number | null)[][] = [xs];
    const seriesOpts: uPlot.Series[] = [{}];
    const haloIdx = series.findIndex((s) => s.halo);
    const order = series.map((_, i) => i).filter((i) => i !== haloIdx);
    if (haloIdx >= 0) order.push(haloIdx);
    sIdx = new Array(series.length);
    for (const i of order) {
      const s = series[i];
      if (i === haloIdx) {
        seriesOpts.push({ stroke: chart, width: (s.width ?? 2) + 2.5, points: { show: false } });
        data.push(cols[i]);
      }
      seriesOpts.push({
        label: s.label,
        stroke: cssVar(s.color),
        width: s.width ?? 1.5,
        dash: s.dash,
        spanGaps: false,
        points: { show: false },
      });
      data.push(cols[i]);
      sIdx[i] = seriesOpts.length - 1;
    }
    const opts: uPlot.Options = {
      width: W,
      height: H,
      padding: small ? [16, axes ? 14 : 4, axes ? 0 : 4, axes ? 10 : 4] : [18, 16, 0, 4],
      legend: { show: false },
      cursor: readout ? { drag: { x: true, y: false }, points: { show: false }, y: false } : { show: false },
      select: { show: readout, left: 0, top: 0, width: 0, height: 0 },
      scales: {
        x: { time: false, range: (_u, min, max) => (readout && (min !== xRange[0] || max !== xRange[1]) ? [min, max] : xRange) },
        y: { range: () => [ylo, yhi] },
      },
      axes: [
        {
          show: axes,
          stroke: ink3,
          font,
          size: small ? 22 : 30,
          grid: { show: false },
          ticks: { stroke: axis, width: 1, size: 4 },
          splits: xTicks ? () => xTicks : undefined,
          values: (_u, v) => v.map((x) => String(Math.round(x))),
        },
        {
          show: axes && !small,
          stroke: ink3,
          font,
          size: 46,
          grid: { stroke: grid, width: 1 },
          ticks: { show: false },
          values: (_u, v) => v.map((x) => x.toFixed(yDecimals ?? (Math.abs(yhi - ylo) < 2 ? 2 : Math.abs(yhi - ylo) < 10 ? 1 : 0))),
        },
      ],
      series: seriesOpts,
      hooks: {
        drawClear: [(u) => drawUnder(u)],
        draw: [
          (u) => {
            drawOver(u);
            if (drawT0 != null) {
              const ms = performance.now() - drawT0;
              drawT0 = null;
              onDrawn?.(ms);
            }
          },
        ],
        setCursor: readout ? [(u) => showTip(u)] : [],
      },
    };
    plot = new uPlot(opts, data as uPlot.AlignedData, host);
  }

  function drawUnder(u: uPlot) {
    const ctx = u.ctx;
    const dpr = window.devicePixelRatio || 1;
    const { left, top, width, height } = u.bbox;
    const band = cssVar('--band');
    const flag = cssVar('--flag');
    const hatch = hatchPattern(ctx, cssVar('--hatch'), dpr);
    const ink4 = cssVar('--ink-4');
    ctx.save();
    ctx.beginPath();
    ctx.rect(left, top, width, height);
    ctx.clip();
    for (const s of shades) {
      const xa = u.valToPos(s.lo, 'x', true);
      const xb = u.valToPos(s.hi, 'x', true);
      if (xb < left || xa > left + width) continue;
      if (s.kind === 'water') {
        ctx.fillStyle = hatch;
        ctx.fillRect(xa, top, xb - xa, height);
      } else if (s.kind === 'flag') {
        ctx.fillStyle = alpha(flag, 0.13);
        ctx.fillRect(xa, top, xb - xa, height);
        ctx.strokeStyle = flag;
        ctx.lineWidth = 1.4 * dpr;
        ctx.setLineDash([4 * dpr, 3 * dpr]);
        ctx.beginPath();
        ctx.moveTo(Math.round(xa) + 0.5, top);
        ctx.lineTo(Math.round(xa) + 0.5, top + height);
        ctx.moveTo(Math.round(xb) + 0.5, top);
        ctx.lineTo(Math.round(xb) + 0.5, top + height);
        ctx.stroke();
        ctx.setLineDash([]);
      } else if (s.kind === 'window') {
        ctx.fillStyle = cssVar('--window');
        ctx.fillRect(xa, top, xb - xa, height);
        ctx.strokeStyle = cssVar('--window-edge');
        ctx.lineWidth = dpr;
        ctx.beginPath();
        ctx.moveTo(Math.round(xa) + 0.5, top);
        ctx.lineTo(Math.round(xa) + 0.5, top + height);
        ctx.moveTo(Math.round(xb) + 0.5, top);
        ctx.lineTo(Math.round(xb) + 0.5, top + height);
        ctx.stroke();
      } else {
        // A band's ±2 nm read window: a light column (so the curve stays on top) that fades
        // downwards, with a solid cap at the top whose strength follows the band's state.
        const minW = (small ? 6 : 4) * dpr;
        const w = Math.max(minW, xb - xa);
        const xc = (xa + xb) / 2 - w / 2;
        const st = s.state ?? 'flat';
        const hl = s.highlight ? 0.2 : 0;
        if (st === 'cant_tell') {
          ctx.fillStyle = hatch;
          ctx.fillRect(xc, top, w, height);
        } else if (st === 'flat') {
          ctx.fillStyle = alpha(ink4, 0.24 + hl);
          ctx.fillRect(xc, top, w, height);
        } else {
          const a = { strong: 0.44, clear: 0.3, trace: 0.16 }[st] + hl;
          const g = ctx.createLinearGradient(0, top, 0, top + height);
          g.addColorStop(0, alpha(band, Math.min(1, a)));
          g.addColorStop(1, alpha(band, Math.min(1, a * 0.4)));
          ctx.fillStyle = g;
          ctx.fillRect(xc, top, w, height);
          ctx.fillStyle = alpha(band, { strong: 1, clear: 0.78, trace: 0.5 }[st]);
          ctx.fillRect(xc, top, w, 3 * dpr);
        }
        if (s.flagged && st !== 'flat' && st !== 'cant_tell') {
          ctx.strokeStyle = flag;
          ctx.lineWidth = 1.4 * dpr;
          ctx.setLineDash([3 * dpr, 2 * dpr]);
          ctx.strokeRect(xc - dpr, top + dpr, w + 2 * dpr, height - 2 * dpr);
          ctx.setLineDash([]);
        }
      }
    }
    // the scan's glow: a blurred stroke under everything else
    const hi = series.findIndex((s) => s.halo);
    const glowA = parseFloat(cssVar('--glow'));
    if (hi >= 0 && glowA > 0) {
      const xs = u.data[0] as number[];
      const ys = u.data[sIdx[hi]] as (number | null)[];
      ctx.save();
      ctx.strokeStyle = alpha(cssVar(series[hi].color), glowA);
      ctx.shadowColor = cssVar(series[hi].color);
      ctx.shadowBlur = (small ? 6 : 9) * dpr;
      ctx.lineWidth = (small ? 2.5 : 3) * dpr;
      ctx.lineJoin = 'round';
      ctx.beginPath();
      let pen = false;
      for (let i = 0; i < xs.length; i++) {
        const v = ys[i];
        if (v == null) {
          pen = false;
          continue;
        }
        const px = u.valToPos(xs[i], 'x', true);
        const py = u.valToPos(v, 'y', true);
        if (pen) ctx.lineTo(px, py);
        else ctx.moveTo(px, py);
        pen = true;
      }
      ctx.stroke();
      ctx.restore();
    }
    if (zero) {
      const y = Math.round(u.valToPos(0, 'y', true)) + 0.5;
      ctx.strokeStyle = cssVar('--axis');
      ctx.lineWidth = dpr;
      ctx.beginPath();
      ctx.moveTo(left, y);
      ctx.lineTo(left + width, y);
      ctx.stroke();
    }
    ctx.restore();
  }

  function text(ctx: CanvasRenderingContext2D, s: string, x: number, y: number, color: string, align: CanvasTextAlign, weight = 400, italic = false) {
    const dpr = window.devicePixelRatio || 1;
    ctx.font = `${italic ? 'italic ' : ''}${weight} ${(small ? 10 : 10.5) * dpr}px ${getComputedStyle(document.body).fontFamily}`;
    ctx.textAlign = align;
    ctx.textBaseline = 'middle';
    ctx.lineJoin = 'round';
    ctx.strokeStyle = cssVar('--chart');
    ctx.lineWidth = 3 * dpr;
    ctx.strokeText(s, x, y);
    ctx.fillStyle = color;
    ctx.fillText(s, x, y);
  }

  /** A small tag: rounded pill with the window edge colour, used to name the model windows. */
  function pill(ctx: CanvasRenderingContext2D, s: string, x: number, y: number) {
    const dpr = window.devicePixelRatio || 1;
    ctx.font = `650 ${10 * dpr}px ${getComputedStyle(document.body).fontFamily}`;
    const w = ctx.measureText(s).width + 10 * dpr;
    const h = 15 * dpr;
    const x0 = Math.round(x - w / 2);
    const y0 = Math.round(y - h / 2);
    const r = h / 2;
    ctx.beginPath();
    ctx.moveTo(x0 + r, y0);
    ctx.lineTo(x0 + w - r, y0);
    ctx.arc(x0 + w - r, y0 + r, r, -Math.PI / 2, Math.PI / 2);
    ctx.lineTo(x0 + r, y0 + h);
    ctx.arc(x0 + r, y0 + r, r, Math.PI / 2, (3 * Math.PI) / 2);
    ctx.closePath();
    ctx.fillStyle = cssVar('--window-pill');
    ctx.fill();
    ctx.strokeStyle = cssVar('--window-edge');
    ctx.lineWidth = dpr;
    ctx.stroke();
    ctx.fillStyle = cssVar('--ink');
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(s, x0 + w / 2, y0 + h / 2 + 0.5 * dpr);
  }

  function drawOver(u: uPlot) {
    const ctx = u.ctx;
    const dpr = window.devicePixelRatio || 1;
    const { left, top, width, height } = u.bbox;
    const ink3 = cssVar('--ink-3');
    const band = cssVar('--band');
    ctx.save();
    // labels for context bands and windows
    for (const s of shades) {
      if (!s.label) continue;
      const xa = u.valToPos(s.lo, 'x', true);
      const xb = u.valToPos(s.hi, 'x', true);
      if (xb < left || xa > left + width) continue;
      if (s.kind === 'water' && !small) text(ctx, s.label, (xa + xb) / 2, top + 7 * dpr, ink3, 'center', 400, true);
      if (s.kind === 'window') pill(ctx, s.label, (xa + xb) / 2, top + (small ? 8 : 24) * dpr);
      if (s.kind === 'flag') text(ctx, s.label, Math.max(xa, left) + 5 * dpr, top + 40 * dpr, cssVar('--flag-ink'), 'left', 650);
    }
    // band wavelength labels (main plot) or glyphs (close-ups)
    let k = 0;
    for (const s of shades) {
      if (s.kind !== 'band') continue;
      const x = u.valToPos((s.lo + s.hi) / 2, 'x', true);
      if (x < left || x > left + width) continue;
      if (bandLabels) {
        text(ctx, String(Math.round((s.lo + s.hi) / 2)), x, top + height - (6 + (k % 2) * 12) * dpr, ink3, 'center');
        k++;
      }
      if (s.glyph) drawEvGlyph(ctx, x, top - 8 * dpr, 6 * dpr, s.state ?? 'flat', band, ink3, dpr);
    }
    // detector joins: small triangles on the x axis
    for (const j of joins) {
      const x = u.valToPos(j, 'x', true);
      if (x <= left || x >= left + width) continue;
      ctx.fillStyle = cssVar('--ink-4');
      ctx.beginPath();
      ctx.moveTo(x, top + height - 6 * dpr);
      ctx.lineTo(x - 3.5 * dpr, top + height);
      ctx.lineTo(x + 3.5 * dpr, top + height);
      ctx.closePath();
      ctx.fill();
    }
    // reference labels at their peaks inside a window: a short tick in the line's colour, the
    // text in ink, stacked without overlaps and kept inside the plot area
    let labX = -1;
    if (refLabelsIn) {
      const [a, b] = refLabelsIn;
      const pts: { x: number; y: number; s: string; c: string }[] = [];
      series.forEach((s, si) => {
        if (!s.peakLabel) return;
        const d = u.data[sIdx[si]] as (number | null)[];
        let best = -Infinity;
        let bx = a;
        (u.data[0] as number[]).forEach((x, i) => {
          const v = d[i];
          if (x >= a && x <= b && v != null && v > best) {
            best = v;
            bx = x;
          }
        });
        if (Number.isFinite(best)) pts.push({ x: u.valToPos(bx, 'x', true), y: u.valToPos(best, 'y', true), s: s.peakLabel, c: cssVar(s.color) });
      });
      pts.sort((p, q) => p.y - q.y);
      const gap = 10.5 * dpr;
      const yMin = top + 5 * dpr;
      const yMax = top + height - 5 * dpr;
      for (let i = 0; i < pts.length; i++) pts[i].y = Math.max(pts[i].y, i ? pts[i - 1].y + gap : yMin);
      const over = pts.length ? pts[pts.length - 1].y - yMax : 0;
      if (over > 0) for (const p of pts) p.y -= over;
      for (let i = pts.length - 2; i >= 0; i--) pts[i].y = Math.min(pts[i].y, pts[i + 1].y - gap);
      const xLab = Math.max(...pts.map((p) => p.x)) + 5 * dpr;
      labX = xLab;
      for (const p of pts) {
        ctx.strokeStyle = p.c;
        ctx.lineWidth = 2 * dpr;
        ctx.beginPath();
        ctx.moveTo(xLab, p.y);
        ctx.lineTo(xLab + 7 * dpr, p.y);
        ctx.stroke();
        text(ctx, p.s, xLab + 9.5 * dpr, p.y, cssVar('--ink-2'), 'left');
      }
    }
    // the flag's corner note goes on the side away from the reference labels
    if (cornerLabel) {
      const right = labX >= 0 && labX < left + width * 0.45;
      text(ctx, cornerLabel, right ? left + width - 3 * dpr : left + 3 * dpr, top + 8 * dpr, cssVar('--flag'), right ? 'right' : 'left', 650);
    }
    ctx.restore();
  }

  function drawEvGlyph(ctx: CanvasRenderingContext2D, x: number, y: number, R: number, st: BandState, accent: string, ink: string, dpr: number) {
    ctx.save();
    ctx.lineWidth = 1.2 * dpr;
    ctx.strokeStyle = st === 'flat' || st === 'cant_tell' ? ink : accent;
    if (st === 'cant_tell') {
      ctx.setLineDash([2 * dpr, 1.8 * dpr]);
      ctx.beginPath();
      ctx.arc(x, y, R - 0.5 * dpr, 0, Math.PI * 2);
      ctx.stroke();
    } else if (st === 'strong') {
      ctx.fillStyle = accent;
      ctx.beginPath();
      ctx.arc(x, y, R, 0, Math.PI * 2);
      ctx.fill();
    } else {
      ctx.beginPath();
      ctx.arc(x, y, R - 0.5 * dpr, 0, Math.PI * 2);
      ctx.stroke();
      if (st === 'flat') {
        ctx.beginPath();
        ctx.moveTo(x - R * 0.5, y);
        ctx.lineTo(x + R * 0.5, y);
        ctx.stroke();
      } else {
        ctx.fillStyle = accent;
        ctx.beginPath();
        ctx.arc(x, y, R * Math.sqrt(st === 'clear' ? 0.62 : 0.3), 0, Math.PI * 2);
        ctx.fill();
      }
    }
    ctx.restore();
  }

  function showTip(u: uPlot) {
    if (!tip) return;
    const idx = u.cursor.idx;
    if (idx == null || u.cursor.left == null || u.cursor.left < 0) {
      tip.hidden = true;
      return;
    }
    const x = (u.data[0] as number[])[idx];
    const rows = series
      .map((s, si) => {
        const v = (u.data[sIdx[si]] as (number | null)[])[idx];
        return `<div class="r"><span><i style="background:${cssVar(s.color)}"></i>${s.label}</span><b>${v == null ? '–' : v.toFixed(Math.abs(v) < 10 ? 2 : 1)}</b></div>`;
      })
      .join('');
    tip.innerHTML = `<div class="h">${Math.round(x)} nm</div>${rows}`;
    tip.hidden = false;
    const left = u.cursor.left + u.bbox.left / (window.devicePixelRatio || 1);
    const w = host.clientWidth;
    tip.style.left = `${left + 190 > w ? left - 14 - tip.offsetWidth : left + 14}px`;
    tip.style.top = `${Math.max(6, (u.cursor.top ?? 0) - 30)}px`;
  }

  onMount(() => {
    ro = new ResizeObserver(() => {
      if (!plot) return build();
      plot.setSize({ width: host.clientWidth, height: host.clientHeight });
    });
    ro.observe(host);
  });

  $effect(() => {
    // rebuild whenever the inputs or the theme change
    void [series, xRange, yFrom, shades, themeKey, cornerLabel, zero];
    build();
  });

  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });
</script>

<div class="plot" class:small bind:this={host} role="img" aria-label={ariaLabel}>
  {#if readout}<div class="tip" bind:this={tip} hidden></div>{/if}
</div>

<style>
  .plot {
    position: absolute;
    inset: 0;
    overflow: hidden;
  }
  .tip {
    position: absolute;
    pointer-events: none;
    z-index: 5;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 7px 9px;
    font-size: var(--fs-xs);
    min-width: 150px;
  }
  .tip :global(.h) {
    font-weight: 650;
    font-size: var(--fs-sm);
    margin-bottom: 3px;
  }
  .tip :global(.r) {
    display: flex;
    gap: 10px;
    justify-content: space-between;
    color: var(--ink-2);
  }
  .tip :global(.r span) {
    display: inline-flex;
    gap: 6px;
    align-items: center;
  }
  .tip :global(i) {
    width: 10px;
    height: 2px;
    display: inline-block;
  }
</style>
