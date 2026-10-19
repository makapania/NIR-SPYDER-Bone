<script lang="ts">
  // Flat scan list (no grouping, no data entry). Each row: verdict glyph (with the rank under it in "most promising
  // first"), the reading per the display policy, then compact marks, each a symbol with the full text in its tooltip and
  // accessible name (Matt, 2026-10-18: never cut-off words): the FLAG pennant when a contaminant or heat sign fired, the
  // ZooMS mark (DECISIONS 80 amended), the strong panel note, NEW, and the band summary; then the file name (often a
  // scan's only identity, so it gets the whole second line) and time. The list can be widened by its right edge.
  import BandSummary from './BandSummary.svelte';
  import FlagMark from './FlagMark.svelte';
  import VerdictGlyph from './VerdictGlyph.svelte';
  import Icon from './Icon.svelte';
  import { formatReading, shortName, splitName, timeOf } from '../display';
  import { strongPanelNote, zoomsLine } from '../notes';
  import { flagLabel, flagText, noteText } from '../strings';
  import { isUnscored, type SortMode } from '../sort';
  import type { ScanResult } from '../types';

  interface Props {
    scans: ScanResult[];
    total: number;
    selectedId: string | null;
    /** Scans tagged NEW: live arrivals not yet clicked, or the newest example scan. */
    newIds: ReadonlySet<string>;
    arrivedId: string | null;
    sortMode: SortMode;
    organicOnly: boolean;
    onSelect: (id: string) => void;
    onSort: (m: SortMode) => void;
    onOrganic: () => void;
    /** The list's width as shown and its bounds (the drag handle on its right edge). */
    width: number;
    widthMin: number;
    widthMax: number;
    onWidth: (w: number) => void;
    onWidthReset: () => void;
  }
  let {
    scans,
    total,
    selectedId,
    newIds,
    arrivedId,
    sortMode,
    organicOnly,
    onSelect,
    onSort,
    onOrganic,
    width,
    widthMin,
    widthMax,
    onWidth,
    onWidthReset,
  }: Props = $props();

  let listEl: HTMLDivElement;

  function rowValue(s: ScanResult): string {
    if (s.scanKind === 'reference') return 'Reference';
    if (s.scanKind === 'unreadable') return 'Unreadable';
    if (s.scanKind === 'unscored') return 'Not scored';
    if (s.verdict === 'rescan') return 'Rescan';
    if (s.verdict === 'not_bone') return 'Not bone?';
    if (s.verdictRule === 'flat_bands') return 'No protein signal';
    return formatReading(s.models.cons3?.value, true);
  }
  const fired = (s: ScanResult) => s.signs.filter((x) => x.fired);
  /** Every fired sign in full, for the pennant's tooltip and accessible name. */
  const flagsText = (s: ScanResult) =>
    fired(s)
      .map((f) => `${flagLabel(f.id)}: ${flagText(f.id, f.heatKind)}`)
      .join(' ');
  const lifted = (s: ScanResult) => s.verdictRule === 'lift_good' || s.verdictRule === 'lift_borderline';

  $effect(() => {
    void selectedId;
    listEl?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
  });

  // the width handle: pointer drag, arrow keys (Shift: bigger steps), Home/End, Enter or a double-click resets
  let drag: { x: number; w: number } | null = null;
  function grab(e: PointerEvent) {
    drag = { x: e.clientX, w: width };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }
  function move(e: PointerEvent) {
    if (drag) onWidth(drag.w + e.clientX - drag.x);
  }
  function release() {
    drag = null;
  }
  function handleKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    if (e.key === 'ArrowRight') onWidth(width + step);
    else if (e.key === 'ArrowLeft') onWidth(width - step);
    else if (e.key === 'Home') onWidth(widthMin);
    else if (e.key === 'End') onWidth(widthMax);
    else if (e.key === 'Enter') onWidthReset();
    else return;
    e.preventDefault();
  }
</script>

