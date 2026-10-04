<script lang="ts">
  // Verdict glyph: the SHAPE carries the verdict (full / half / third disc, "?" ring, dashed ring,
  // dashed diamond); colour is redundant. Port of the design mockup's glyph().
  import { verdictMeta } from '../verdict';
  import type { Verdict } from '../types';

  let { verdict, size = 16 }: { verdict: Verdict; size?: number } = $props();

  const meta = $derived(verdictMeta(verdict));
  const c = $derived(size / 2);
  const r = $derived(size / 2 - 1);
  const sw = $derived(size > 30 ? 2.5 : 1.5);
  const col = $derived(`var(--v-${meta.tone})`);
  const fillPath = $derived.by(() => {
    const f = meta.fill;
    if (f <= 0 || f >= 1) return '';
    const y0 = c + r - 2 * r * f;
    const dx = Math.sqrt(Math.max(0, r * r - (y0 - c) ** 2));
    return `M${c - dx} ${y0} A${r} ${r} 0 ${f > 0.5 ? 1 : 0} 0 ${c + dx} ${y0} Z`;
  });
</script>

<svg width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true" class="vglyph">
  {#if meta.shape === 'full'}
    <circle cx={c} cy={c} r={r} fill={col} />
  {:else if meta.shape === 'half' || meta.shape === 'third'}
    <circle cx={c} cy={c} r={r - sw / 2 + 0.5} fill="var(--v-{meta.tone}-tint)" stroke={col} stroke-width={sw} />
    <path d={fillPath} fill={col} />
  {:else if meta.shape === 'question'}
    <circle cx={c} cy={c} r={r - 0.3} fill="var(--v-none-tint)" stroke="var(--v-none)" stroke-width={sw} />
    <text x={c} y={c + size * 0.2} text-anchor="middle" font-size={size * 0.58} font-weight="700" fill="var(--v-none-ink)"
      font-family="var(--font-ui)">?</text>
  {:else if meta.shape === 'dashed'}
    <circle cx={c} cy={c} r={r} fill="none" stroke={col} stroke-width={sw} stroke-dasharray="{size / 7} {size / 8}" />
  {:else}
    <path d="M{c} {1 + sw / 2} L{size - 1 - sw / 2} {c} L{c} {size - 1 - sw / 2} L{1 + sw / 2} {c} Z" fill="var(--v-none-tint)"
      stroke={col} stroke-width={sw} stroke-dasharray="{size / 8} {size / 10}" stroke-linejoin="round" />
  {/if}
</svg>

<style>
  .vglyph {
    display: block;
    flex: none;
  }
</style>
