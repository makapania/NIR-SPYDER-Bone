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
  import {
    BANDS,
    BAND_1545,
    CLOSEUPS,
    LABEL_FITS,
    OHC_NOTE,
    PAIR_SEP,
    PAIR_SEP_PAD_EM,
    PLOT_INSET,
    bandAt,
    bandTitle,
    bandX,
    closeUpLabelWidth,
    closeUpSpan,
    placeRow,
    type CloseUp,
    type CloseUpFrame,
    type CloseUpWindow,
    type LabelText,
    type LabelTexts,
    type LabelWindow,
    type Placement,
  } from '../bands';
  import { LEVEL_WORD, keepRanges, zoomsHeadline } from '../display';
  import { flagLabel } from '../strings';
  import { verdictMeta } from '../verdict';
  import type { BandId, BandReading, ReferenceSpectrum, ScanResult, SpectrumViews } from '../types';

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
  /** A close-up's frame (padding, border, axis break; the CSS below), and in a narrow hero card (App.svelte's container
   *  query: 2 px side padding, 8 px breaks). */
  const FRAME: CloseUpFrame = { pad: 4, border: 1, brk: 12 };
  const FRAME_NARROW: CloseUpFrame = { pad: 2, border: 1, brk: 8 };
  const chrome = (f: CloseUpFrame) => 2 * (f.pad + f.border);
  /** Floors as tuned on Segoe UI (the approved look on Windows); the text a close-up holds is measured in the rendered font
   *  (`needs`), and a wider font (SF Pro on a Mac, a Linux fallback) widens the box to fit it. */
  const minWidth = (cu: CloseUp) => (cu.dip ? 58 : cu.windows.length > 1 ? 120 : 86);
  /** In a narrow hero card (App.svelte) the titles shorten, so a close-up can be as narrow as v0.3's were, but every
   *  close-up keeps its one label line inside its box: a pair label ("1689 · 1728", "2262 · 2284") or "1545*" over its
   *  band near the window's end, at the smallest label type if need be. */
  const minNarrow = (cu: CloseUp) => (cu.dip ? 56 : cu.windows.length > 1 ? 70 : 62);
  const maxWidth = (cu: CloseUp) => Math.round(closeUpSpan(cu) * PX_PER_NM_MAX + chrome(FRAME) + (cu.windows.length - 1) * FRAME.brk);
  const mins = (cu: CloseUp) => {
    const n = needs[cu.family];
    return { wide: Math.max(minWidth(cu), n?.wide ?? 0), narrow: Math.max(minNarrow(cu), n?.narrow ?? 0), full: n?.full ?? 0 };
  };
  function cellStyle(cu: CloseUp) {
    const m = mins(cu);
    return `--grow: ${closeUpSpan(cu)}; --minw: ${m.wide}px; --minw-narrow: ${m.narrow}px; --maxw: ${Math.max(m.wide, m.narrow, m.full, maxWidth(cu))}px`;
  }

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
  // by side (2262/2284 always, 1689/1728 in a narrow close-up or a wide font) is set as one label "2262 · 2284" centred
  // on the pair, each number still its own button, each glyph still on its own band. The 1545 asterisk hangs to the right
  // of its number, so the number stays centred.
  // Everything is decided from widths measured in the font the app is drawn in (the hidden measurer below): Segoe UI,
  // SF Pro and a Linux fallback differ by a fifth, so nothing is tuned to one font. They are measured again whenever a
  // measured text or a close-up changes size (a font arriving late, a resize). If the numbers still do not fit, the row's
  // label type tightens a little, then shrinks a little (LABEL_FITS); never over the box edge (placeRow).
  const tagLeft = (w: CloseUpWindow, nm: number) => `calc(${PLOT_INSET}px + (100% - ${2 * PLOT_INSET}px) * ${bandAt(w, nm).toFixed(4)})`;
  const wkey = (w: CloseUpWindow) => w.bands.join('-');
  const MEASURED_BANDS = [BAND_1545, ...BANDS];
  let rowEl = $state<HTMLDivElement>();
  let measurer = $state<HTMLDivElement>();
  /** The label texts' widths in the rendered font, at the design size (LABEL_FITS[0]), and the titles' widths. */
  let texts = $state<LabelTexts | null>(null);
  let titleW = $state<Record<string, number>>({});
  /** Each window's width and the stretch its numbers may use (px, from the window's left edge): to the close-up's inner
   *  edges, or halfway into an axis break. */
  let rooms = $state<Record<string, { ww: number; room: [number, number] }>>({});
  /** The hero card around the row (App.svelte): its inner width, the verdict's minimum beside the close-ups (0: the row is
   *  under the verdict anyway), the row's gaps and side padding, and whether the close-ups are in their narrow form. */
  let host = $state({ inner: 0, verdictMin: 0, gap: 0, pad: 0, narrow: false });

  function measure() {
    if (measurer) {
      const w = (e: Element | null) => (e ? e.getBoundingClientRect().width : 0);
      const nums: Partial<Record<BandId, LabelText>> = {};
      for (const e of measurer.querySelectorAll<HTMLElement>('[data-m-band]')) nums[e.dataset.mBand as BandId] = { w: w(e), n: e.textContent!.length };
      const t: LabelTexts = { nums, tail: { w: w(measurer.querySelector('[data-m-tail]')), n: 1 }, sep: { w: w(measurer.querySelector('[data-m-sep]')) + 2 * PAIR_SEP_PAD_EM * LABEL_FITS[0].fs, n: 1 } };
      if (JSON.stringify(t) !== JSON.stringify(texts)) texts = t;
      const tw: Record<string, number> = {};
      for (const e of measurer.querySelectorAll<HTMLElement>('[data-m-title]')) tw[e.dataset.mTitle!] = w(e);
      if (JSON.stringify(tw) !== JSON.stringify(titleW)) titleW = tw;
    }
    if (rowEl) {
      const out: typeof rooms = {};
      for (const cu of rowEl.querySelectorAll<HTMLElement>('.cu')) {
        const cs = getComputedStyle(cu);
        const b = cu.getBoundingClientRect();
        const inL = b.left + parseFloat(cs.borderLeftWidth);
        const inR = b.right - parseFloat(cs.borderRightWidth);
        const wins = [...cu.querySelectorAll<HTMLElement>('.win')];
        const rs = wins.map((e) => e.getBoundingClientRect());
        wins.forEach((e, i) => {
          const r = rs[i];
          const lo = i === 0 ? inL : (rs[i - 1].right + r.left) / 2;
          const hi = i === wins.length - 1 ? inR : (r.right + rs[i + 1].left) / 2;
          out[e.dataset.key!] = { ww: r.width, room: [lo - r.left, hi - r.left] };
        });
      }
      if (JSON.stringify(out) !== JSON.stringify(rooms)) rooms = out;
      const hero = rowEl.parentElement;
      if (hero) {
        const hs = getComputedStyle(hero);
        const rs = getComputedStyle(rowEl);
        const h = {
          inner: hero.getBoundingClientRect().width - parseFloat(hs.borderLeftWidth) - parseFloat(hs.borderRightWidth),
          verdictMin: parseFloat(hs.getPropertyValue('--verdict-min')) || 0,
          gap: parseFloat(rs.columnGap) || 0,
          pad: parseFloat(rs.paddingLeft) + parseFloat(rs.paddingRight),
          narrow: rs.getPropertyValue('--narrow').trim() === '1',
        };
        if (JSON.stringify(h) !== JSON.stringify(host)) host = h;
      }
    }
  }
  $effect(() => {
    void groups; // the close-ups shown (a scan without the 1545 band shows three): observe the ones on screen
    if (!rowEl || !measurer) return;
    const ro = new ResizeObserver(() => measure());
    for (const e of measurer.querySelectorAll('[data-m-band], [data-m-tail], [data-m-sep], [data-m-title]')) ro.observe(e);
    for (const e of rowEl.querySelectorAll('.cu, .win')) ro.observe(e);
    if (rowEl.parentElement) ro.observe(rowEl.parentElement);
    const fonts = document.fonts;
    fonts?.addEventListener?.('loadingdone', measure);
    fonts?.ready?.then(measure);
    measure();
    return () => {
      ro.disconnect();
      fonts?.removeEventListener?.('loadingdone', measure);
    };
  });

  /** The bands this scan has. */
  const present = $derived(new Set(scan.evidence.bands.map((b) => b.id)));
  /** The widths each close-up needs for its title and its label line, from the measured texts: at the design size where
   *  the card is roomy, at the smallest label type in a narrow hero card, and (`full`, for the maximum width) at the design
   *  size in the narrow form, so a close-up with room to grow never needs the smaller type. */
  const needs = $derived.by(() => {
    const out: Record<string, { wide: number; narrow: number; full: number }> = {};
    if (!texts) return out;
    const t: LabelTexts = { ...texts, nums: Object.fromEntries(Object.entries(texts.nums).filter(([id]) => present.has(id as BandId))) };
    for (const cu of CLOSEUPS) {
      const title = (s: string | undefined, f: CloseUpFrame) => Math.ceil((titleW[s ?? ''] ?? 0) + chrome(f) + 2);
      out[cu.family] = {
        wide: Math.max(closeUpLabelWidth(cu, t, FRAME, LABEL_FITS[0]), title(cu.family, FRAME)),
        narrow: Math.max(closeUpLabelWidth(cu, t, FRAME_NARROW, LABEL_FITS[LABEL_FITS.length - 1]), title(cu.short ?? cu.family, FRAME_NARROW)),
        full: closeUpLabelWidth(cu, t, FRAME_NARROW, LABEL_FITS[0]),
      };
    }
    return out;
  });

  /** The row's label type and each window's placement (placeRow). Before the first measurement: centred, design size. */
  const labels = $derived.by(() => {
    if (!texts) return null;
    const keys: string[] = [];
    const wins: LabelWindow[] = [];
    for (const g of groups)
      for (const { w, items } of g.wins) {
        const r = rooms[wkey(w)];
        if (!r) return null;
        keys.push(wkey(w));
        wins.push({
          nums: items.map((it) => ({ x: bandX(w, it.b.nm, r.ww), num: texts!.nums[it.b.id] ?? { w: 22, n: 4 }, tail: it.b.id === 'nh1545' ? texts!.tail : undefined })),
          room: r.room,
        });
      }
    const res = placeRow(wins, texts.sep);
    return { fit: LABEL_FITS[res.fit], at: Object.fromEntries(keys.map((k, i) => [k, res.placed[i]])) as Record<string, Placement> };
  });
  /** The close-ups at their minimum widths do not fit beside the verdict at its minimum (a wide font on a small window):
   *  App.svelte then puts them under the verdict, where they have the card's width. */
  const cramped = $derived.by(() => {
    if (!host.verdictMin || !host.inner) return false;
    const row = groups.reduce((t, g) => t + (host.narrow ? mins(g.cu).narrow : mins(g.cu).wide), 0) + (groups.length - 1) * host.gap + host.pad;
    return row > host.inner - host.verdictMin + 0.5;
  });
  const placeOf = (w: CloseUpWindow, items: Item[]): Placement => labels?.at[wkey(w)] ?? { shift: items.map(() => 0), sepAt: null };

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
  <div class="bandrow" class:cramped bind:this={rowEl} style={labels ? `--lab-fs: ${labels.fit.fs}px; --lab-ls: ${labels.fit.ls}px` : undefined}>
    <!-- the label texts in the rendered font, measured for placing the numbers (never shown) -->
    <div class="measure" aria-hidden="true" bind:this={measurer}>
      <span class="mtags">
        {#each MEASURED_BANDS as b (b.id)}<span class:quiet={b.id === 'ch2284'} data-m-band={b.id}>{b.nm}</span>{/each}
        <span data-m-tail>*</span><span data-m-sep>{PAIR_SEP}</span>
      </span>
      <span class="mcut">
        {#each CLOSEUPS as cu (cu.family)}<span data-m-title={cu.family}>{cu.family}</span>{#if cu.short}<span data-m-title={cu.short}>{cu.short}</span>{/if}{/each}
      </span>
    </div>
    {#each groups as g, gi (g.cu.family)}
      {@const cu = g.cu}
      <div class="cu" class:dip={cu.dip} style={cellStyle(cu)}>
        <div class="cut">{#if cu.short}<span class="long">{cu.family}</span><span class="short" title="{cu.family} bands">{cu.short}</span>{:else}{cu.family}{/if}</div>
        <div class="wins">
          {#each g.wins as { w, items }, wi (wkey(w))}
            {@const pl = placeOf(w, items)}
            {#if wi > 0}
              {@const prev = g.wins[wi - 1].w}
              <div class="brk" title="Axis break: {prev.range[1]}–{w.range[0]} nm is left out (it carries neither band)" aria-hidden="true">
                <svg width="10" height="14" viewBox="0 0 10 14"><path d="M1 10 L5 2 M5 12 L9 4" /></svg>
              </div>
            {/if}
            <div class="win" style="flex: {w.range[1] - w.range[0]} 1 0" data-range="{w.range[0]}-{w.range[1]}" data-key={wkey(w)}>
              <div class="tags">
                {#each items as it, i (it.b.id)}
                  <button class="tag" class:quiet={it.b.id === 'ch2284'} class:hl={highlight === it.k} style="left: {tagLeft(w, it.b.nm)}"
                    aria-label={bandTitle(it.b)} data-band={it.b.id} data-nm={it.b.nm}
                    onmouseenter={() => onHighlight(it.k)} onmouseleave={() => onHighlight(-1)} onfocus={() => onHighlight(it.k)} onblur={() => onHighlight(-1)}>
                    <span class="nm" style="--shift: {(pl.shift[i] ?? 0).toFixed(2)}px">{it.b.nm}{#if it.b.id === 'nh1545'}<span class="ast">*</span>{/if}</span>
                    <span class="g"><EvidenceGlyph state={it.b.state} size={10} /></span>
                    <!-- the tooltip, on hover and on keyboard focus (the label carries the same text for screen readers) -->
                    <span class="tip" class:end={gi >= groups.length / 2} aria-hidden="true">{keepRanges(bandTitle(it.b))}</span>
                  </button>
                {/each}
                {#if pl.sepAt !== null}
                  <span class="sep" style="left: {pl.sepAt.toFixed(2)}px" aria-hidden="true">{PAIR_SEP}</span>
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
    position: relative;
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
    color: var(--ink-2);
  }
  .cut,
  .mcut {
    font-size: var(--fs-xs);
    font-weight: 650;
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
  .tags,
  .mtags {
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    line-height: var(--line-h);
    white-space: nowrap;
  }
  /* the row's label type: the design size, or a little tighter / smaller where the measured numbers need it (placeRow) */
  .tags {
    position: relative;
    height: var(--lane);
    margin-top: 2px;
    font-size: var(--lab-fs, 10.5px);
    letter-spacing: var(--lab-ls, 0px);
  }
  /* the measurer: the label texts at the design size and the titles, in the rendered font; laid out, never seen, and
     taking no room (each text keeps its own width inside a box of none) */
  .measure {
    position: absolute;
    left: 0;
    top: 0;
    width: 0;
    height: 0;
    overflow: hidden;
    visibility: hidden;
    pointer-events: none;
  }
  .measure span span {
    display: block;
    width: max-content;
  }
  .mtags .quiet {
    font-weight: 500;
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
    /* a button's UA style resets the tracking; the row's label type (placeRow) may tighten it */
    letter-spacing: inherit;
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
    letter-spacing: normal;
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