<aside class="scans" aria-label="Scans">
  <header>
    <div class="listhead"><span class="eyebrow">Scans</span><span class="muted count">{total} scans</span></div>
    <div class="seg full" role="group" aria-label="Order">
      <button aria-pressed={sortMode === 'newest'} onclick={() => onSort('newest')}>Newest<span class="opt">{' first'}</span></button>
      <button aria-pressed={sortMode === 'promising'} onclick={() => onSort('promising')}>Most promising<span class="opt">{' first'}</span></button>
    </div>
    <button class="chip" aria-pressed={organicOnly} onclick={onOrganic}
      title="Show only scans with at least a trace of protein signal">Any organic signal</button>
  </header>
  <div class="group eyebrow">{sortMode === 'promising' ? 'Most promising first' : 'Newest first'}{organicOnly ? ' · any organic signal' : ''}</div>
  <div class="list" bind:this={listEl}>
    {#each scans as s, i (s.scanId)}
      {@const nm = splitName(shortName(s.file))}
      <button class="srow" class:arrived={s.scanId === arrivedId} aria-current={s.scanId === selectedId}
        onclick={() => onSelect(s.scanId)} data-scan={s.file}>
        <span class="gl">{#if isUnscored(s)}<span class="unscored" title={s.scanKind === 'reference'
              ? 'Reference scan (not scored)'
              : s.scanKind === 'unscored'
                ? 'Not scored: no verdict model is available'
                : 'Not a readable ASD scan (not scored)'}><Icon name="file" size={16} color="var(--ink-3)" /></span>{:else}<VerdictGlyph verdict={s.verdict} size={18} />{/if}{#if sortMode === 'promising'}<span
              class="rank" title="{i + 1} in the most-promising order">{i + 1}</span>{/if}</span>
        <span class="v">
          <span class="val">{rowValue(s)}</span>
          <!-- the marks, in the order they give way when the row is narrow: the band summary first, then NEW, ZooMS, and last
               the strong-note warning and the flag; a mark that does not fit is left out whole, never cut -->
          <span class="marks">
            {#if fired(s).length}
              {@const ft = flagsText(s)}
              <span class="mk flag" role="img" aria-label={ft} title={ft}><FlagMark size={12} />{#if fired(s).length > 1}<b>{fired(s).length}</b>{/if}</span>
            {/if}
            {#if !isUnscored(s)}
              {@const pn = strongPanelNote(s.notes)}
              {#if pn}<span class="mk pmark" role="img" data-panel={pn.key} aria-label={noteText(pn, s)} title={noteText(pn, s)}><Icon
                    name={pn.key === 'ch_above_nh_strong' ? 'flask' : 'disagree'} size={12} color="var(--warn-on)" /></span>{/if}
              {@const zl = zoomsLine(s.notes)}
              {#if zl}<span class="mk zmark" role="img" data-zooms={zl.key} aria-label="ZooMS: {noteText(zl)}" title={noteText(zl)}><Icon name="zooms" size={12} color="var(--accent)" /></span>{/if}
            {/if}
            {#if newIds.has(s.scanId)}
              {#if (s.fileRevision ?? 1) > 1}
                <span class="newtag" title="The file changed on disk; this is revision {s.fileRevision}">Rev {s.fileRevision}</span>
              {:else}
                <span class="newtag" title="New since you last looked">New</span>
              {/if}
            {/if}
            {#if !isUnscored(s)}<span class="sum"><BandSummary evidence={s.evidence} /></span>{/if}
          </span>
        </span>
        <span class="f">
          <span class="mono name" title={s.file}><span class="nh">{nm.head}</span>{#if nm.tail}<span class="nt">{nm.tail}</span>{/if}</span>
          {#if lifted(s)}<span title="Raised by the collagen bands" class="lift"><Icon name="up" size={12} color="var(--accent)" /></span>{/if}
          <span class="spacer"></span>
          <span class="time">{timeOf(s.acquiredAt)}</span>
        </span>
      </button>
    {/each}
    {#if !scans.length}<p class="empty muted">No scans match.</p>{/if}
  </div>
  <div class="kbdhint"><kbd>↑</kbd> <kbd>↓</kbd> flip through scans</div>
  <div class="grip" role="slider" aria-orientation="horizontal" aria-label="Scans list width" aria-valuetext="{width} pixels" aria-valuenow={width} aria-valuemin={widthMin}
    aria-valuemax={widthMax} tabindex="0" title="Drag to widen the list (double-click: back to the default width)"
    onpointerdown={grab} onpointermove={move} onpointerup={release} onpointercancel={release} ondblclick={onWidthReset} onkeydown={handleKey}></div>
</aside>

<style>
  .scans {
    position: relative;
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--panel-2);
  }
  header {
    padding: 14px 14px 6px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: stretch;
  }
  header .chip {
    align-self: flex-start;
  }
  .listhead {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .count {
    font-size: var(--fs-xs);
  }
  .group {
    padding: 8px 18px 4px;
  }
  .list {
    overflow: auto;
    padding: 0 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    min-height: 0;
  }
  .srow {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr);
    gap: 1px 8px;
    align-items: center;
    padding: 7px 8px 7px 9px;
    border-radius: 9px;
    cursor: pointer;
    border: 0;
    background: none;
    text-align: left;
    width: 100%;
  }
  .srow:hover {
    background: var(--line-soft);
  }
  .srow[aria-current='true'] {
    background: var(--panel);
    box-shadow: 0 0 0 1px var(--line), 0 1px 2px rgba(0, 0, 0, 0.06);
  }
  .srow.arrived {
    animation: arrive 0.8s var(--ease);
  }
  @keyframes arrive {
    from {
      background: var(--accent-soft);
      transform: translateY(-4px);
    }
    to {
      transform: none;
    }
  }
  .gl {
    grid-row: 1 / 3;
    line-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
  }
  .gl .rank {
    min-width: 20px;
    height: 15px;
    padding: 0 2px;
    font-size: 10px;
    line-height: 15px;
  }
  .v {
    font-weight: 650;
    font-size: 14.5px;
    display: flex;
    gap: 5px;
    align-items: center;
    min-width: 0;
    white-space: nowrap;
  }
  /* the reading is never cut or squeezed: the marks give way instead */
  .val {
    flex: none;
  }
  /* One line of marks; a mark that does not fit wraps onto a hidden second line, so it is left out whole (the verdict card
     still shows it). The DOM order is the order they give way: the band summary first, the flag last. */
  .marks {
    flex: 1 1 0;
    min-width: 0;
    height: 18px;
    overflow: hidden;
    display: flex;
    flex-wrap: wrap;
    align-content: flex-start;
    align-items: center;
    gap: 0 4px;
  }
  .marks > * {
    flex: none;
    height: 18px;
    display: inline-flex;
    align-items: center;
  }
  /* the band summary keeps its own column at the row's right end, over the time */
  .sum {
    margin-left: auto;
    padding-left: 2px;
  }
  .unscored {
    display: inline-block;
    line-height: 0;
  }
  .marks > .newtag {
    height: 16px;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--accent);
    background: var(--accent-soft);
    padding: 0 6px;
    border-radius: 999px;
  }
  .f {
    grid-column: 2;
    font-size: var(--fs-xs);
    color: var(--ink-3);
    display: flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
  }
  /* a long name loses its middle, never its end (the counter, spot or repeat that tells scans apart) */
  .name {
    flex: 0 1 auto;
    display: inline-flex;
    min-width: 10ch;
    overflow: hidden;
    white-space: nowrap;
  }
  .nh {
    flex: 0 1 auto;
    min-width: 3ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .nt {
    flex: none;
  }
  /* the whole name: the tooltip on hover, wrapped in place while the row has keyboard focus */
  .srow:focus-visible .name {
    display: block;
    white-space: normal;
    overflow-wrap: anywhere;
  }
  .lift {
    display: inline-flex;
  }
  /* a mark: a fixed symbol in a small pill; the full text is its tooltip and accessible name */
  .marks > .mk {
    height: 16px;
    min-width: 20px;
    justify-content: center;
    gap: 2px;
    padding: 0 4px;
    border-radius: 999px;
  }
  /* the flag: the pennant in the flag colour (DECISIONS 53, 54), with a count when more than one sign fired */
  .mk.flag {
    background: var(--flag-tint);
    box-shadow: inset 0 0 0 1px var(--flag-edge);
    color: var(--flag-ink);
  }
  .mk.flag b {
    font-size: 10px;
    font-weight: 750;
    line-height: 1;
  }
  /* the ZooMS line: the peptide-peaks mark in the accent, never a verdict colour */
  .mk.zmark {
    background: var(--accent-soft);
  }
  /* a strong panel note (C–H "likely": a flask; readers disagree strongly: not equal): a warning, solid in the warning colour
     with a square-cornered pill (the flag's is round, with the pennant) */
  .marks > .mk.pmark {
    background: var(--warn);
    border-radius: 4px;
  }
  .time {
    flex: none;
  }
  .empty {
    padding: 12px;
  }
  /* narrow windows: the sort labels drop "first" so both fit */
  @media (max-width: 1300px) {
    .opt {
      display: none;
    }
  }
  /* the width handle: a thin strip on the right edge, a line when hovered, dragged or focused */
  .grip {
    position: absolute;
    top: 0;
    right: -4px;
    bottom: 0;
    width: 8px;
    cursor: col-resize;
    z-index: 3;
    touch-action: none;
  }
  .grip::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    background: transparent;
    transition: background 0.15s;
  }
  .grip:hover::after,
  .grip:active::after,
  .grip:focus-visible::after {
    background: var(--accent);
  }
  .grip:focus-visible {
    outline: none;
  }
  .kbdhint {
    padding: 8px 16px 10px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
    border-top: 1px solid var(--line-soft);
  }
</style>
