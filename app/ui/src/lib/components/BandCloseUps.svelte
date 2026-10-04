<script lang="ts">
  // The three evidence close-ups (the visual models). They plot the evidence kernel (SG 31, the
  // curve the rule reads), not the display smoothing, on a y-scale fixed from the 10% reference, so a
  // flat band cannot be blown up to look lit and the shading matches the ZooMS call exactly.
  import EvidenceGlyph from './EvidenceGlyph.svelte';
  import FlagMark from './FlagMark.svelte';
  import SpectrumPlot, { type PlotSeries, type Shade } from './SpectrumPlot.svelte';
  import { CLOSEUPS } from '../bands';
  import { LEVEL_WORD, zoomsHeadline } from '../display';
  import { flagLabel } from '../strings';
  import type { ReferenceSpectrum, ScanResult, SpectrumViews } from '../types';

  interface Props {
    scan: ScanResult;
    views: SpectrumViews | null;
    refs: ReferenceSpectrum[];
    themeKey: string;
    highlight: number;
  }
  let { scan, views, refs, themeKey, highlight }: Props = $props();

  const ref3 = $derived(refs.find((r) => r.label === '3%'));
  const ref10 = $derived(refs.find((r) => r.label === '10%'));
  const fired = $derived(scan.signs.filter((s) => s.fired && s.id !== 'burnt'));
  const flagged = $derived(fired.length > 0);

  const series = $derived.by((): PlotSeries[] => {
    if (!views) return [];
    const out: PlotSeries[] = [];
    if (ref3) out.push({ label: '3% reference', data: ref3.spectra.views.D2_31, color: '--r3', width: 1.3, dash: [4, 2.5] });
    out.push({ label: scan.file, data: views.views.D2_31, color: '--s1', width: 2, halo: true });
    return out;
  });
  const yFrom = $derived(views && ref10 ? [ref10.spectra.views.D2_31, views.views.D2_31] : undefined);

  function shadesFor(i: number): Shade[] {
    return CLOSEUPS[i].bands.map((k) => {
      const b = scan.evidence.bands[k];
      return { lo: b.nm - 2, hi: b.nm + 2, kind: 'band', state: b.state, glyph: true, flagged, highlight: highlight === k };
    });
  }
  const headline = $derived(
    scan.evidence.bands.length === 0 ? 'not readable' : zoomsHeadline(scan.zooms, scan.evidence.bands),
  );
</script>

<div class="closeups">
  <div class="head">
    <h2>Collagen bands</h2>
    <span class="spacer"></span>
    {#if scan.evidence.bands.length}
      <span class="level" title="How clearly the collagen bands show in this spectrum. A level, not a percentage.">
        <span class="muted">Protein bands</span><EvidenceGlyph state={scan.evidence.level} size={16} /><b>{LEVEL_WORD[scan.evidence.level]}</b>
      </span>
    {/if}
  </div>
  <div class="subrow">
    <span class="sub">{headline}</span>
    <span class="spacer"></span>
    <span class="muted kern">−d²A/dλ², SG 31, read ±2 nm</span>
  </div>
  <div class="row">
    {#each CLOSEUPS as cu, i (cu.title)}
      <div class="cu" style="flex: {i === 1 ? 1.3 : 1}">
        <div class="cut">{cu.title}</div>
        <div class="cuplot">
          {#if views && series.length && scan.evidence.bands.length}
            <SpectrumPlot startNm={views.startNm} series={series} xRange={cu.range} {yFrom} zero shades={shadesFor(i)}
              small axes={false} {themeKey} ariaLabel="{cu.title} close-up" />
          {:else}
            <div class="empty muted">—</div>
          {/if}
        </div>
      </div>
    {/each}
  </div>
  {#if flagged}
    <div class="cuflag"><FlagMark size={13} /><span><b>{flagLabel(fired[0].id)}:</b> lit bands may come from the coating, not collagen.</span></div>
  {:else if scan.evidence.bands.length}
    <div class="legend">
      <span><EvidenceGlyph state="trace" size={11} /><EvidenceGlyph state="clear" size={11} /><EvidenceGlyph state="strong" size={11} />fill = band strength</span>
      <span><EvidenceGlyph state="flat" size={11} />flat</span>
      <span><EvidenceGlyph state="cant_tell" size={11} />too noisy</span>
      <span><i class="refdash"></i>3% collagen reference</span>
    </div>
  {/if}
</div>

<style>
  .closeups {
    padding: 16px 18px 12px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    font-size: var(--fs-lg);
    font-weight: 650;
  }
  .level {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 10px 2px 9px;
    border-radius: 999px;
    background: var(--panel-2);
    box-shadow: inset 0 0 0 1px var(--line);
    font-size: var(--fs-sm);
  }
  .level .muted {
    margin-right: 2px;
  }
  .level b {
    font-weight: 650;
    font-size: var(--fs-md);
  }
  .subrow {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex-wrap: wrap;
    margin-top: -4px;
  }
  .sub {
    font-size: var(--fs-sm);
    color: var(--ink-2);
  }
  .kern {
    font-size: var(--fs-xs);
  }
  .row {
    display: flex;
    gap: 10px;
    flex: 1;
    min-height: 124px;
  }
  .cu {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    border: 1px solid var(--line-soft);
    border-radius: var(--r-md);
    padding: 6px 8px 4px;
    background: var(--chart);
  }
  .cut {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--ink-2);
  }
  .cuplot {
    position: relative;
    flex: 1;
    min-height: 100px;
  }
  .empty {
    display: grid;
    place-items: center;
    height: 100%;
  }
  .legend {
    font-size: var(--fs-xs);
    color: var(--ink-3);
    display: flex;
    flex-wrap: wrap;
    gap: 3px 12px;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .legend span :global(svg) {
    margin-right: 1px;
  }
  @media (max-width: 1300px) {
    .closeups {
      padding: 12px 12px 10px;
      gap: 7px;
    }
    .kern {
      display: none;
    }
    .cu {
      padding: 5px 6px 3px;
    }
    .cut {
      font-size: 10.5px;
    }
  }
  .refdash {
    display: inline-block;
    width: 14px;
    border-top: 2px dashed var(--r3);
    margin-right: 3px;
  }
  .cuflag {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: var(--fs-sm);
    color: var(--flag-ink);
  }
  .cuflag b {
    font-weight: 650;
  }
</style>
