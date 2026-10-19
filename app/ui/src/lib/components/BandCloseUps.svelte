<script lang="ts">
  // The collagen-band close-ups: the one place to see every band (Matt, 2026-10-18). All seven bands the rules read, each
  // on the curve its rule reads (the SG 31 evidence kernel; the OH-corrected curve for 1545*, where collagen is a dip), on
  // a y-scale fixed from the 10% reference, so a flat band cannot be blown up to look lit.
  // Peak distinctness comes first (Matt: in the old right-hand pane "even when there was a signal you could not see
  // well"): each window is cut to its band's own shape, drawn at about v0.3's horizontal scale (never stretched: wider
  // peaks look flatter) and the room goes into height.
  // Three parts, placed by the hero card's grid (App.svelte): the summary (the protein-band level and the ZooMS band
  // check), the row of close-ups, and the legend with the technical notes one click away. Beside the verdict when the main
  // column has room (laptops from 1280 px), else the close-ups go under it.
  import EvidenceGlyph from './EvidenceGlyph.svelte';
  import FlagMark from './FlagMark.svelte';
  import SpectrumPlot, { type PlotSeries, type Shade } from './SpectrumPlot.svelte';
  import VerdictGlyph from './VerdictGlyph.svelte';
  import { CLOSEUPS, OHC_NOTE, PAIR_SEP, bandAt, bandTitle, closeUpSpan, placeNumbers, type CloseUp, type CloseUpWindow } from '../bands';
  import { LEVEL_WORD, keepRanges, zoomsHeadline } from '../display';
  import { flagLabel } from '../strings';
  import { verdictMeta } from '../verdict';
  import type { BandReading, ReferenceSpectrum, ScanResult, SpectrumViews } from '../types';

  interface Props {
    scan: ScanResult;
    views: SpectrumViews | null;
    refs: ReferenceSpectrum[];
    themeKey: string;
    highlight: number;
    /** A band label is hovered or focused: its index in `scan.evidence.bands` (-1: none), for the main spectrum. */
    onHighlight: (k: number) => void;
    /** Conventional 2nd derivative (bands down). */
    bandsDown?: boolean;
  }
  let { scan, views, refs, themeKey, highlight, onHighlight, bandsDown = false }: Props = $props();

  const ref3 = $derived(refs.find((r) => r.label === '3%'));
  const ref10 = $derived(refs.find((r) => r.label === '10%'));
  const fired = $derived(scan.signs.filter((s) => s.fired && s.id !== 'burnt'));
  const flagged = $derived(fired.length > 0);
  const readable = $derived(scan.evidence.bands.length > 0);

  interface Item {
    /** Index in scan.evidence.bands (the main spectrum's highlight). */
    k: number;
    b: BandReading;
  }
  const itemsOf = (w: CloseUpWindow): Item[] =>
    w.bands
      .map((id) => scan.evidence.bands.findIndex((b) => b.id === id))
      .filter((k) => k >= 0)
      .map((k) => ({ k, b: scan.evidence.bands[k] }));
  /** The close-ups with the bands this scan has (a profile without the 1545 band shows three). */
  const groups = $derived(
    CLOSEUPS.map((cu) => ({ cu, wins: cu.windows.map((w) => ({ w, items: itemsOf(w) })).filter((x) => x.items.length > 0) })).filter(
      (g) => g.wins.length > 0,
    ),
  );

  // Horizontal scale: about v0.3's (0.75 px/nm at 1280 px) and never more than PX_PER_NM_MAX, since stretching a peak
  // sideways makes its sides shallower. Each close-up grows with the nm it shows, up to that cap; its minimum holds its
  // title and keeps its labels centred over their bands inside the box.
  const PX_PER_NM_MAX = 0.95;
  const CHROME = 10; // a close-up's padding and border
  const BREAK = 12; // an axis break
  const minWidth = (cu: CloseUp) => (cu.dip ? 58 : cu.windows.length > 1 ? 120 : 86);
  /** In a narrow hero card (App.svelte) the titles shorten, so a close-up can be as narrow as v0.3's were, but every
   *  close-up keeps its one label line inside its box: a pair label ("1689 · 1728", "2262 · 2284") or "1545*" over its
   *  band near the window's end. */
  const minNarrow = (cu: CloseUp) => (cu.dip ? 56 : cu.windows.length > 1 ? 70 : 62);
  const maxWidth = (cu: CloseUp) => Math.round(closeUpSpan(cu) * PX_PER_NM_MAX + CHROME + (cu.windows.length - 1) * BREAK);
  const cellStyle = (cu: CloseUp) =>
    `--grow: ${closeUpSpan(cu)}; --minw: ${minWidth(cu)}px; --minw-narrow: ${minNarrow(cu)}px; --maxw: ${Math.max(minWidth(cu), maxWidth(cu))}px`;

  function seriesFor(view: CloseUp['view']): PlotSeries[] {
    const d = views?.views[view];
    if (!d) return [];
    const r = ref3?.spectra.views[view];
    const out: PlotSeries[] = [];
    if (r) out.push({ label: '3% reference', data: r, color: '--r3', width: 1.3, dash: [4, 2.5] });
    out.push({ label: scan.file, data: d, color: '--s1', width: 2, halo: true });
    return out;
  }
  const series = $derived({ D2_31: seriesFor('D2_31'), D2_31_ohc: seriesFor('D2_31_ohc') });
  // the y range from the 10% reference and the scan, in the same view as the curve
  const yFrom = $derived.by(() => {
    const y = (v: CloseUp['view']) => {
      const a = ref10?.spectra.views[v];
      const b = views?.views[v];
      return a && b ? [a, b] : undefined;
    };
    return { D2_31: y('D2_31'), D2_31_ohc: y('D2_31_ohc') };
  });
  // The scale floor (Matt, DECISIONS 125): for the six plain bands the side away from the bands follows the drawn curves
  // (the scan and the 3% reference), so a lit band fills more of its plot; the band side stays fixed by the 10% reference,
  // so a flat band still reads flat. Not for the 1545 dip: there the floor is the band's own direction.
  const yAway = $derived.by(() => {
    const a = ref3?.spectra.views.D2_31;
    const b = views?.views.D2_31;
    return a && b ? [a, b] : undefined;
  });

  function shadesFor(cu: CloseUp, items: Item[]): Shade[] {
    // the end a lit band points to: up for the six with bands up, down for the 1545 dip; Bands down flips both
    const toward = !!cu.dip !== bandsDown ? 'down' : 'up';
    return items.map(({ k, b }) => ({
      lo: b.nm - 2,
      hi: b.nm + 2,
      kind: 'band',
      state: b.state,
      flagged,
      highlight: highlight === k,
      toward,
      arrow: !!cu.dip,
    }));
  }

  // The label lane over the plots (Matt, 2026-10-19: numbers staggered on two lines read as "a ragged staircase of
  // numbers, not as labels"): one line of numbers in every close-up, the evidence glyphs on the line under it, the same
  // two lines in every close-up at every width. Each number is centred on its band; a pair too close for two numbers side
  // by side (2262/2284 always, 1689/1728 in a narrow close-up) is set as one label "2262 · 2284" centred on the pair, each
  // number still its own button, each glyph still on its own band (placeNumbers). The 1545 asterisk hangs to the right of
  // its number, so the number stays centred.
  /** A band's x over its window: the plot area is inset 4 px each side (small plot, no axes). */
  const INSET = 4;
  const tagLeft = (w: CloseUpWindow, nm: number) => `calc(${INSET}px + (100% - ${2 * INSET}px) * ${bandAt(w, nm).toFixed(4)})`;
  const wkey = (w: CloseUpWindow) => w.bands.join('-');
  /** Measured widths: each window (px) and each band's number (px), for placing the numbers. */
  let winW = $state<Record<string, number>>({});
  let nmW = $state<Record<string, number>>({});
  function place(w: CloseUpWindow, items: Item[]) {
    const ww = winW[wkey(w)] ?? 0;
    const xs = items.map((it) => INSET + bandAt(w, it.b.nm) * (ww - 2 * INSET));
    return placeNumbers(
      xs,
      items.map((it) => nmW[it.b.id] || 22),
    );
  }

  const zcall = $derived(scan.zooms.verdict ?? 'cant_tell');
  const headline = $derived(zoomsHeadline(scan.zooms, scan.evidence.bands));
  const LEVEL_EXPLAIN: Record<string, string> = {
    none: 'No protein signal detected. Significant collagen is very unlikely at this spot.',
    trace: 'Faint organic signal. Some organic matter is present, but not much protein.',
    clear: 'Protein signal present. See the collagen models for how much.',
    strong: 'Strong protein signal. See the collagen models for how much.',
    cant_tell: 'Too noisy to tell at this spot. A lighter or cleaner area, or more averages, may scan better.',
  };
  const ZOOMS_EXPLAIN =
    'The six collagen band patterns, with the 1545 nm band as a vote. Supporting evidence only: it never changes the verdict. Where it calls a scan better than the verdict, a ZooMS line shows under the verdict.';
