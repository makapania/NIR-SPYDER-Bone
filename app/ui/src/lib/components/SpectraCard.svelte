<script lang="ts">
  // The spectra card: the main plot over the reference bones, and the two model-window close-ups.
  // Second derivative is drawn as −d²A/dλ² (bands up); default view 1100–2450 nm. Display smoothing
  // changes only the main plot. On high-res the main plot shows the transferred spectrum (what every model and
  // band rule reads) with "As measured" one toggle away. The 2045 close-up defaults to the OH-corrected view:
  // the window features with the OH/water direction projected out, exactly as the OH-corrected model sees them
  // (computed in Rust; the reading they imply IS the model's reading), with an "As measured" toggle; the 1500
  // close-up is OH-corrected. The corrected view exists only inside the model window.
  import SpectrumPlot, { type PlotSeries, type Shade } from './SpectrumPlot.svelte';
  import { formatModel, shortName } from '../display';
  import type { OhWindowInfo, ReferenceSpectrum, ScanResult, SpectrumViews, ViewKey } from '../types';

  export type Mode = 'R' | 'A' | 'D2';

  interface Props {
    scan: ScanResult;
    views: SpectrumViews | null;
    refs: ReferenceSpectrum[];
    refsOn: Record<string, boolean>;
    mode: Mode;
    smoothing: number;
    lens: 'ohc' | 'raw';
    /** High-res main plot: the transferred spectrum ('std', default) or the scan as measured ('meas'). */
    stream: 'std' | 'meas';
    ohWindows: OhWindowInfo[];
    themeKey: string;
    highlight: number;
    onMode: (m: Mode) => void;
    onSmoothing: (w: number) => void;
    onToggleRef: (label: string) => void;
    onLens: (l: 'ohc' | 'raw') => void;
    onStream: (s: 'std' | 'meas') => void;
    /** The main plot finished its first draw (performance budget). */
    onMainDrawn?: (ms: number) => void;
  }
  let {
    scan,
    views,
    refs,
    refsOn,
    mode,
    smoothing,
    lens,
    stream,
    ohWindows,
    themeKey,
    highlight,
    onMode,
    onSmoothing,
    onToggleRef,
    onLens,
    onStream,
    onMainDrawn,
  }: Props = $props();

  const SMOOTHING = Array.from({ length: 21 }, (_, i) => 11 + 2 * i);
  const REF_COLOR: Record<string, string> = { '0%': '--r0', '1%': '--r1', '3%': '--r3', '6%': '--r6', '10%': '--r10' };
  const RAMP = ['--r0', '--r1', '--r3', '--r6', '--r10'];
  const hires = $derived(scan.instrumentClass === 'hires');
  /** The scan as measured on the main plot (high-res only; standard-res is its own stream). */
  const asMeasured = $derived(hires && stream === 'meas');
  const MEAS: Record<'R' | 'A' | 'D2', ViewKey> = { R: 'R_meas', A: 'A_meas', D2: 'D2_meas' };
  const win2045 = $derived(ohWindows.find((w) => w.label === '2045'));
  const win1500 = $derived(ohWindows.find((w) => w.label === '1500'));
  /** The corrected 2045 view is drawn only when the app has the model's OH/water direction. */
  const ohc2045 = $derived(lens === 'ohc' && (win2045?.available ?? false));
  const ohc1500 = $derived(win1500?.available ?? false);
  const flagged = $derived(scan.signs.some((s) => s.fired && s.id !== 'burnt'));
  const usable = $derived(scan.verdict !== 'rescan');
  // The heat sign, marked where it was read: charred = the visible edge stays dark up to edge50 (Reflectance and
  // Absorbance views, 450 nm up); calcined = sharp OH peaks at 979 and 1433 nm.
  const heat = $derived(scan.signs.find((s) => s.id === 'burnt' && s.fired));
  const heatShades = $derived.by((): Shade[] => {
    if (!heat) return [];
    if (heat.heatKind === 'calcined')
      return [
        { lo: 967, hi: 991, kind: 'flag', label: 'heat sign' },
        { lo: 1420, hi: 1446, kind: 'flag', label: 'heat sign' },
      ];
    if (heat.heatKind === 'charred' && heat.edge50Nm != null && mode !== 'D2')
      return [{ lo: 450, hi: heat.edge50Nm, kind: 'flag', label: `heat sign: dark visible edge (to ${Math.round(heat.edge50Nm)} nm)` }];
    return [];
  });
  /** The charred mark lives in the visible range, outside the 2nd-derivative view. */
  const heatOffView = $derived(heat?.heatKind === 'charred' && heat.edge50Nm != null && mode === 'D2');

  const mainSeries = $derived.by((): PlotSeries[] => {
    if (!views) return [];
    const out: PlotSeries[] = refs
      .filter((r) => refsOn[r.label])
      .map((r) => ({ label: r.legend, data: r.spectra.views[mode], color: REF_COLOR[r.label] ?? '--r3', width: 1.35, dash: [5, 3] }));
    out.push({
      label: shortName(scan.file) + (asMeasured ? ' (as measured)' : ''),
      data: views.views[asMeasured ? MEAS[mode] : mode],
      color: '--s1',
      width: 2.2,
      halo: true,
    });
    return out;
  });

  const mainShades = $derived.by((): Shade[] => {
    const s: Shade[] = [
      { lo: 1405, hi: 1495, kind: 'water', label: 'OH/water band' },
      { lo: 1885, hi: 1985, kind: 'water', label: 'OH/water band' },
      { lo: 1500, hi: 1550, kind: 'window', label: '1545' },
      { lo: 2030, hi: 2060, kind: 'window', label: '2045' },
    ];
    // band states belong to the transferred spectrum: no band shading over the as-measured high-res curve
    if (mode === 'D2' && scan.evidence.bands.length && !asMeasured) {
      scan.evidence.bands.forEach((b, k) =>
        s.push({ lo: b.nm - 2, hi: b.nm + 2, kind: 'band', state: b.state, flagged, highlight: highlight === k }),
      );
    }
    return [...s, ...heatShades];
  });

  // the references in the same view as the scan (both corrected, or both as measured)
  const lensSeries = (key: 'D2_31' | 'D2_31_ohc'): PlotSeries[] => {
    if (!views) return [];
    const out: PlotSeries[] = refs
      .filter((r) => refsOn[r.label])
      .map((r) => ({ label: r.legend, data: r.spectra.views[key], color: REF_COLOR[r.label] ?? '--r3', width: 1.25, dash: [4, 2.5], peakLabel: r.label }));
    out.push({ label: shortName(scan.file), data: views.views[key], color: '--s1', width: 2, halo: true });
    return out;
  };
  const lens2045 = $derived(lensSeries(ohc2045 ? 'D2_31_ohc' : 'D2_31'));
  const lens1500 = $derived(lensSeries(ohc1500 ? 'D2_31_ohc' : 'D2_31'));
  // the OH-corrected 1545 nm evidence band, read at 1543–1547 nm on this window (DECISIONS 75)
  const band1545 = $derived(scan.evidence.bands.find((b) => b.id === 'nh1545'));
  // The same x-range in both lens states (the corrected view exists only inside the window), so toggling never looks
  // like a peak shift: a wider uncorrected view zoomed out 2.9x and made a < 1 nm move read as a shift (phase 0c evidence §A).
  const range2045: [number, number] = [2026, 2064];
  const range1500: [number, number] = [1496, 1554];

  const lifted = $derived(scan.verdictRule === 'lift_good' || scan.verdictRule === 'lift_borderline');
  const extra = $derived(
    lifted
      ? ' The window shows more protein than the models read; the bands raised this verdict.'
      : flagged
        ? ' Band height may come from the coating.'
        : scan.verdictRule === 'flat_bands'
          ? ' The protein bands are flat, so the verdict follows the bands.'
          : '',
  );
  const yLabel = $derived(
    mode === 'R'
      ? 'Reflectance'
      : mode === 'A'
        ? 'Absorbance, log₁₀(1/R)'
        : `−d²A/dλ² (×10⁻⁵), display smoothing SG ${smoothing}`,
  );
  const streamNote = $derived(
    !hires ? '' : asMeasured ? ' · high-res, as measured (the models read the transferred spectrum)' : ' · high-res, transferred to standard resolution',
  );
