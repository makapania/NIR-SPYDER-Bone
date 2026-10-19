<script lang="ts">
  // The scans list's band summary (Matt, 2026-10-18: one shorthand instead of seven dots, so long file names fit; the bars
  // were chosen, DECISIONS 126). One bar per band in the close-ups' order (1545 | C–H | protein | combination): height =
  // strength, so it reads without colour; a stub is flat, a dotted outline too noisy. A quick look, not a vote count (the
  // tooltip says so). The band hue and ink only, never verdict colours.
  import { bandSummaryTitle, bandsByCloseUp } from '../bands';
  import type { BandState, Evidence } from '../types';

  let { evidence }: { evidence: Evidence } = $props();

  const LEAD = 'One bar per band, left to right 1545 | 1689 1728 | 2044 2175 | 2262 2284: taller = stronger, a stub = flat, dotted = too noisy.';
  const title = $derived(`${LEAD} ${bandSummaryTitle(evidence.level, evidence.bands)}`);
  const groups = $derived(bandsByCloseUp(evidence.bands));
  const BAR: Record<BandState, number> = { strong: 12, clear: 8.5, trace: 5, flat: 1.6, cant_tell: 12 };
  /** Each bar's x: 4 px a bar, 3 px more between close-ups. */
  const xs = $derived.by(() => {
    const out: number[][] = [];
    let x = 0;
    for (const g of groups) {
      out.push(g.map((_, i) => x + i * 4));
      x += g.length * 4 + 3;
    }
    return { at: out, width: Math.max(0, x - 4) };
  });
</script>

{#if evidence.bands.length}
  <span class="bsum" {title}>
    <svg width={xs.width} height="13" aria-hidden="true">
      {#each groups as g, gi (gi)}
        {#each g as b, bi (b.id)}
          {@const x = xs.at[gi][bi]}
          {#if b.state === 'cant_tell'}
            <rect x={x + 0.5} y="1.5" width="2" height="11" fill="none" stroke="var(--ink-3)" stroke-width="0.9" stroke-dasharray="1.6 1.4" />
          {:else}
            <rect x={x} y={13 - BAR[b.state]} width="3" height={BAR[b.state]} rx="0.6" fill={b.state === 'flat' ? 'var(--ink-4)' : 'var(--band)'} />
          {/if}
        {/each}
      {/each}
    </svg>
  </span>
{/if}

<style>
  .bsum {
    flex: none;
    display: inline-flex;
    align-items: center;
    line-height: 0;
  }
  .bsum svg {
    display: block;
  }
</style>