</script>

<div class="bandsum">
  <div class="head">
    <h2>Collagen bands</h2>
    <span class="spacer"></span>
    {#if readable}
      <span class="level" title="{LEVEL_EXPLAIN[scan.evidence.level]} The level comes from the band rules; it is not a percentage.">
        <span class="muted lw">Protein bands</span><EvidenceGlyph state={scan.evidence.level} size={16} /><b>{LEVEL_WORD[scan.evidence.level]}</b>
      </span>
    {/if}
  </div>
  {#if readable}
    <div class="zrow" data-testid="zooms-check" title={ZOOMS_EXPLAIN}>
      <VerdictGlyph verdict={zcall} size={14} />
      <span><b>ZooMS band check: {verdictMeta(zcall).short}</b> <span class="zh">· {headline}</span></span>
    </div>
    {#if flagged}
      <div class="cuflag"><FlagMark size={13} /><span><b>{flagLabel(fired[0].id)}:</b> lit bands may come from {fired[0].id === 'wax' ? 'a lipid (such as wax)' : 'the coating'}, not collagen.</span></div>
    {/if}
  {:else}
    <p class="none muted">No band reading for this scan.</p>
  {/if}
</div>

{#if readable}
  <div class="bandrow">
    {#each groups as g, gi (g.cu.family)}
      {@const cu = g.cu}
      <div class="cu" class:dip={cu.dip} style={cellStyle(cu)}>
        <div class="cut">{#if cu.short}<span class="long">{cu.family}</span><span class="short" title="{cu.family} bands">{cu.short}</span>{:else}{cu.family}{/if}</div>
        <div class="wins">
          {#each g.wins as { w, items }, wi (wkey(w))}
            {@const pl = place(w, items)}
            {#if wi > 0}
              {@const prev = g.wins[wi - 1].w}
              <div class="brk" title="Axis break: {prev.range[1]}–{w.range[0]} nm is left out (it carries neither band)" aria-hidden="true">
                <svg width="10" height="14" viewBox="0 0 10 14"><path d="M1 10 L5 2 M5 12 L9 4" /></svg>
              </div>
            {/if}
            <div class="win" style="flex: {w.range[1] - w.range[0]} 1 0" data-range="{w.range[0]}-{w.range[1]}" bind:clientWidth={winW[wkey(w)]}>
              <div class="tags">
                {#each items as it, i (it.b.id)}
                  <button class="tag" class:quiet={it.b.id === 'ch2284'} class:hl={highlight === it.k} style="left: {tagLeft(w, it.b.nm)}"
                    aria-label={bandTitle(it.b)} data-band={it.b.id} data-nm={it.b.nm}
                    onmouseenter={() => onHighlight(it.k)} onmouseleave={() => onHighlight(-1)} onfocus={() => onHighlight(it.k)} onblur={() => onHighlight(-1)}>
                    <span class="nm" style="--shift: {pl.shift[i].toFixed(2)}px" bind:offsetWidth={nmW[it.b.id]}>{it.b.nm}{#if it.b.id === 'nh1545'}<span class="ast">*</span>{/if}</span>
                    <span class="g"><EvidenceGlyph state={it.b.state} size={10} /></span>
                    <!-- the tooltip, on hover and on keyboard focus (the label carries the same text for screen readers) -->
                    <span class="tip" class:end={gi >= groups.length / 2} aria-hidden="true">{keepRanges(bandTitle(it.b))}</span>
                  </button>
                {/each}
                {#if pl.sepAt !== null}
                  <span class="sep" style="left: {pl.sepAt.toFixed(2)}px" aria-hidden="true">{PAIR_SEP.trim()}</span>
                {/if}
              </div>
              <div class="cuplot">
                {#if views && series[cu.view].length}
                  <SpectrumPlot startNm={views.startNm} series={series[cu.view]} xRange={w.range} yFrom={yFrom[cu.view]}
                    yAwayFrom={cu.dip ? undefined : yAway} zero flip={bandsDown}
                    shades={shadesFor(cu, items)} small axes={false} labelRoom={false} {themeKey} ariaLabel="{cu.family} close-up, {w.range[0]}–{w.range[1]} nm" />
                {:else}
                  <div class="empty muted">—</div>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/each}
  </div>

  <div class="bandfoot">
    <span class="legend">
      <span><EvidenceGlyph state="trace" size={10} /><EvidenceGlyph state="clear" size={10} /><EvidenceGlyph state="strong" size={10} />strength</span>
      <span><EvidenceGlyph state="flat" size={10} />flat</span>
      <span><EvidenceGlyph state="cant_tell" size={10} />too noisy</span>
      <span><i class="refdash"></i>3% reference</span>
    </span>
    <details class="about">
      <summary>About these bands</summary>
      <ul>
        <li>The curves are what the band rules read: {bandsDown ? 'd²A/dλ² (bands down)' : '−d²A/dλ² (bands up)'}, SG 31, each band read over ±2 nm.
          The band side of each plot is fixed by the 10% reference, so a flat band cannot be blown up to look lit; the other side follows the
          drawn curves (for 1545*, the 10% reference sets both). Each close-up shows only its bands and their shoulders; // marks a stretch
          left out.</li>
        <li>1545*: {OHC_NOTE}{bandsDown ? ' With bands down the dip is drawn as a peak.' : ''} A smaller rise near 1526 nm comes first; the corrected
          curve ends at 1550 nm.</li>
        <li>2044 nm sits on the OH/water band shoulder, so it can only add evidence.</li>
        <li>2284 nm (amide III + CH₂) counts for the ZooMS band pattern only, not for the level.</li>
        <li>The level (None, Trace, Clear, Strong or Can't tell) comes from the band rules; it is not a percentage.</li>
        <li>ZooMS band check: {ZOOMS_EXPLAIN}</li>
      </ul>
    </details>
  </div>
{/if}

<style>
  .bandsum {
    grid-area: sum;
    padding: 14px 16px 4px;
    display: flex;
    flex-direction: column;
    gap: 7px;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
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
    white-space: nowrap;
  }
  .level b {
    font-weight: 650;
    font-size: var(--fs-md);
  }
  .zrow {
    display: grid;
    grid-template-columns: 14px 1fr;
    gap: 7px;
    align-items: start;
    font-size: var(--fs-sm);
    color: var(--ink-2);
    text-wrap: pretty;
  }
  .zrow :global(svg) {
    margin-top: 2px;
  }
  .zrow b {
    font-weight: 600;
    color: var(--ink);
  }
  .none {
    margin: 0;
    font-size: var(--fs-sm);
  }
  .cuflag {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: var(--fs-sm);
    color: var(--flag-ink);
    text-wrap: pretty;
  }
  .cuflag b {
    font-weight: 650;
  }
  /* the close-ups: tall plots, each as wide as the nm it shows, capped near v0.3's horizontal scale; one row, or two rows
     of four, as narrow as v0.3's close-ups in a narrow hero card */
  .bandrow {
    --line-h: 14px;
    --lane: calc(var(--line-h) + 12px);
    grid-area: plots;
    display: flex;
    gap: 6px;
    padding: 6px 14px 4px;
    min-width: 0;
  }
  .cu {
    flex: var(--grow) 1 0px;
    min-width: var(--minw);
    max-width: var(--maxw);
    display: flex;
    flex-direction: column;
    border: 1px solid var(--line-soft);
    border-radius: var(--r-md);
    padding: 4px 4px 2px;
    background: var(--chart);
  }
  .cut {
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--fs-xs);
    font-weight: 650;
    color: var(--ink-2);
    letter-spacing: 0.01em;
  }
  .cut .short {
    display: none;
  }
  .wins {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .win {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  /* an axis break between two windows: a gap with two slashes, so nobody reads the curve as continuous */
  .brk {
    flex: none;
    width: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: repeating-linear-gradient(to bottom, var(--line) 0 3px, transparent 3px 7px) center / 1px 100% no-repeat;
  }
  .brk svg {
    background: var(--chart);
    padding: 2px 0;
  }
  .brk path {
    fill: none;
    stroke: var(--ink-3);
    stroke-width: 1.4;
    stroke-linecap: round;
  }
  /* the label lane: one line of numbers, each centred on its band (a close pair as one label "a · b" centred on the
     pair), and under it each band's state glyph, centred on the band's column; the same two lines in every close-up at
     every width, so titles, numbers and glyphs share their baselines across the row */
  .tags {
    position: relative;
    height: var(--lane);
    margin-top: 2px;
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    line-height: var(--line-h);
    white-space: nowrap;
  }
  /* a band's button: a zero-width anchor on the band's x; its number and glyph hang from it, so the hit area is exactly
     what is drawn */
  .tag {
    position: absolute;
    top: 0;
    width: 0;
    height: var(--lane);
    padding: 0;
    border: 0;
    background: transparent;
    font: inherit;
    color: var(--ink-2);
    cursor: default;
  }
  .tag:focus-visible {
    outline: none;
  }
  .nm {
    position: absolute;
    top: 0;
    left: 0;
    transform: translateX(calc(-50% + var(--shift, 0px)));
    isolation: isolate;
  }
  /* the hover / highlight pill sits behind the number without changing its width (the width places the numbers) */
  .nm::before {
    content: '';
    position: absolute;
    inset: 0 -3px;
    z-index: -1;
    border-radius: 999px;
  }
  .tag:hover .nm,
  .tag:focus-visible .nm,
  .tag.hl .nm {
    color: var(--ink);
  }
  .tag:hover .nm::before,
  .tag:focus-visible .nm::before,
  .tag.hl .nm::before {
    background: var(--band-soft);
  }
  .tag:focus-visible .nm::before {
    outline: 1.5px solid var(--accent);
    outline-offset: 1px;
  }
  /* the 1545 asterisk hangs to the right, so the number itself stays centred over the band */
  .ast {
    position: absolute;
    left: 100%;
    top: 0;
  }
  .g {
    position: absolute;
    left: 0;
    top: calc(var(--line-h) + 1px);
    transform: translateX(-50%);
    line-height: 0;
  }
  /* the pair label's separator, on the pair's midpoint */
  .sep {
    position: absolute;
    top: 0;
    transform: translateX(-50%);
    color: var(--ink-3);
    pointer-events: none;
  }
  .brk {
    margin-top: calc(var(--lane) + 4px);
  }
  /* 2284 counts for the ZooMS pattern only: quieter */
  .tag.quiet .nm {
    color: var(--ink-3);
    font-weight: 500;
  }
  .tag.quiet.hl .nm,
  .tag.quiet:hover .nm {
    color: var(--ink);
  }
  /* the band's tooltip: shown on hover and keyboard focus (a native title shows on hover only) */
  .tip {
    display: none;
    position: absolute;
    top: calc(var(--lane) + 2px);
    left: -8px;
    z-index: 6;
    width: max-content;
    max-width: 230px;
    white-space: normal;
    text-wrap: pretty;
    padding: 6px 8px;
    border-radius: 8px;
    background: var(--panel);
    border: 1px solid var(--line);
    box-shadow: var(--shadow);
    color: var(--ink-2);
    font-size: var(--fs-xs);
    font-weight: 400;
    font-variant-numeric: normal;
    line-height: 1.4;
    text-align: left;
    pointer-events: none;
  }
  .tip.end {
    left: auto;
    right: -8px;
  }
  .tag:hover .tip,
  .tag:focus .tip {
    display: block;
  }
  /* tall plots: at least as tall as v0.3's close-ups (the y-scale stays fixed from the 10% reference); beside the
     verdict they take whatever height the verdict card gives */
  .cuplot {
    position: relative;
    flex: 1;
    min-height: 170px;
  }
  .empty {
    display: grid;
    place-items: center;
    height: 100%;
    font-size: var(--fs-sm);
  }
  .bandfoot {
    grid-area: foot;
    display: flex;
    align-items: flex-start;
    gap: 4px 12px;
    flex-wrap: wrap;
    padding: 4px 16px 10px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
    min-width: 0;
  }
  .legend {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 3px 11px;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .about summary {
    cursor: pointer;
    list-style: none;
    text-decoration: underline dotted;
    text-underline-offset: 2px;
    white-space: nowrap;
  }
  .about summary::-webkit-details-marker {
    display: none;
  }
  .about[open] {
    flex-basis: 100%;
  }
  .about ul {
    margin: 4px 0 0;
    padding-left: 16px;
    color: var(--ink-2);
    line-height: 1.4;
    text-wrap: pretty;
  }
  .refdash {
    display: inline-block;
    width: 14px;
    border-top: 2px dashed var(--r3);
    margin-right: 3px;
  }
  @media (max-width: 1300px) {
    .bandsum {
      padding: 11px 12px 4px;
      gap: 6px;
    }
    .bandrow {
      padding: 4px 10px 2px;
    }
    .bandfoot {
      padding: 4px 12px 9px;
    }
    .lw {
      display: none;
    }
  }
</style>
