<script lang="ts">
  // The flag mark (DECISIONS 53–54): one consistent, crafted swallow-tailed pennant on a slim staff,
  // in the dedicated flag colour. Used for every contaminant and heat sign; the words say which.
  // `on` draws it for a filled chip (the mark in the chip's "on" colour).
  let { size = 14, on = false }: { size?: number; on?: boolean } = $props();
  const col = $derived(on ? 'var(--flag-on)' : 'var(--flag)');
</script>

<svg width={size} height={size} viewBox="0 0 16 16" aria-hidden="true" class="flagmark">
  <line x1="3.6" y1="2.2" x2="3.6" y2="14.2" stroke={col} stroke-width="1.5" stroke-linecap="round" />
  <path
    d="M4.4 2.6 C6.8 1.9 9.6 3.5 13.4 2.7 L10.9 6.05 L13.4 9.4 C9.6 10.2 6.8 8.6 4.4 9.3 Z"
    fill={col}
  />
</svg>

<style>
  .flagmark {
    display: block;
    flex: none;
  }
</style>
