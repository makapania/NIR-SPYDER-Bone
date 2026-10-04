<script lang="ts">
  // Evidence glyph: fill AREA shows band strength (trace / clear / strong); a dash means flat; a
  // dashed ring means too noisy to read. The band hue (--band) and ink only, never verdict colours.
  import type { BandState, EvidenceLevel } from '../types';

  type S = BandState | EvidenceLevel;
  let { state, size = 14 }: { state: S; size?: number } = $props();
  const c = $derived(size / 2);
  const R = $derived(size / 2 - 1);
  const frac: Record<string, number> = { trace: 0.3, clear: 0.62 };
</script>

<svg width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true" class="evglyph">
  {#if state === 'cant_tell'}
    <circle cx={c} cy={c} r={R - 0.5} fill="none" stroke="var(--ink-3)" stroke-width="1.2" stroke-dasharray="2 1.8" />
  {:else if state === 'strong'}
    <circle cx={c} cy={c} r={R} fill="var(--band)" />
  {:else}
    <circle cx={c} cy={c} r={R - 0.5} fill="none" stroke={state === 'flat' || state === 'none' ? 'var(--ink-3)' : 'var(--band)'} stroke-width="1.2" />
    {#if state === 'flat' || state === 'none'}
      <line x1={c - R * 0.5} x2={c + R * 0.5} y1={c} y2={c} stroke="var(--ink-3)" stroke-width="1.4" stroke-linecap="round" />
    {:else}
      <circle cx={c} cy={c} r={R * Math.sqrt(frac[state] ?? 0.3)} fill="var(--band)" />
    {/if}
  {/if}
</svg>

<style>
  .evglyph {
    display: block;
    flex: none;
  }
</style>