</script>

<section class="card chartcard" aria-label="Spectra">
  <div class="toolbar">
    <div class="seg" role="group" aria-label="Spectrum view">
      <button aria-pressed={mode === 'R'} onclick={() => onMode('R')}>Reflectance</button>
      <button aria-pressed={mode === 'A'} onclick={() => onMode('A')}>Absorbance</button>
      <button aria-pressed={mode === 'D2'} onclick={() => onMode('D2')}>2nd derivative</button>
    </div>
    <label class="sg" title="Display only. The models and the band evidence keep SG 31.">
      Display smoothing
      <select class="plain" value={smoothing} disabled={mode !== 'D2'} onchange={(e) => onSmoothing(+(e.currentTarget as HTMLSelectElement).value)}>
        {#each SMOOTHING as w (w)}<option value={w}>SG {w}{w === 31 ? ' (default)' : ''}</option>{/each}
      </select>
    </label>
    {#if smoothing !== 31 && mode === 'D2'}<span class="chainnote">Chart only: the models read SG 31.</span>{/if}
    {#if heatOffView && usable}
      <button class="heatbtn" onclick={() => onMode('R')} title="The heat sign is read on the visible edge (450 nm up), outside this view">Heat sign: show in Reflectance</button>
    {/if}
    {#if hires}
      <div class="seg" role="group" aria-label="High-res spectrum">
        <button aria-pressed={stream === 'std'} onclick={() => onStream('std')}
          title="The spectrum transferred to standard resolution (provisional): what every model and band rule reads">Transferred</button>
        <button aria-pressed={stream === 'meas'} onclick={() => onStream('meas')} title="The high-res scan as measured">As measured</button>
      </div>
    {/if}
    <span class="spacer"></span>
    <div class="chips" aria-label="Reference bone">
      {#each refs as r (r.label)}
        <button class="chip" aria-pressed={refsOn[r.label]} onclick={() => onToggleRef(r.label)} title="Reference bone, {r.legend} collagen (mean of {r.n} Ryder reference bones)">
          <span class="dash" style="color: var({REF_COLOR[r.label]})"></span>{r.legend}
        </button>
      {/each}
    </div>
  </div>

  <div class="chartwrap">
    <div class="main">
      <div class="ylabel">{yLabel}{streamNote}</div>
      <div class="plotbox">
        {#if views && usable}
          <SpectrumPlot startNm={views.startNm} series={mainSeries} xRange={mode === 'D2' ? [1100, 2450] : [350, 2500]}
            zero={mode === 'D2'} shades={mainShades} joins={[1000, 1800]} bandLabels={mode === 'D2'}
            xTicks={mode === 'D2' ? [1250, 1500, 1750, 2000, 2250] : [500, 1000, 1500, 2000, 2500]} yDecimals={mode === 'D2' ? 0 : 2}
            {themeKey} ariaLabel="Spectrum of {scan.file} over the reference bones" readout onDrawn={onMainDrawn} />
        {:else if views}
          <SpectrumPlot startNm={views.startNm} series={mainSeries.slice(-1)} xRange={[350, 2500]} joins={[1000, 1800]}
            xTicks={[500, 1000, 1500, 2000, 2500]} {themeKey} ariaLabel="Spectrum of {scan.file}" readout onDrawn={onMainDrawn} />
        {/if}
      </div>
    </div>

    <div class="lens">
      <div class="lenshead"><span class="ttl">2045 nm model</span><span class="muted small">2030–2060 nm</span></div>
      <div class="seg mini" role="group" aria-label="2045 nm view">
        <button aria-pressed={ohc2045} onclick={() => onLens('ohc')} disabled={!win2045?.available}
          title={win2045?.available ? 'The window as the OH-corrected model reads it: the altered OH/water direction projected out (references the same way). Heights in this view are not collagen amounts; the model reads the shape.' : `Not available: ${win2045?.reason ?? 'no OH/water direction for this model'}`}>OH-corrected</button>
        <button aria-pressed={!ohc2045} onclick={() => onLens('raw')} title="The same window without the OH/water correction">Uncorrected</button>
      </div>
      <div class="plotlens" class:on={views && usable}>
        {#if views && usable}
          <SpectrumPlot startNm={views.startNm} series={lens2045} xRange={range2045} zero
            shades={[{ lo: 2030, hi: 2060, kind: 'window' }]}
            small axes xTicks={[2030, 2045, 2060]} refLabelsIn={[2030, 2060]}
            cornerLabel={flagged ? 'coating sign' : undefined} {themeKey} ariaLabel="2045 nm model window ({ohc2045 ? 'OH-corrected' : 'uncorrected'})" />
        {/if}
      </div>
      {#if usable && scan.models.wc2045}
        <p>
          {#if !ohc2045}
            Uncorrected, the altered OH/water band tilts this window. The model reads the OH-corrected view (<b>{formatModel(scan.models.wc2045.value)}</b>).
            {#if !win2045?.available}<span class="unavail">OH-corrected view not available: {win2045?.reason ?? 'no OH/water direction for this model'}.</span>{/if}
          {:else}
            2045 nm, OH-corrected reads <b>{formatModel(scan.models.wc2045.value)}</b>.{extra}
          {/if}
        </p>
      {/if}

      <div class="lenshead"><span class="ttl">1545 nm model</span><span class="muted small">1500–1550 nm · {ohc1500 ? 'OH-corrected' : 'uncorrected'}</span></div>
      <div class="plotlens" class:on={views && usable}>
        {#if views && usable}
          <SpectrumPlot startNm={views.startNm} series={lens1500} xRange={range1500} zero
            shades={[{ lo: 1500, hi: 1550, kind: 'window' }, ...(band1545 && ohc1500 ? [{ lo: 1543, hi: 1547, kind: 'band' as const, state: band1545.state, flagged }] : [])]} small axes xTicks={[1500, 1525, 1550]} refLabelsIn={[1500, 1525]}
            cornerLabel={flagged ? 'coating sign' : undefined} {themeKey} ariaLabel="1545 nm model window (reads 1500–1550 nm)" />
        {/if}
      </div>
      {#if usable && scan.models.wc1500}
        <p>1545 nm, OH-corrected reads <b>{formatModel(scan.models.wc1500.value)}</b>.{#if !ohc1500}
            <span class="unavail">OH-corrected view not available: {win1500?.reason ?? 'no OH/water direction for this model'}.</span>{/if}</p>
      {/if}
    </div>
  </div>

  <div class="legend">
    <span class="me"><i class="scanline"></i>{shortName(scan.file)}</span>
    <span class="spacer"></span>
    <span title="Mean spectra of the Ryder et al. 2026 reference bones at 0, 1, 3, 6 and 10% collagen">
      <i class="ramp">{#each RAMP as c (c)}<b style="border-color: var({c})"></b>{/each}</i>Reference bones 0 → 10% collagen
    </span>
    <span><i class="sw lit"></i>Collagen band (lit)</span>
    <span><i class="sw win"></i>Model window</span>
    <span><i class="sw hatch"></i>OH/water band</span>
    {#if heatShades.length}<span><i class="sw heat"></i>Heat sign</span>{/if}
  </div>
</section>

<style>
  .chartcard {
    display: flex;
    flex-direction: column;
    min-height: 360px;
    flex: 1 0 auto;
  }
  @media (max-height: 780px) {
    .chartcard {
      min-height: 320px;
    }
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--line-soft);
    flex-wrap: wrap;
  }
  .sg {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .sg select:disabled {
    opacity: 0.45;
  }
  .chainnote {
    font-size: var(--fs-xs);
    color: var(--accent);
  }
  .chips {
    display: inline-flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip .dash {
    width: 16px;
    height: 0;
    border-top: 3px dashed currentColor;
    border-radius: 1px;
  }
  .chip[aria-pressed='true'] {
    background: var(--panel-2);
  }
  .chip[aria-pressed='false'] {
    text-decoration: line-through;
    text-decoration-color: var(--ink-4);
  }
  .chip[aria-pressed='false'] .dash {
    opacity: 0.25;
  }
  .chartwrap {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 252px;
    flex: 1;
    min-height: 270px;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 10px 6px 4px 8px;
  }
  .ylabel {
    font-size: var(--fs-xs);
    color: var(--ink-3);
    padding-left: 46px;
  }
  .plotbox {
    position: relative;
    flex: 1;
    min-height: 200px;
  }
  .lens {
    border-left: 1px solid var(--line-soft);
    background: var(--panel-2);
    padding: 12px 12px 10px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-height: 0;
  }
  .lenshead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0 8px;
    white-space: nowrap;
  }
  .ttl {
    font-size: var(--fs-sm);
    font-weight: 650;
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  /* the same framed-window mark the main chart uses, so the close-up reads as "this window, zoomed" */
  .ttl::before {
    content: '';
    width: 10px;
    height: 12px;
    border-inline: 1.5px solid var(--window-edge);
    background: var(--window);
    flex: none;
  }
  .small {
    font-size: var(--fs-xs);
  }
  .seg.mini {
    align-self: flex-start;
  }
  .plotlens {
    position: relative;
    flex: 1;
    min-height: 92px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
  }
  .plotlens.on {
    background: var(--chart);
    border-color: var(--line-soft);
  }
  .lens p {
    margin: 0 0 6px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
    line-height: 1.4;
  }
  .lens p .unavail {
    display: block;
    margin-top: 3px;
    color: var(--ink-4);
  }
  .lens p b {
    color: var(--ink);
    font-weight: 650;
  }
  .legend {
    display: flex;
    gap: 6px 15px;
    flex-wrap: wrap;
    padding: 7px 14px 8px;
    font-size: var(--fs-xs);
    color: var(--ink-2);
    border-top: 1px solid var(--line-soft);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .legend .me {
    color: var(--ink);
    font-weight: 650;
    font-size: var(--fs-sm);
  }
  .legend i {
    display: inline-block;
    flex: none;
  }
  .legend i.scanline {
    width: 18px;
    height: 3px;
    border-radius: 2px;
    background: var(--s1);
    box-shadow: 0 0 6px var(--s1);
  }
  .legend i.ramp {
    display: inline-flex;
    gap: 2px;
  }
  .legend i.ramp b {
    width: 7px;
    border-top: 2.5px solid;
    border-radius: 1px;
  }
  .legend i.sw {
    height: 11px;
  }
  .legend i.lit {
    width: 6px;
    background: linear-gradient(var(--band) 0 3px, var(--band-soft) 3px);
  }
  .legend i.win {
    width: 14px;
    background: var(--window);
    box-shadow: inset 1.5px 0 var(--window-edge), inset -1.5px 0 var(--window-edge);
  }
  .legend i.heat {
    width: 14px;
    background: color-mix(in srgb, var(--flag) 18%, transparent);
    box-shadow: inset 1.5px 0 var(--flag), inset -1.5px 0 var(--flag);
  }
  .heatbtn {
    font-size: var(--fs-xs);
    color: var(--flag-ink);
    border: 1px solid var(--flag-edge);
    background: var(--flag-tint);
    border-radius: var(--r-sm);
    padding: 2px 8px;
  }
  .legend i.hatch {
    width: 14px;
    background: repeating-linear-gradient(45deg, var(--ink-4) 0 1.2px, transparent 1.2px 4px);
  }
  @media (max-width: 1366px) {
    .chartwrap {
      grid-template-columns: minmax(0, 1fr) 216px;
    }
  }
</style>
