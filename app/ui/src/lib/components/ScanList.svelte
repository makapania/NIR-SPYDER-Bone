<script lang="ts">
  // Flat scan list (no grouping, no data entry). Each row: verdict glyph, reading per the display
  // policy, evidence strip, file name, time, the FLAG mark when a contaminant or heat sign fired, and a
  // small "ZooMS" mark when the scan has a ZooMS line (DECISIONS 80 amended). Rank badges only in
  // "most promising first".
  import EvidenceGlyph from './EvidenceGlyph.svelte';
  import FlagMark from './FlagMark.svelte';
  import VerdictGlyph from './VerdictGlyph.svelte';
  import Icon from './Icon.svelte';
  import { formatReading, shortName, timeOf } from '../display';
  import { zoomsLine } from '../notes';
  import { flagLabel, noteText } from '../strings';
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
  }
  let { scans, total, selectedId, newIds, arrivedId, sortMode, organicOnly, onSelect, onSort, onOrganic }: Props = $props();

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
  const ROW_FLAG: Record<string, string> = { wax: 'wax', ester: 'consolidant', plaster: 'plaster', c1: 'organic', burnt: 'heat' };
  const lifted = (s: ScanResult) => s.verdictRule === 'lift_good' || s.verdictRule === 'lift_borderline';

  $effect(() => {
    void selectedId;
    listEl?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
  });
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
      <button class="srow" class:arrived={s.scanId === arrivedId} aria-current={s.scanId === selectedId}
        onclick={() => onSelect(s.scanId)} data-scan={s.file}>
        <span class="gl">{#if isUnscored(s)}<span class="unscored" title={s.scanKind === 'reference'
              ? 'Reference scan (not scored)'
              : s.scanKind === 'unscored'
                ? 'Not scored: no verdict model is available'
                : 'Not a readable ASD scan (not scored)'}><Icon name="file" size={16} color="var(--ink-3)" /></span>{:else}<VerdictGlyph verdict={s.verdict} size={18} />{/if}</span>
        <span class="v">
          {#if sortMode === 'promising'}<span class="rank">{i + 1}</span>{/if}
          <span class="val">{rowValue(s)}</span>
          {#each fired(s) as f (f.id)}
            <span class="flag" title={flagLabel(f.id)}><FlagMark size={12} /><span class="ft">{ROW_FLAG[f.id]}</span></span>
          {/each}
          {#if !isUnscored(s)}
            {@const zl = zoomsLine(s.notes)}
            {#if zl}<span class="zmark" data-zooms={zl.key} title={noteText(zl)}><Icon name="zooms" size={11} color="var(--accent)" /><span class="zt">ZooMS</span></span>{/if}
          {/if}
          {#if newIds.has(s.scanId)}
            {#if (s.fileRevision ?? 1) > 1}
              <span class="newtag" title="The file changed on disk; this is revision {s.fileRevision}">Changed</span>
            {:else}
              <span class="newtag">New</span>
            {/if}
          {/if}
        </span>
        <span class="f">
          <span class="mono name">{shortName(s.file)}</span>
          {#if lifted(s)}<span title="Raised by the collagen bands" class="lift"><Icon name="up" size={12} color="var(--accent)" /></span>{/if}
          <span class="spacer"></span>
          <span class="strip">
            {#if s.evidence.bands.length}
              {#each s.evidence.bands as b, k (b.id)}
                {#if k === 2 || k === 4 || k === 6}<i></i>{/if}
                <EvidenceGlyph state={b.state} size={8} />
              {/each}
            {/if}
          </span>
          <span class="time">{timeOf(s.acquiredAt)}</span>
        </span>
      </button>
    {/each}
    {#if !scans.length}<p class="empty muted">No scans match.</p>{/if}
  </div>
  <div class="kbdhint"><kbd>↑</kbd> <kbd>↓</kbd> flip through scans</div>
</aside>

<style>
  .scans {
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
    gap: 1px 10px;
    align-items: center;
    padding: 7px 10px;
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
  }
  .v {
    font-weight: 650;
    font-size: 14.5px;
    display: flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
    white-space: nowrap;
  }
  /* The reading wins: it keeps at least ~"≈ 1.5%"; flag chips shrink to their mark first (full label in the
     tooltip and the verdict panel). */
  .val {
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 0 1 auto;
    min-width: 3.6em;
  }
  .newtag,
  .rank {
    flex: none;
  }
  .newtag {
    margin-left: auto;
  }
  .unscored {
    display: inline-block;
    line-height: 0;
  }
  .newtag {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--accent);
    background: var(--accent-soft);
    padding: 0 6px;
    border-radius: 999px;
  }
  .strip {
    flex: none;
    margin-right: 6px;
    display: inline-flex;
    gap: 2px;
    align-items: center;
  }
  .strip i {
    width: 3px;
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
  .ft {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    flex: 0 1 auto;
    min-width: 10ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .flag {
    flex: 0 1 auto;
    min-width: 22px;
    overflow: hidden;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 0 6px 0 4px;
    border-radius: 999px;
    background: var(--flag-tint);
    color: var(--flag-ink);
    box-shadow: inset 0 0 0 1px var(--flag-edge);
    font-weight: 650;
    font-size: 10.5px;
    line-height: 16px;
  }
  .lift {
    display: inline-flex;
  }
  /* the ZooMS line, in the list: small, accent-coloured, never a verdict colour; shrinks to its mark first */
  .zmark {
    flex: 0 1 auto;
    min-width: 19px;
    overflow: hidden;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 0 6px 0 4px;
    border-radius: 999px;
    color: var(--accent);
    background: var(--accent-soft);
    font-weight: 650;
    font-size: 10.5px;
    line-height: 16px;
  }
  .time {
    flex: none;
  }
  .empty {
    padding: 12px;
  }
  /* narrow windows: the file name (often a scan's only identity) wins over the mini strip, and the
     sort labels drop "first" so both fit */
  @media (max-width: 1300px) {
    .strip {
      display: none;
    }
    /* flags become the mark alone (label in the tooltip and the verdict panel) so the reading stays whole */
    .ft,
    .zt {
      display: none;
    }
    .zmark {
      padding: 0 4px;
    }
    .flag {
      padding: 0 4px;
    }
    .opt {
      display: none;
    }
  }
  .kbdhint {
    padding: 8px 16px 10px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
    border-top: 1px solid var(--line-soft);
  }
</style>
